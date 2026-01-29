"""Gamification models for XP, levels, tiers, and achievements."""

from datetime import datetime
from enum import Enum
from typing import TYPE_CHECKING

from sqlalchemy import (
    Boolean,
    DateTime,
    Enum as SQLEnum,
    Float,
    ForeignKey,
    Integer,
    String,
    Text,
    func,
    Index,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.models.base import Base

if TYPE_CHECKING:
    from app.models.user import User


class PrestigeTier(str, Enum):
    """Prestige tiers for the leveling system."""
    BRONZE = "bronze"
    SILVER = "silver"
    GOLD = "gold"
    PLATINUM = "platinum"
    DIAMOND = "diamond"
    MASTER = "master"
    LEGEND = "legend"


class AchievementRarity(str, Enum):
    """Achievement rarity levels."""
    COMMON = "common"
    RARE = "rare"
    EPIC = "epic"
    LEGENDARY = "legendary"


class AchievementCategory(str, Enum):
    """Achievement categories."""
    VOLUME = "volume"
    STREAK = "streak"
    SPEED = "speed"
    CONTEXT = "context"
    FORMALITY = "formality"
    LEARNING = "learning"
    TEMPORAL = "temporal"
    RECORDS = "records"
    COMBO = "combo"
    SPECIAL = "special"


class AchievementDefinition(Base):
    """Static achievement definitions - seeded once, shared by all users."""

    __tablename__ = "achievement_definitions"

    id: Mapped[str] = mapped_column(String(100), primary_key=True)  # e.g., "vol_words_5"
    name: Mapped[str] = mapped_column(String(255))  # e.g., "Word Warrior V"
    description: Mapped[str] = mapped_column(Text)  # e.g., "Transcribe 2,500 words"
    # Use String to avoid PostgreSQL enum mapping issues - values validated at app layer
    category: Mapped[str] = mapped_column(String(50))
    rarity: Mapped[str] = mapped_column(String(50))
    xp_reward: Mapped[int] = mapped_column(Integer)  # XP awarded on unlock
    icon: Mapped[str] = mapped_column(String(50))  # Emoji or icon identifier
    tier: Mapped[int] = mapped_column(Integer)  # Tier within the achievement line (1-20)
    threshold: Mapped[float] = mapped_column(Float)  # Value needed to unlock
    metric_type: Mapped[str] = mapped_column(String(100))  # e.g., "total_words", "current_streak"
    is_hidden: Mapped[bool] = mapped_column(Boolean, default=False)  # Hidden achievements

    # Optional: parent achievement for tiered progressions
    parent_id: Mapped[str | None] = mapped_column(String(100), nullable=True)

    # Relationships
    user_achievements: Mapped[list["UserAchievement"]] = relationship(
        "UserAchievement",
        back_populates="achievement",
        cascade="all, delete-orphan",
    )

    __table_args__ = (
        Index("ix_achievement_category", "category"),
        Index("ix_achievement_rarity", "rarity"),
        Index("ix_achievement_metric", "metric_type"),
    )


class UserGamification(Base):
    """User's gamification progress - XP, level, tier."""

    __tablename__ = "user_gamification"

    id: Mapped[int] = mapped_column(primary_key=True)
    user_id: Mapped[int] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"),
        unique=True,
        index=True,
    )

    # Current progress
    current_xp: Mapped[int] = mapped_column(Integer, default=0)  # XP in current level
    current_level: Mapped[int] = mapped_column(Integer, default=1)  # 1-100
    # Use String to avoid PostgreSQL enum mapping issues
    prestige_tier: Mapped[str] = mapped_column(String(50), default="bronze")

    # Lifetime stats
    lifetime_xp: Mapped[int] = mapped_column(Integer, default=0)  # Total XP ever earned
    achievements_unlocked: Mapped[int] = mapped_column(Integer, default=0)

    # Multipliers and bonuses
    xp_multiplier: Mapped[float] = mapped_column(Float, default=1.0)
    streak_bonus_active: Mapped[bool] = mapped_column(Boolean, default=False)

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
    last_xp_earned_at: Mapped[datetime | None] = mapped_column(
        DateTime(timezone=True),
        nullable=True,
    )

    # Relationship
    user: Mapped["User"] = relationship("User", back_populates="gamification")
    xp_transactions: Mapped[list["XPTransaction"]] = relationship(
        "XPTransaction",
        back_populates="user_gamification",
        cascade="all, delete-orphan",
    )


class UserAchievement(Base):
    """Junction table tracking which achievements a user has earned."""

    __tablename__ = "user_achievements"

    id: Mapped[int] = mapped_column(primary_key=True)
    user_id: Mapped[int] = mapped_column(
        ForeignKey("users.id", ondelete="CASCADE"),
        index=True,
    )
    achievement_id: Mapped[str] = mapped_column(
        ForeignKey("achievement_definitions.id", ondelete="CASCADE"),
        index=True,
    )

    # Progress tracking
    current_value: Mapped[float] = mapped_column(Float, default=0.0)  # Current progress value
    is_unlocked: Mapped[bool] = mapped_column(Boolean, default=False)
    unlocked_at: Mapped[datetime | None] = mapped_column(
        DateTime(timezone=True),
        nullable=True,
    )

    # For display purposes
    notified: Mapped[bool] = mapped_column(Boolean, default=False)  # Has user been notified?

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

    # Relationships
    achievement: Mapped["AchievementDefinition"] = relationship(
        "AchievementDefinition",
        back_populates="user_achievements",
    )

    __table_args__ = (
        Index("ix_user_achievement_unique", "user_id", "achievement_id", unique=True),
        Index("ix_user_achievement_unlocked", "user_id", "is_unlocked"),
    )


class XPTransaction(Base):
    """Log of all XP transactions for audit and history."""

    __tablename__ = "xp_transactions"

    id: Mapped[int] = mapped_column(primary_key=True)
    user_gamification_id: Mapped[int] = mapped_column(
        ForeignKey("user_gamification.id", ondelete="CASCADE"),
        index=True,
    )

    # Transaction details
    amount: Mapped[int] = mapped_column(Integer)  # Raw XP amount
    multiplier: Mapped[float] = mapped_column(Float, default=1.0)  # Applied multiplier
    final_amount: Mapped[int] = mapped_column(Integer)  # amount * multiplier

    # Source tracking
    source: Mapped[str] = mapped_column(String(100))  # e.g., "transcription", "achievement", "daily_login"
    source_id: Mapped[str | None] = mapped_column(String(100), nullable=True)  # Reference ID if applicable
    description: Mapped[str | None] = mapped_column(Text, nullable=True)

    # Level snapshot at time of transaction
    level_before: Mapped[int] = mapped_column(Integer)
    level_after: Mapped[int] = mapped_column(Integer)

    # Timestamp
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
        index=True,
    )

    # Relationship
    user_gamification: Mapped["UserGamification"] = relationship(
        "UserGamification",
        back_populates="xp_transactions",
    )

    __table_args__ = (
        Index("ix_xp_transaction_source", "source"),
        Index("ix_xp_transaction_user_date", "user_gamification_id", "created_at"),
    )
