from datetime import datetime
from enum import Enum
from typing import TYPE_CHECKING

from sqlalchemy import Boolean, DateTime, Enum as SQLEnum, String, func
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.models.base import Base

if TYPE_CHECKING:
    from app.models.dictionary import DictionaryEntry
    from app.models.transcript import Transcript
    from app.models.gamification import UserGamification


class SubscriptionTier(str, Enum):
    """User subscription tiers as defined in the product vision."""

    DEVELOPER = "developer"  # Developer/testing tier (free, unlimited)
    ACCESS = "access"  # At-cost tier for verified disabled users (~$2.50/mo)
    STANDARD = "standard"  # General public ($5/mo)
    ENTERPRISE = "enterprise"  # API/Enterprise tier (custom pricing)


class User(Base):
    """User model for authentication and subscription tracking."""

    __tablename__ = "users"

    id: Mapped[int] = mapped_column(primary_key=True)
    email: Mapped[str] = mapped_column(String(255), unique=True, index=True)
    hashed_password: Mapped[str] = mapped_column(String(255))
    full_name: Mapped[str | None] = mapped_column(String(255), nullable=True)

    # Subscription
    tier: Mapped[SubscriptionTier] = mapped_column(
        SQLEnum(SubscriptionTier),
        default=SubscriptionTier.STANDARD,
    )
    is_active: Mapped[bool] = mapped_column(Boolean, default=True)

    # Accessibility verification
    accessibility_verified: Mapped[bool] = mapped_column(Boolean, default=False)
    accessibility_verified_at: Mapped[datetime | None] = mapped_column(
        DateTime(timezone=True), nullable=True
    )

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

    # Usage tracking
    total_audio_seconds: Mapped[int] = mapped_column(default=0)
    total_polish_tokens: Mapped[int] = mapped_column(default=0)
    total_transcriptions: Mapped[int] = mapped_column(default=0)
    total_words: Mapped[int] = mapped_column(default=0)

    # User settings
    is_admin: Mapped[bool] = mapped_column(Boolean, default=False)
    typing_wpm: Mapped[int] = mapped_column(default=40)

    # Relationships
    dictionary_entries: Mapped[list["DictionaryEntry"]] = relationship(
        "DictionaryEntry",
        back_populates="user",
        cascade="all, delete-orphan",
    )
    transcripts: Mapped[list["Transcript"]] = relationship(
        "Transcript",
        back_populates="user",
        cascade="all, delete-orphan",
    )
    gamification: Mapped["UserGamification | None"] = relationship(
        "UserGamification",
        back_populates="user",
        uselist=False,
        cascade="all, delete-orphan",
    )
