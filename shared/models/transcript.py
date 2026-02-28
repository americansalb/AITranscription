from datetime import datetime
from typing import TYPE_CHECKING

from sqlalchemy import DateTime, Float, ForeignKey, Integer, String, Text, func
from sqlalchemy.orm import Mapped, mapped_column, relationship

from shared.models.base import Base

if TYPE_CHECKING:
    from shared.models.user import User


class Transcript(Base):
    """Model for storing all transcriptions with statistics."""

    __tablename__ = "transcripts"

    id: Mapped[int] = mapped_column(primary_key=True)
    user_id: Mapped[int | None] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"),
        nullable=True,
        index=True,
    )

    # Transcription content
    raw_text: Mapped[str] = mapped_column(Text)
    polished_text: Mapped[str] = mapped_column(Text)

    # Audio metadata
    audio_duration_seconds: Mapped[float] = mapped_column(Float, default=0.0)
    language: Mapped[str | None] = mapped_column(String(10), nullable=True)

    # Statistics
    word_count: Mapped[int] = mapped_column(Integer, default=0)
    character_count: Mapped[int] = mapped_column(Integer, default=0)
    words_per_minute: Mapped[float] = mapped_column(Float, default=0.0)

    # Context used
    context: Mapped[str | None] = mapped_column(String(50), nullable=True)
    formality: Mapped[str | None] = mapped_column(String(20), nullable=True)

    # Transcript type: 'input' (user recording) or 'output' (Claude speaking)
    transcript_type: Mapped[str] = mapped_column(String(20), default="input")

    # Session identifier
    session_id: Mapped[str | None] = mapped_column(String(100), nullable=True, index=True)

    # Timestamps
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
        index=True,
    )

    # Relationships
    user: Mapped["User | None"] = relationship("User", back_populates="transcripts")
