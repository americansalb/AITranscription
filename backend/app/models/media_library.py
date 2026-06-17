"""Database models for the bulk Transcription Studio.

A ``MediaItem`` is one uploaded audio/video file and its transcription job. Its
transcript is stored both as one full-text blob (``transcript``) for display/download
and as ordered, timestamped ``TranscriptSegment`` rows for keyword search and for
building citation context when answering questions with Claude.

Status is a plain string column (not a SQL enum) on purpose — migration 004 in this
project specifically moved away from enum columns because of cross-database friction.
"""

from datetime import datetime

from sqlalchemy import (
    DateTime,
    Float,
    ForeignKey,
    Integer,
    String,
    Text,
    func,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.models.base import Base

# Job lifecycle states.
STATUS_QUEUED = "queued"
STATUS_PROCESSING = "processing"
STATUS_COMPLETED = "completed"
STATUS_FAILED = "failed"


class MediaItem(Base):
    """One uploaded media file and its transcription job."""

    __tablename__ = "studio_media_items"

    id: Mapped[int] = mapped_column(primary_key=True)

    filename: Mapped[str] = mapped_column(String(512), nullable=False)
    content_type: Mapped[str | None] = mapped_column(String(128), nullable=True)
    size_bytes: Mapped[int | None] = mapped_column(Integer, nullable=True)

    status: Mapped[str] = mapped_column(
        String(20), nullable=False, default=STATUS_QUEUED, index=True
    )
    error: Mapped[str | None] = mapped_column(Text, nullable=True)

    # Filled in once transcription completes.
    language: Mapped[str | None] = mapped_column(String(32), nullable=True)
    duration_seconds: Mapped[float | None] = mapped_column(Float, nullable=True)
    transcript: Mapped[str | None] = mapped_column(Text, nullable=True)
    word_count: Mapped[int] = mapped_column(Integer, nullable=False, default=0)

    # Transient on-disk path to the staged upload while it is being processed.
    # Cleared (and the file removed) once the job reaches a terminal state.
    source_path: Mapped[str | None] = mapped_column(String(1024), nullable=True)

    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
        onupdate=func.now(),
        nullable=False,
    )
    completed_at: Mapped[datetime | None] = mapped_column(
        DateTime(timezone=True), nullable=True
    )

    segments: Mapped[list["TranscriptSegment"]] = relationship(
        back_populates="media_item",
        cascade="all, delete-orphan",
        passive_deletes=True,
    )


class TranscriptSegment(Base):
    """An ordered, timestamped slice of a transcript — the unit of search & citation."""

    __tablename__ = "studio_transcript_segments"

    id: Mapped[int] = mapped_column(primary_key=True)
    media_id: Mapped[int] = mapped_column(
        ForeignKey("studio_media_items.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
    )

    idx: Mapped[int] = mapped_column(Integer, nullable=False)
    text: Mapped[str] = mapped_column(Text, nullable=False)
    start_seconds: Mapped[float | None] = mapped_column(Float, nullable=True)
    end_seconds: Mapped[float | None] = mapped_column(Float, nullable=True)

    media_item: Mapped["MediaItem"] = relationship(back_populates="segments")
