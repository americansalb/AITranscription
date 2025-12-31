from datetime import datetime

from sqlalchemy import DateTime, ForeignKey, String, Text, func
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.models.base import Base


class DictionaryEntry(Base):
    """Custom dictionary entry for user-specific vocabulary."""

    __tablename__ = "dictionary_entries"

    id: Mapped[int] = mapped_column(primary_key=True)
    user_id: Mapped[int] = mapped_column(ForeignKey("users.id", ondelete="CASCADE"), index=True)

    # The word/phrase exactly as it should appear
    word: Mapped[str] = mapped_column(String(255), index=True)

    # Optional pronunciation hint (how it might be spoken/transcribed)
    pronunciation: Mapped[str | None] = mapped_column(String(255), nullable=True)

    # Optional description/context for the word
    description: Mapped[str | None] = mapped_column(Text, nullable=True)

    # Category for organization (e.g., "names", "technical", "medical")
    category: Mapped[str | None] = mapped_column(String(100), nullable=True)

    # Timestamps
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
    )
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
        onupdate=func.now(),
    )

    # Relationship
    user: Mapped["User"] = relationship("User", back_populates="dictionary_entries")


# Add relationship to User model
from app.models.user import User

User.dictionary_entries = relationship(
    "DictionaryEntry",
    back_populates="user",
    cascade="all, delete-orphan",
)
