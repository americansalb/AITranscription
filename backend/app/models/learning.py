"""Models for the ML learning system."""
from datetime import datetime
from typing import TYPE_CHECKING

from pgvector.sqlalchemy import Vector
from sqlalchemy import (
    Boolean,
    Date,
    DateTime,
    Float,
    ForeignKey,
    Integer,
    String,
    Text,
    UniqueConstraint,
    func,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.models.base import Base

if TYPE_CHECKING:
    from app.models.user import User


class CorrectionEmbedding(Base):
    """Stores correction pairs with vector embeddings for retrieval learning."""

    __tablename__ = "correction_embeddings"

    id: Mapped[int] = mapped_column(primary_key=True)
    user_id: Mapped[int] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"), index=True
    )

    # The original (incorrect) text
    original_text: Mapped[str] = mapped_column(Text, nullable=False)

    # The corrected text
    corrected_text: Mapped[str] = mapped_column(Text, nullable=False)

    # Vector embedding for similarity search (384 dims for all-MiniLM-L6-v2)
    embedding: Mapped[list[float]] = mapped_column(Vector(384), nullable=True)

    # Classification of correction type
    correction_type: Mapped[str | None] = mapped_column(
        String(50), nullable=True
    )  # 'spelling', 'grammar', 'punctuation', 'vocabulary', 'filler'

    # How many times this exact correction has been made
    correction_count: Mapped[int] = mapped_column(Integer, default=1)

    # Link to audio sample if available
    audio_sample_id: Mapped[int | None] = mapped_column(
        ForeignKey("audio_samples.id", ondelete="SET NULL"),
        nullable=True,
    )

    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
    )

    # Relationships
    user: Mapped["User"] = relationship("User")
    audio_sample: Mapped["AudioSample | None"] = relationship(
        "AudioSample", back_populates="corrections"
    )


class AudioSample(Base):
    """Stores audio samples for Whisper fine-tuning."""

    __tablename__ = "audio_samples"

    id: Mapped[int] = mapped_column(primary_key=True)
    user_id: Mapped[int] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"), index=True
    )

    # Path to audio file (S3 URL or local path)
    audio_path: Mapped[str] = mapped_column(String(500), nullable=False)

    # Audio metadata
    duration_seconds: Mapped[float | None] = mapped_column(Float, nullable=True)

    # Transcriptions
    raw_transcription: Mapped[str | None] = mapped_column(Text, nullable=True)
    corrected_transcription: Mapped[str | None] = mapped_column(Text, nullable=True)

    # Word error rate before correction (0.0 to 1.0)
    error_rate: Mapped[float | None] = mapped_column(Float, nullable=True)

    # Whether this sample has been used for training
    used_for_training: Mapped[bool] = mapped_column(Boolean, default=False)

    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
    )

    # Relationships
    user: Mapped["User"] = relationship("User")
    corrections: Mapped[list["CorrectionEmbedding"]] = relationship(
        "CorrectionEmbedding", back_populates="audio_sample"
    )


class LearningMetrics(Base):
    """Daily learning metrics for dashboard display."""

    __tablename__ = "learning_metrics"
    __table_args__ = (
        UniqueConstraint("user_id", "date", name="uq_metrics_user_date"),
    )

    id: Mapped[int] = mapped_column(primary_key=True)
    user_id: Mapped[int] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"), index=True
    )
    date: Mapped[datetime] = mapped_column(Date, nullable=False)

    # Daily counts
    transcriptions_count: Mapped[int] = mapped_column(Integer, default=0)
    corrections_count: Mapped[int] = mapped_column(Integer, default=0)
    auto_accepted: Mapped[int] = mapped_column(Integer, default=0)

    # Average confidence score for the day
    avg_confidence: Mapped[float | None] = mapped_column(Float, nullable=True)

    # Model version used
    model_version: Mapped[str | None] = mapped_column(String(50), nullable=True)

    # Relationships
    user: Mapped["User"] = relationship("User")


class ModelVersion(Base):
    """Tracks trained model versions for each user."""

    __tablename__ = "model_versions"

    id: Mapped[int] = mapped_column(primary_key=True)
    user_id: Mapped[int] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"), index=True
    )

    # Type of model
    model_type: Mapped[str] = mapped_column(
        String(50), nullable=False
    )  # 'correction_nn', 'whisper_lora'

    # Version number (increments with each training)
    version: Mapped[int] = mapped_column(Integer, nullable=False)

    # Path to saved model file
    model_path: Mapped[str | None] = mapped_column(String(500), nullable=True)

    # Training metadata
    training_samples: Mapped[int | None] = mapped_column(Integer, nullable=True)
    training_loss: Mapped[float | None] = mapped_column(Float, nullable=True)
    validation_wer: Mapped[float | None] = mapped_column(
        Float, nullable=True
    )  # Word error rate

    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
    )

    # Relationships
    user: Mapped["User"] = relationship("User")


class CorrectionRule(Base):
    """User-defined correction rules for the rule-based layer."""

    __tablename__ = "correction_rules"

    id: Mapped[int] = mapped_column(primary_key=True)
    user_id: Mapped[int] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"), index=True
    )

    # Pattern to match (plain text or regex)
    pattern: Mapped[str] = mapped_column(String(500), nullable=False)

    # Replacement text
    replacement: Mapped[str] = mapped_column(String(500), nullable=False)

    # Whether pattern is a regex
    is_regex: Mapped[bool] = mapped_column(Boolean, default=False)

    # Priority for ordering (higher = applied first)
    priority: Mapped[int] = mapped_column(Integer, default=0)

    # How many times this rule has been applied
    hit_count: Mapped[int] = mapped_column(Integer, default=0)

    # Whether the rule is active
    is_active: Mapped[bool] = mapped_column(Boolean, default=True)

    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
    )

    # Relationships
    user: Mapped["User"] = relationship("User")
