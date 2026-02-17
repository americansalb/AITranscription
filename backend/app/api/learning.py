"""Learning system API routes."""
from datetime import date
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.database import get_db
from app.models.user import User
from app.models.learning import LearningMetrics
from app.api.auth import get_current_user
from app.services.correction_retriever import CorrectionRetriever
from app.services.audio_collector import AudioCollector

router = APIRouter(prefix="/learning", tags=["learning"])


# Request/Response schemas
class FeedbackRequest(BaseModel):
    """Request to submit correction feedback."""

    original_text: str
    corrected_text: str
    audio_sample_id: Optional[int] = None


class FeedbackResponse(BaseModel):
    """Response after storing feedback."""

    success: bool
    correction_id: Optional[int] = None
    message: str


class CorrectionListItem(BaseModel):
    """A single correction in the list."""

    id: int
    original_text: str
    corrected_text: str
    correction_type: Optional[str]
    correction_count: int
    created_at: Optional[str]


class LearningStatsResponse(BaseModel):
    """Learning statistics for the user."""

    # Correction stats
    total_corrections: int
    unique_types: int
    total_applications: int
    corrections_by_type: dict

    # Audio stats
    audio_samples: int
    audio_duration_seconds: float
    ready_for_whisper_training: bool

    # Model info
    correction_model_version: Optional[str] = None
    whisper_model_version: Optional[str] = None


class CorrectionsListResponse(BaseModel):
    """List of learned corrections."""

    corrections: list[CorrectionListItem]
    total: int


@router.post("/feedback", response_model=FeedbackResponse)
async def submit_feedback(
    request: FeedbackRequest,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Submit a correction to improve the learning system.

    Call this when the user edits the polished text to provide
    feedback that the original polishing was incorrect.
    """
    if request.original_text.strip() == request.corrected_text.strip():
        return FeedbackResponse(
            success=False,
            message="Original and corrected text are identical",
        )

    retriever = CorrectionRetriever(db, user.id)

    try:
        correction = await retriever.store_correction(
            original=request.original_text,
            corrected=request.corrected_text,
            audio_sample_id=request.audio_sample_id,
        )

        # Update daily metrics
        await _update_daily_metrics(db, user.id, correction_added=True)

        return FeedbackResponse(
            success=True,
            correction_id=correction.id if correction else None,
            message="Correction stored successfully",
        )
    except Exception as e:
        import logging
        logging.getLogger(__name__).error("Failed to store feedback: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=500, detail="Failed to store feedback")


@router.get("/stats", response_model=LearningStatsResponse)
async def get_learning_stats(
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Get learning statistics for the current user.

    Shows how much the system has learned from this user's corrections.
    """
    retriever = CorrectionRetriever(db, user.id)
    audio_collector = AudioCollector(db, user.id)

    correction_stats = await retriever.get_correction_stats()
    audio_stats = await audio_collector.get_sample_stats()

    # Get latest model versions
    from sqlalchemy import select
    from app.models.learning import ModelVersion

    correction_model = await db.execute(
        select(ModelVersion)
        .where(
            ModelVersion.user_id == user.id,
            ModelVersion.model_type == "correction_nn",
        )
        .order_by(ModelVersion.version.desc())
        .limit(1)
    )
    correction_model = correction_model.scalar_one_or_none()

    whisper_model = await db.execute(
        select(ModelVersion)
        .where(
            ModelVersion.user_id == user.id,
            ModelVersion.model_type == "whisper_lora",
        )
        .order_by(ModelVersion.version.desc())
        .limit(1)
    )
    whisper_model = whisper_model.scalar_one_or_none()

    return LearningStatsResponse(
        total_corrections=correction_stats["total_corrections"],
        unique_types=correction_stats["unique_types"],
        total_applications=correction_stats["total_applications"],
        corrections_by_type=correction_stats["by_type"],
        audio_samples=audio_stats["total_samples"],
        audio_duration_seconds=audio_stats["total_duration_seconds"],
        ready_for_whisper_training=audio_stats["ready_for_whisper_training"],
        correction_model_version=f"v{correction_model.version}"
        if correction_model
        else None,
        whisper_model_version=f"v{whisper_model.version}" if whisper_model else None,
    )


@router.get("/corrections", response_model=CorrectionsListResponse)
async def get_learned_corrections(
    limit: int = 50,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Get list of learned corrections for this user.

    Users can review what the system has learned and delete incorrect entries.
    """
    retriever = CorrectionRetriever(db, user.id)
    corrections = await retriever.get_recent_corrections(limit=limit)

    stats = await retriever.get_correction_stats()

    return CorrectionsListResponse(
        corrections=[
            CorrectionListItem(**c) for c in corrections
        ],
        total=stats["total_corrections"],
    )


@router.delete("/corrections/{correction_id}")
async def delete_correction(
    correction_id: int,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Delete a learned correction.

    Use this to remove incorrect patterns the system has learned.
    """
    retriever = CorrectionRetriever(db, user.id)
    deleted = await retriever.delete_correction(correction_id)

    if not deleted:
        raise HTTPException(status_code=404, detail="Correction not found")

    return {"success": True, "message": "Correction deleted"}


@router.get("/similar")
async def find_similar_corrections(
    text: str,
    threshold: float = 0.6,
    limit: int = 5,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Find corrections similar to the given text.

    Useful for debugging and understanding what the system has learned.
    """
    retriever = CorrectionRetriever(db, user.id)
    similar = await retriever.find_similar(text, threshold=threshold, limit=limit)

    return {
        "query": text,
        "similar_corrections": similar,
    }


class RuleRequest(BaseModel):
    """Request to create a correction rule."""

    pattern: str
    replacement: str
    is_regex: bool = False
    priority: int = 0


class RuleResponse(BaseModel):
    """Response for a correction rule."""

    id: int
    pattern: str
    replacement: str
    is_regex: bool
    priority: int
    hit_count: int


class RulesListResponse(BaseModel):
    """List of correction rules."""

    rules: list[RuleResponse]
    total: int


class TrainModelRequest(BaseModel):
    """Request to train the correction model."""

    epochs: int = 10
    batch_size: int = 16
    learning_rate: float = 0.0001


class TrainModelResponse(BaseModel):
    """Response after training attempt."""

    success: bool
    message: str
    version: Optional[int] = None
    training_loss: Optional[float] = None
    training_samples: Optional[int] = None
    epochs_trained: Optional[int] = None


class WhisperTrainRequest(BaseModel):
    """Request to train Whisper model."""

    epochs: int = 3
    batch_size: int = 4
    learning_rate: float = 0.0001


@router.post("/train", response_model=TrainModelResponse)
async def train_correction_model(
    request: TrainModelRequest = TrainModelRequest(),
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Trigger training of the correction neural network.

    Requires at least 50 corrections to start training.
    Training runs in the foreground (may take a few minutes).
    """
    from app.training.correction_trainer import CorrectionTrainer

    trainer = CorrectionTrainer(db, user.id)

    try:
        result = await trainer.train(
            epochs=request.epochs,
            batch_size=request.batch_size,
            learning_rate=request.learning_rate,
        )

        if result is None:
            return TrainModelResponse(
                success=False,
                message="Not enough training samples. Need at least 50 corrections.",
            )

        return TrainModelResponse(
            success=True,
            message="Model trained successfully",
            version=result["version"],
            training_loss=result["training_loss"],
            training_samples=result["training_samples"],
            epochs_trained=result["epochs_trained"],
        )
    except Exception as e:
        import logging
        logging.getLogger(__name__).error("Training failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=500, detail="Training failed")


@router.post("/train-whisper", response_model=TrainModelResponse)
async def train_whisper_model(
    request: WhisperTrainRequest = WhisperTrainRequest(),
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Trigger LoRA fine-tuning of Whisper on user's audio samples.

    Requires:
    - At least 50 audio samples with corrections
    - GPU for training (CUDA)
    - PEFT library installed

    Training typically takes 10-30 minutes depending on sample count.
    """
    from app.training.whisper_finetuner import WhisperFineTuner

    finetuner = WhisperFineTuner(db, user.id)

    try:
        result = await finetuner.train(
            epochs=request.epochs,
            batch_size=request.batch_size,
            learning_rate=request.learning_rate,
        )

        if result is None:
            return TrainModelResponse(
                success=False,
                message="Not enough audio samples. Need at least 50 samples with corrections.",
            )

        return TrainModelResponse(
            success=True,
            message="Whisper model fine-tuned successfully",
            version=result["version"],
            training_loss=result["training_loss"],
            training_samples=result["training_samples"],
            epochs_trained=result["epochs_trained"],
        )
    except RuntimeError as e:
        import logging
        logging.getLogger(__name__).error("Whisper training RuntimeError: %s", e)
        raise HTTPException(status_code=400, detail="Training configuration error")
    except Exception as e:
        import logging
        logging.getLogger(__name__).error("Whisper training failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=500, detail="Training failed")


@router.get("/rules", response_model=RulesListResponse)
async def get_correction_rules(
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Get all correction rules for the current user."""
    from app.correctors.rule_based import RuleBasedCorrector

    corrector = RuleBasedCorrector(db, user.id)
    rules = await corrector.get_rules_list()

    return RulesListResponse(
        rules=[RuleResponse(**r) for r in rules],
        total=len(rules),
    )


@router.post("/rules", response_model=RuleResponse, status_code=201)
async def create_correction_rule(
    request: RuleRequest,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Create a new correction rule.

    Rules can be simple text patterns or regex patterns.
    Higher priority rules are applied first.
    """
    from app.correctors.rule_based import RuleBasedCorrector

    corrector = RuleBasedCorrector(db, user.id)

    try:
        rule = await corrector.add_rule(
            pattern=request.pattern,
            replacement=request.replacement,
            is_regex=request.is_regex,
            priority=request.priority,
        )

        return RuleResponse(
            id=rule.id,
            pattern=rule.pattern,
            replacement=rule.replacement,
            is_regex=rule.is_regex,
            priority=rule.priority,
            hit_count=rule.hit_count,
        )
    except ValueError as e:
        import logging
        logging.getLogger(__name__).error("Invalid correction rule: %s", e)
        raise HTTPException(status_code=400, detail="Invalid rule pattern")


@router.delete("/rules/{rule_id}")
async def delete_correction_rule(
    rule_id: int,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Delete a correction rule."""
    from app.correctors.rule_based import RuleBasedCorrector

    corrector = RuleBasedCorrector(db, user.id)
    deleted = await corrector.delete_rule(rule_id)

    if not deleted:
        raise HTTPException(status_code=404, detail="Rule not found")

    return {"success": True, "message": "Rule deleted"}


@router.post("/correct")
async def correct_text_hybrid(
    text: str,
    context: Optional[str] = None,
    use_llm: bool = True,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Correct text using the hybrid correction system.

    Routes through: rules -> ML model -> embeddings -> LLM
    Returns the corrected text and information about which layer was used.
    """
    from app.correctors.router import CorrectionRouter

    router = CorrectionRouter(db, user.id)
    result = await router.correct(text, context=context, use_llm_fallback=use_llm)

    return result


@router.get("/correct/breakdown")
async def get_correction_breakdown(
    text: str,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """Get detailed breakdown of what each correction layer would do.

    Useful for debugging and understanding the correction pipeline.
    """
    from app.correctors.router import CorrectionRouter

    router = CorrectionRouter(db, user.id)
    return await router.get_correction_breakdown(text)


async def _update_daily_metrics(
    db: AsyncSession,
    user_id: int,
    transcription_added: bool = False,
    correction_added: bool = False,
    auto_accepted: bool = False,
):
    """Update daily learning metrics."""
    from sqlalchemy import select

    today = date.today()

    # Get or create today's metrics
    result = await db.execute(
        select(LearningMetrics).where(
            LearningMetrics.user_id == user_id,
            LearningMetrics.date == today,
        )
    )
    metrics = result.scalar_one_or_none()

    if not metrics:
        metrics = LearningMetrics(
            user_id=user_id,
            date=today,
        )
        db.add(metrics)

    if transcription_added:
        metrics.transcriptions_count += 1
    if correction_added:
        metrics.corrections_count += 1
    if auto_accepted:
        metrics.auto_accepted += 1

    await db.commit()
