"""Hybrid correction router.

Routes corrections through multiple layers based on confidence:
0. Hallucination detection (very conservative, only obvious cases)
1. Rule-based (fast, deterministic)
2. ML model (fast, learned patterns)
3. Embedding retrieval (medium, similar corrections)
4. LLM (slow, complex cases)

Optimizes for speed by using faster methods when confident.
"""
import logging
from typing import Optional

from sqlalchemy.ext.asyncio import AsyncSession

from app.correctors.rule_based import RuleBasedCorrector, HallucinationDetector
from app.services.correction_retriever import CorrectionRetriever
from app.services.polish import polish_service
from app.training.correction_trainer import MLCorrector

logger = logging.getLogger(__name__)


class CorrectionRouter:
    """Routes text through appropriate correction methods based on confidence."""

    # Confidence thresholds for each layer
    RULE_CONFIDENCE_THRESHOLD = 0.8
    ML_CONFIDENCE_THRESHOLD = 0.7
    EMBEDDING_CONFIDENCE_THRESHOLD = 0.6
    # Very high threshold for hallucination removal - must be almost certain
    HALLUCINATION_CONFIDENCE_THRESHOLD = 0.9

    def __init__(self, db: AsyncSession, user_id: int):
        self.db = db
        self.user_id = user_id
        self.hallucination_detector = HallucinationDetector()
        self.rule_corrector = RuleBasedCorrector(db, user_id)
        self.ml_corrector = MLCorrector(db, user_id)
        self.retriever = CorrectionRetriever(db, user_id)

    async def correct(
        self,
        text: str,
        context: Optional[str] = None,
        use_llm_fallback: bool = True,
        audio_duration_seconds: Optional[float] = None,
    ) -> dict:
        """Route text through correction layers.

        Args:
            text: Input text to correct
            context: Optional context for LLM (e.g., 'email', 'slack')
            use_llm_fallback: Whether to use LLM for uncertain cases
            audio_duration_seconds: Duration of audio (helps detect hallucinations)

        Returns:
            Dict with corrected text, confidence, and source info
        """
        corrections_applied = []
        current_text = text
        final_confidence = 0.0
        source = "none"
        hallucination_warning = None

        # Layer 0: Hallucination detection (VERY conservative)
        # Only auto-correct if we're almost certain it's a hallucination
        hallucination_result = self.hallucination_detector.detect(
            text, audio_duration_seconds
        )

        if hallucination_result["is_hallucination"]:
            if hallucination_result["confidence"] >= self.HALLUCINATION_CONFIDENCE_THRESHOLD:
                # High confidence - auto-remove the hallucination
                current_text = hallucination_result["corrected"]
                corrections_applied.append({
                    "type": "hallucination_removal",
                    "confidence": hallucination_result["confidence"],
                    "detections": hallucination_result["detections"],
                })
                logger.info(
                    f"Hallucination removed (confidence {hallucination_result['confidence']:.2f}): "
                    f"'{text}' -> '{current_text}'"
                )

                # If text is now empty, return early
                if not current_text.strip():
                    return self._build_response(
                        original=text,
                        corrected="",
                        confidence=hallucination_result["confidence"],
                        source="hallucination_filter",
                        corrections=corrections_applied,
                        hallucination_warning="Detected and removed hallucination",
                    )
            else:
                # Lower confidence - just flag it, don't auto-remove
                hallucination_warning = (
                    f"Possible hallucination detected: {hallucination_result['detections']}"
                )
                logger.debug(f"Hallucination flagged (not removed): {hallucination_warning}")

        # Layer 1: Rule-based corrections (always apply)
        rule_result = await self.rule_corrector.correct(current_text)
        if rule_result["changed"]:
            current_text = rule_result["corrected"]
            corrections_applied.extend(rule_result["corrections"])
            final_confidence = rule_result["confidence"]
            source = "rule_based"
            logger.debug(
                f"Rule-based corrections applied: {rule_result['corrections_applied']}"
            )

            # If rule-based is confident, we might be done
            if final_confidence >= self.RULE_CONFIDENCE_THRESHOLD:
                return self._build_response(
                    original=text,
                    corrected=current_text,
                    confidence=final_confidence,
                    source=source,
                    corrections=corrections_applied,
                )

        # Layer 2: ML model correction
        if await self.ml_corrector.has_trained_model():
            ml_result = await self.ml_corrector.correct(current_text)
            if ml_result:
                if ml_result["confidence"] >= self.ML_CONFIDENCE_THRESHOLD:
                    current_text = ml_result["corrected"]
                    corrections_applied.append({
                        "type": "ml_model",
                        "confidence": ml_result["confidence"],
                        "model_version": ml_result["model_version"],
                    })
                    final_confidence = max(final_confidence, ml_result["confidence"])
                    source = "ml_model"
                    logger.debug(
                        f"ML correction applied with confidence {ml_result['confidence']:.2f}"
                    )

                    if final_confidence >= self.ML_CONFIDENCE_THRESHOLD:
                        return self._build_response(
                            original=text,
                            corrected=current_text,
                            confidence=final_confidence,
                            source=source,
                            corrections=corrections_applied,
                        )

        # Layer 3: Embedding-based retrieval (find similar past corrections)
        similar_corrections = await self.retriever.retrieve_relevant_corrections(
            current_text,
            top_k=3,
            threshold=self.EMBEDDING_CONFIDENCE_THRESHOLD,
        )
        if similar_corrections:
            # Apply the most similar correction if highly confident
            best_match = similar_corrections[0]
            if best_match["similarity"] >= 0.85:
                # Direct substitution for very high similarity
                current_text = best_match["corrected_text"]
                corrections_applied.append({
                    "type": "embedding_match",
                    "similarity": best_match["similarity"],
                    "correction_id": best_match["id"],
                })
                final_confidence = best_match["similarity"]
                source = "embedding_retrieval"
                logger.debug(
                    f"Embedding match applied with similarity {best_match['similarity']:.2f}"
                )

                return self._build_response(
                    original=text,
                    corrected=current_text,
                    confidence=final_confidence,
                    source=source,
                    corrections=corrections_applied,
                )

        # Layer 4: LLM fallback (for complex cases)
        if use_llm_fallback and current_text == text:
            # Only call LLM if no other corrections were made
            try:
                llm_result = await polish_service.polish(
                    raw_text=current_text,
                    context=context,
                    db=self.db,
                    user_id=self.user_id,
                )
                if llm_result["text"] != current_text:
                    current_text = llm_result["text"]
                    corrections_applied.append({
                        "type": "llm",
                        "corrections_used": llm_result.get("corrections_used", 0),
                    })
                    # LLM confidence is moderate (it's not perfect)
                    final_confidence = 0.7
                    source = "llm"
                    logger.debug("LLM polish applied")

            except Exception as e:
                logger.warning(f"LLM fallback failed: {e}")

        return self._build_response(
            original=text,
            corrected=current_text,
            confidence=final_confidence,
            source=source,
            corrections=corrections_applied,
            hallucination_warning=hallucination_warning,
        )

    def _build_response(
        self,
        original: str,
        corrected: str,
        confidence: float,
        source: str,
        corrections: list,
        hallucination_warning: Optional[str] = None,
    ) -> dict:
        """Build standardized response dict."""
        response = {
            "original": original,
            "corrected": corrected,
            "changed": original != corrected,
            "confidence": confidence,
            "source": source,
            "corrections_applied": len(corrections),
            "corrections": corrections,
        }
        if hallucination_warning:
            response["hallucination_warning"] = hallucination_warning
        return response

    async def get_correction_breakdown(
        self, text: str, audio_duration_seconds: Optional[float] = None
    ) -> dict:
        """Get detailed breakdown of what each layer would do.

        Useful for debugging and understanding the correction pipeline.
        """
        results = {
            "input": text,
            "layers": [],
        }

        # Test each layer independently
        # Hallucination detection
        hallucination_result = self.hallucination_detector.detect(
            text, audio_duration_seconds
        )
        would_auto_remove = (
            hallucination_result["is_hallucination"]
            and hallucination_result["confidence"] >= self.HALLUCINATION_CONFIDENCE_THRESHOLD
        )
        results["layers"].append({
            "name": "hallucination_detection",
            "would_change": would_auto_remove,
            "output": hallucination_result["corrected"] if would_auto_remove else text,
            "confidence": hallucination_result["confidence"],
            "is_hallucination": hallucination_result["is_hallucination"],
            "details": hallucination_result["detections"],
            "note": (
                "Would auto-remove" if would_auto_remove
                else "Flagged but not auto-removed" if hallucination_result["detections"]
                else "No hallucination detected"
            ),
        })

        # Rule-based
        rule_result = await self.rule_corrector.correct(text)
        results["layers"].append({
            "name": "rule_based",
            "would_change": rule_result["changed"],
            "output": rule_result["corrected"],
            "confidence": rule_result["confidence"],
            "details": rule_result["corrections"],
        })

        # ML model
        if await self.ml_corrector.has_trained_model():
            ml_result = await self.ml_corrector.correct(text)
            results["layers"].append({
                "name": "ml_model",
                "would_change": ml_result is not None,
                "output": ml_result["corrected"] if ml_result else text,
                "confidence": ml_result["confidence"] if ml_result else 0.0,
                "details": ml_result if ml_result else None,
            })
        else:
            results["layers"].append({
                "name": "ml_model",
                "would_change": False,
                "output": text,
                "confidence": 0.0,
                "details": {"status": "no_trained_model"},
            })

        # Embedding retrieval
        similar = await self.retriever.retrieve_relevant_corrections(
            text, top_k=5, threshold=0.5
        )
        results["layers"].append({
            "name": "embedding_retrieval",
            "would_change": len(similar) > 0 and similar[0]["similarity"] >= 0.85,
            "output": similar[0]["corrected_text"] if similar else text,
            "confidence": similar[0]["similarity"] if similar else 0.0,
            "details": similar[:3] if similar else [],
        })

        return results
