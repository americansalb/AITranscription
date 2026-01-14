"""Audio collector service for Whisper fine-tuning data."""
import hashlib
import logging
import os
from datetime import datetime
from pathlib import Path
from typing import Optional

from sqlalchemy import select, func
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.learning import AudioSample

logger = logging.getLogger(__name__)

# Default storage configuration
AUDIO_STORAGE_DIR = os.environ.get("AUDIO_STORAGE_DIR", "./audio_samples")
MIN_ERROR_RATE_FOR_TRAINING = 0.05  # Only store samples with >5% error


def calculate_word_error_rate(original: str, corrected: str) -> float:
    """Calculate word error rate between original and corrected text.

    WER = (Substitutions + Insertions + Deletions) / Words in Reference
    Uses a simplified approach for speed.
    """
    original_words = original.lower().split()
    corrected_words = corrected.lower().split()

    if not corrected_words:
        return 1.0 if original_words else 0.0

    # Count differences (simplified - not true Levenshtein)
    original_set = set(original_words)
    corrected_set = set(corrected_words)

    # Words that are different
    different = len(original_set.symmetric_difference(corrected_set))

    # Normalize by reference length
    return min(different / max(len(corrected_words), 1), 1.0)


class AudioCollector:
    """Service for collecting and managing audio samples for training."""

    def __init__(self, db: AsyncSession, user_id: int):
        self.db = db
        self.user_id = user_id
        self.storage_dir = Path(AUDIO_STORAGE_DIR) / str(user_id)
        self.storage_dir.mkdir(parents=True, exist_ok=True)

    async def store_audio_sample(
        self,
        audio_data: bytes,
        raw_transcription: str,
        corrected_transcription: str,
        duration_seconds: Optional[float] = None,
    ) -> Optional[AudioSample]:
        """Store an audio sample for future training.

        Only stores samples with meaningful corrections (error rate > threshold).
        """
        # Calculate error rate
        error_rate = calculate_word_error_rate(raw_transcription, corrected_transcription)

        # Skip if error rate is too low (good transcription, not useful for training)
        if error_rate < MIN_ERROR_RATE_FOR_TRAINING:
            logger.debug(
                f"Skipping audio sample with low error rate: {error_rate:.2%}"
            )
            return None

        # Generate unique filename based on content hash
        content_hash = hashlib.sha256(audio_data).hexdigest()[:16]
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        filename = f"audio_{timestamp}_{content_hash}.wav"
        audio_path = self.storage_dir / filename

        # Save audio file
        try:
            with open(audio_path, "wb") as f:
                f.write(audio_data)
        except Exception as e:
            logger.error(f"Failed to save audio file: {e}")
            return None

        # Create database record
        sample = AudioSample(
            user_id=self.user_id,
            audio_path=str(audio_path),
            duration_seconds=duration_seconds,
            raw_transcription=raw_transcription,
            corrected_transcription=corrected_transcription,
            error_rate=error_rate,
            used_for_training=False,
        )
        self.db.add(sample)
        await self.db.commit()
        await self.db.refresh(sample)

        logger.info(
            f"Stored audio sample for user {self.user_id}: "
            f"{filename} (error_rate={error_rate:.2%})"
        )
        return sample

    async def get_training_samples(
        self,
        min_samples: int = 50,
        unused_only: bool = True,
    ) -> Optional[list[AudioSample]]:
        """Get audio samples available for training.

        Returns None if not enough samples are available.
        """
        query = select(AudioSample).where(AudioSample.user_id == self.user_id)

        if unused_only:
            query = query.where(AudioSample.used_for_training == False)

        query = query.order_by(AudioSample.created_at.desc())
        result = await self.db.execute(query)
        samples = list(result.scalars())

        if len(samples) < min_samples:
            return None

        return samples

    async def mark_samples_as_used(self, sample_ids: list[int]) -> int:
        """Mark samples as used for training."""
        from sqlalchemy import update

        result = await self.db.execute(
            update(AudioSample)
            .where(
                AudioSample.id.in_(sample_ids),
                AudioSample.user_id == self.user_id,
            )
            .values(used_for_training=True)
        )
        await self.db.commit()
        return result.rowcount

    async def get_sample_stats(self) -> dict:
        """Get statistics about collected audio samples."""
        result = await self.db.execute(
            select(
                func.count(AudioSample.id).label("total"),
                func.sum(AudioSample.duration_seconds).label("total_duration"),
                func.avg(AudioSample.error_rate).label("avg_error_rate"),
                func.count(AudioSample.id)
                .filter(AudioSample.used_for_training == True)
                .label("used_for_training"),
            ).where(AudioSample.user_id == self.user_id)
        )
        row = result.fetchone()

        return {
            "total_samples": row.total or 0,
            "total_duration_seconds": float(row.total_duration or 0),
            "avg_error_rate": float(row.avg_error_rate or 0),
            "samples_used_for_training": row.used_for_training or 0,
            "samples_available_for_training": (row.total or 0)
            - (row.used_for_training or 0),
            "ready_for_whisper_training": (row.total or 0) >= 50,
        }

    async def delete_sample(self, sample_id: int) -> bool:
        """Delete an audio sample."""
        result = await self.db.execute(
            select(AudioSample).where(
                AudioSample.id == sample_id,
                AudioSample.user_id == self.user_id,
            )
        )
        sample = result.scalar_one_or_none()

        if sample:
            # Delete file if it exists
            try:
                Path(sample.audio_path).unlink(missing_ok=True)
            except Exception as e:
                logger.warning(f"Failed to delete audio file: {e}")

            await self.db.delete(sample)
            await self.db.commit()
            return True
        return False

    async def get_recent_samples(self, limit: int = 10) -> list[dict]:
        """Get recent audio samples for display."""
        result = await self.db.execute(
            select(AudioSample)
            .where(AudioSample.user_id == self.user_id)
            .order_by(AudioSample.created_at.desc())
            .limit(limit)
        )

        return [
            {
                "id": s.id,
                "duration_seconds": s.duration_seconds,
                "error_rate": s.error_rate,
                "used_for_training": s.used_for_training,
                "created_at": s.created_at.isoformat() if s.created_at else None,
            }
            for s in result.scalars()
        ]
