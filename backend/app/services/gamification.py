"""Gamification service - XP, levels, achievements, and progression logic."""

import math
from datetime import datetime, timedelta, timezone
from typing import Any

from sqlalchemy import select, func, and_, or_, exists
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.models.gamification import (
    AchievementDefinition,
    UserGamification,
    UserAchievement,
    XPTransaction,
    PrestigeTier,
    AchievementRarity,
    AchievementCategory,
)
from app.models.user import User
from app.models.transcript import Transcript
from app.models.learning import CorrectionEmbedding, AudioSample, CorrectionRule
from app.models.dictionary import DictionaryEntry


# =============================================================================
# CONSTANTS
# =============================================================================

# Tier thresholds (cumulative XP)
TIER_THRESHOLDS = {
    PrestigeTier.BRONZE: 0,
    PrestigeTier.SILVER: 2_750_000,
    PrestigeTier.GOLD: 5_500_000,
    PrestigeTier.PLATINUM: 8_250_000,
    PrestigeTier.DIAMOND: 11_000_000,
    PrestigeTier.MASTER: 13_750_000,
    PrestigeTier.LEGEND: 16_500_000,
}

TIER_COLORS = {
    PrestigeTier.BRONZE: "#CD7F32",
    PrestigeTier.SILVER: "#C0C0C0",
    PrestigeTier.GOLD: "#FFD700",
    PrestigeTier.PLATINUM: "#E5E4E2",
    PrestigeTier.DIAMOND: "#B9F2FF",
    PrestigeTier.MASTER: "#9B30FF",
    PrestigeTier.LEGEND: "rainbow",
}

# XP Sources
XP_SOURCES = {
    "transcription": 10,
    "words_per_10": 1,
    "daily_login": 25,
    "streak_bonus_per_day": 5,  # max 150 (30 days)
    "ai_correction": 15,
    "audio_sample": 25,
}

MAX_STREAK_BONUS = 150


# =============================================================================
# LEVEL CALCULATIONS
# =============================================================================

def xp_for_level(level: int) -> int:
    """Calculate XP required to reach a given level."""
    return int(100 * level * (1 + level / 10))


def xp_for_level_range(start_level: int, end_level: int) -> int:
    """Calculate total XP needed to go from start_level to end_level."""
    return sum(xp_for_level(l) for l in range(start_level, end_level))


def total_xp_for_level(level: int) -> int:
    """Calculate total XP needed to reach a level from level 1."""
    return sum(xp_for_level(l) for l in range(1, level))


def level_from_xp(total_xp: int) -> tuple[int, int]:
    """
    Calculate level and XP progress from total XP.
    Returns (current_level, xp_into_current_level)
    """
    level = 1
    remaining_xp = total_xp

    while True:
        xp_needed = xp_for_level(level)
        if remaining_xp < xp_needed:
            return level, remaining_xp
        remaining_xp -= xp_needed
        level += 1
        if level > 100:
            return 100, remaining_xp


def get_tier_from_lifetime_xp(lifetime_xp: int) -> PrestigeTier:
    """Determine prestige tier from lifetime XP."""
    for tier in reversed(list(PrestigeTier)):
        if lifetime_xp >= TIER_THRESHOLDS[tier]:
            return tier
    return PrestigeTier.BRONZE


def get_tier_progress(lifetime_xp: int) -> dict[str, Any]:
    """Get progress within current tier."""
    current_tier = get_tier_from_lifetime_xp(lifetime_xp)
    tier_list = list(PrestigeTier)
    tier_index = tier_list.index(current_tier)

    tier_start = TIER_THRESHOLDS[current_tier]

    if tier_index < len(tier_list) - 1:
        next_tier = tier_list[tier_index + 1]
        tier_end = TIER_THRESHOLDS[next_tier]
        progress = (lifetime_xp - tier_start) / (tier_end - tier_start)
    else:
        # Legend tier - no cap
        next_tier = None
        tier_end = None
        progress = 1.0

    return {
        "current_tier": current_tier.value,  # Convert enum to string
        "next_tier": next_tier.value if next_tier else None,  # Convert enum to string
        "tier_start_xp": tier_start,
        "tier_end_xp": tier_end,
        "xp_in_tier": lifetime_xp - tier_start,
        "progress": min(progress, 1.0),
        "color": TIER_COLORS[current_tier],
    }


# =============================================================================
# GAMIFICATION SERVICE
# =============================================================================

class GamificationService:
    """Service for managing user gamification data."""

    def __init__(self, db: AsyncSession):
        self.db = db

    async def get_or_create_user_gamification(self, user_id: int) -> UserGamification:
        """Get or create gamification record for a user, with retroactive XP for existing transcripts."""
        from app.models.transcript import Transcript

        result = await self.db.execute(
            select(UserGamification).where(UserGamification.user_id == user_id)
        )
        gamification = result.scalar_one_or_none()

        if not gamification:
            # Calculate retroactive XP from existing transcripts
            result = await self.db.execute(
                select(
                    func.count(Transcript.id),
                    func.coalesce(func.sum(Transcript.word_count), 0)
                ).where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
            )
            row = result.one()
            total_transcriptions = int(row[0] or 0)
            total_words = int(row[1] or 0)

            # XP formula: 10 XP per transcription + 1 XP per 10 words
            retroactive_xp = (total_transcriptions * 10) + (total_words // 10)

            print(f"[GAMIFICATION] Creating new record for user {user_id}")
            print(f"[GAMIFICATION] Transcripts: {total_transcriptions}, Words: {total_words}")
            print(f"[GAMIFICATION] Retroactive XP: {retroactive_xp}")

            # Calculate level and tier from retroactive XP
            tier = get_tier_from_lifetime_xp(retroactive_xp)
            tier_start = TIER_THRESHOLDS[tier]
            xp_in_tier = retroactive_xp - tier_start
            level, xp_into_level = level_from_xp(xp_in_tier)

            print(f"[GAMIFICATION] Level: {level}, XP in level: {xp_into_level}, Tier: {tier.value}")

            gamification = UserGamification(
                user_id=user_id,
                lifetime_xp=retroactive_xp,
                current_xp=xp_into_level,
                current_level=level,
                prestige_tier=tier.value,
            )
            self.db.add(gamification)
            await self.db.flush()

        return gamification

    async def award_xp(
        self,
        user_id: int,
        amount: int,
        source: str,
        source_id: str | None = None,
        description: str | None = None,
    ) -> dict[str, Any]:
        """
        Award XP to a user and handle level ups.
        Returns info about XP gained and any level changes.
        """
        gamification = await self.get_or_create_user_gamification(user_id)

        level_before = gamification.current_level
        multiplier = gamification.xp_multiplier
        final_amount = int(amount * multiplier)

        # Update XP
        gamification.lifetime_xp += final_amount
        gamification.last_xp_earned_at = datetime.now(timezone.utc)

        # Calculate new level from lifetime XP within current prestige tier
        # Convert string to enum for threshold lookup
        current_tier_enum = PrestigeTier(gamification.prestige_tier)
        tier_start_xp = TIER_THRESHOLDS[current_tier_enum]
        xp_in_tier = gamification.lifetime_xp - tier_start_xp

        new_level, xp_into_level = level_from_xp(xp_in_tier)
        gamification.current_level = new_level
        gamification.current_xp = xp_into_level

        # Check for tier promotion
        new_tier = get_tier_from_lifetime_xp(gamification.lifetime_xp)
        tier_changed = new_tier.value != gamification.prestige_tier
        if tier_changed:
            gamification.prestige_tier = new_tier.value  # Store as string
            # Reset level for new tier
            new_tier_start = TIER_THRESHOLDS[new_tier]
            xp_in_new_tier = gamification.lifetime_xp - new_tier_start
            gamification.current_level, gamification.current_xp = level_from_xp(xp_in_new_tier)

        level_after = gamification.current_level

        # Log transaction
        transaction = XPTransaction(
            user_gamification_id=gamification.id,
            amount=amount,
            multiplier=multiplier,
            final_amount=final_amount,
            source=source,
            source_id=source_id,
            description=description,
            level_before=level_before,
            level_after=level_after,
        )
        self.db.add(transaction)

        await self.db.flush()

        return {
            "xp_gained": final_amount,
            "multiplier": multiplier,
            "level_before": level_before,
            "level_after": level_after,
            "leveled_up": level_after > level_before,
            "levels_gained": level_after - level_before,
            "tier_changed": tier_changed,
            "new_tier": new_tier if tier_changed else None,
            "current_xp": gamification.current_xp,
            "xp_to_next_level": xp_for_level(level_after),
            "lifetime_xp": gamification.lifetime_xp,
        }

    async def award_transcription_xp(
        self,
        user_id: int,
        word_count: int,
        transcript_id: int | None = None,
    ) -> dict[str, Any]:
        """Award XP for completing a transcription."""
        base_xp = XP_SOURCES["transcription"]
        word_xp = (word_count // 10) * XP_SOURCES["words_per_10"]
        total_xp = base_xp + word_xp

        return await self.award_xp(
            user_id=user_id,
            amount=total_xp,
            source="transcription",
            source_id=str(transcript_id) if transcript_id else None,
            description=f"Transcription with {word_count} words",
        )

    async def award_daily_login_xp(self, user_id: int) -> dict[str, Any]:
        """Award XP for daily login (called once per day)."""
        return await self.award_xp(
            user_id=user_id,
            amount=XP_SOURCES["daily_login"],
            source="daily_login",
            description="Daily login bonus",
        )

    async def award_streak_bonus(self, user_id: int, streak_days: int) -> dict[str, Any]:
        """Award streak bonus XP."""
        bonus = min(streak_days * XP_SOURCES["streak_bonus_per_day"], MAX_STREAK_BONUS)
        return await self.award_xp(
            user_id=user_id,
            amount=bonus,
            source="streak_bonus",
            description=f"{streak_days}-day streak bonus",
        )

    async def award_achievement_xp(
        self,
        user_id: int,
        achievement_id: str,
        xp_amount: int,
    ) -> dict[str, Any]:
        """Award XP for unlocking an achievement."""
        return await self.award_xp(
            user_id=user_id,
            amount=xp_amount,
            source="achievement",
            source_id=achievement_id,
            description=f"Achievement unlocked: {achievement_id}",
        )

    async def get_user_progress(self, user_id: int) -> dict[str, Any]:
        """Get complete gamification progress for a user."""
        gamification = await self.get_or_create_user_gamification(user_id)

        # Get achievement counts
        result = await self.db.execute(
            select(
                func.count(UserAchievement.id).filter(UserAchievement.is_unlocked == True)
            ).where(UserAchievement.user_id == user_id)
        )
        unlocked_count = result.scalar() or 0

        # Get total achievements
        result = await self.db.execute(select(func.count(AchievementDefinition.id)))
        total_achievements = result.scalar() or 0

        # Get rarity breakdown
        result = await self.db.execute(
            select(
                AchievementDefinition.rarity,
                func.count(UserAchievement.id)
            )
            .join(UserAchievement, UserAchievement.achievement_id == AchievementDefinition.id)
            .where(
                and_(
                    UserAchievement.user_id == user_id,
                    UserAchievement.is_unlocked == True
                )
            )
            .group_by(AchievementDefinition.rarity)
        )
        rarity_counts = {row[0]: row[1] for row in result.all()}  # rarity is already a string

        tier_progress = get_tier_progress(gamification.lifetime_xp)

        return {
            "user_id": user_id,
            "current_level": gamification.current_level,
            "current_xp": gamification.current_xp,
            "xp_to_next_level": xp_for_level(gamification.current_level),
            "level_progress": gamification.current_xp / xp_for_level(gamification.current_level),
            "lifetime_xp": gamification.lifetime_xp,
            "prestige_tier": gamification.prestige_tier,  # Already a string
            "tier_color": tier_progress["color"],
            "tier_progress": tier_progress,
            "xp_multiplier": gamification.xp_multiplier,
            "achievements": {
                "unlocked": unlocked_count,
                "total": total_achievements,
                "progress": unlocked_count / total_achievements if total_achievements > 0 else 0,
                "by_rarity": {
                    "common": rarity_counts.get("common", 0),
                    "rare": rarity_counts.get("rare", 0),
                    "epic": rarity_counts.get("epic", 0),
                    "legendary": rarity_counts.get("legendary", 0),
                }
            },
            "last_xp_earned_at": gamification.last_xp_earned_at.isoformat() if gamification.last_xp_earned_at else None,
        }

    async def get_recent_xp_transactions(
        self,
        user_id: int,
        limit: int = 20,
    ) -> list[dict[str, Any]]:
        """Get recent XP transactions for a user."""
        gamification = await self.get_or_create_user_gamification(user_id)

        result = await self.db.execute(
            select(XPTransaction)
            .where(XPTransaction.user_gamification_id == gamification.id)
            .order_by(XPTransaction.created_at.desc())
            .limit(limit)
        )
        transactions = result.scalars().all()

        return [
            {
                "id": t.id,
                "amount": t.amount,
                "final_amount": t.final_amount,
                "multiplier": t.multiplier,
                "source": t.source,
                "source_id": t.source_id,
                "description": t.description,
                "level_before": t.level_before,
                "level_after": t.level_after,
                "created_at": t.created_at.isoformat(),
            }
            for t in transactions
        ]


# =============================================================================
# ACHIEVEMENT CHECKING SERVICE
# =============================================================================

class AchievementService:
    """Service for checking and unlocking achievements."""

    def __init__(self, db: AsyncSession):
        self.db = db
        self.gamification_service = GamificationService(db)

    async def get_user_metrics(self, user_id: int) -> dict[str, Any]:
        """Gather all metrics needed for achievement checking."""
        metrics = {}

        # Get user basic stats
        result = await self.db.execute(
            select(User).where(User.id == user_id)
        )
        user = result.scalar_one_or_none()
        if not user:
            return metrics

        metrics["total_words"] = user.total_words
        metrics["total_transcriptions"] = user.total_transcriptions
        metrics["total_audio_seconds"] = user.total_audio_seconds

        # Get transcript aggregates
        result = await self.db.execute(
            select(func.sum(Transcript.character_count))
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
        )
        metrics["total_characters"] = result.scalar() or 0

        # Get streak info
        metrics.update(await self._calculate_streaks(user_id))

        # Get speed metrics
        metrics.update(await self._calculate_speed_metrics(user_id))

        # Get context counts
        metrics.update(await self._calculate_context_metrics(user_id))

        # Get formality counts
        metrics.update(await self._calculate_formality_metrics(user_id))

        # Get learning metrics
        metrics.update(await self._calculate_learning_metrics(user_id))

        # Get temporal metrics
        metrics.update(await self._calculate_temporal_metrics(user_id))

        # Get record metrics
        metrics.update(await self._calculate_record_metrics(user_id))

        # Get achievement counts (for meta achievements)
        metrics.update(await self._calculate_achievement_metrics(user_id))

        return metrics

    async def _calculate_streaks(self, user_id: int) -> dict[str, Any]:
        """Calculate streak-related metrics."""
        # Get all dates with transcriptions
        result = await self.db.execute(
            select(func.date(Transcript.created_at))
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
            .distinct()
            .order_by(func.date(Transcript.created_at))
        )
        dates = [row[0] for row in result.all()]

        if not dates:
            return {
                "current_streak": 0,
                "longest_streak": 0,
                "total_active_days": 0,
                "perfect_weeks": 0,
                "perfect_months": 0,
                "comeback_count": 0,
            }

        total_active_days = len(dates)
        current_streak = 0
        longest_streak = 0
        streak = 1
        perfect_weeks = 0
        perfect_months = 0
        comeback_count = 0

        today = datetime.now(timezone.utc).date()

        # Calculate current streak (from today backwards)
        if dates:
            last_date = dates[-1]
            if last_date == today or last_date == today - timedelta(days=1):
                current_streak = 1
                for i in range(len(dates) - 2, -1, -1):
                    if dates[i] == dates[i + 1] - timedelta(days=1):
                        current_streak += 1
                    else:
                        break

        # Calculate longest streak and comebacks
        for i in range(1, len(dates)):
            diff = (dates[i] - dates[i-1]).days
            if diff == 1:
                streak += 1
            else:
                longest_streak = max(longest_streak, streak)
                streak = 1
                if diff > 7:  # Count as comeback if >7 day gap
                    comeback_count += 1

        longest_streak = max(longest_streak, streak)

        # Perfect weeks/months calculation
        # A perfect week: 7 consecutive days, perfect month: 30 consecutive days
        # C1 FIX: Use single counter without premature reset — count all milestones
        consecutive = 1
        for i in range(1, len(dates)):
            if (dates[i] - dates[i-1]).days == 1:
                consecutive += 1
            else:
                # Streak broken — count completed weeks/months from this streak
                perfect_weeks += consecutive // 7
                perfect_months += consecutive // 30
                consecutive = 1
        # Count the final streak
        perfect_weeks += consecutive // 7
        perfect_months += consecutive // 30

        return {
            "current_streak": current_streak,
            "longest_streak": longest_streak,
            "total_active_days": total_active_days,
            "perfect_weeks": perfect_weeks,
            "perfect_months": perfect_months,
            "comeback_count": comeback_count,
        }

    async def _calculate_speed_metrics(self, user_id: int) -> dict[str, Any]:
        """Calculate speed-related metrics.

        Filters out very short transcriptions (< 5s audio or < 10 words)
        to avoid inflated WPM from accidental/tiny recordings.
        """
        speed_filter = and_(
            Transcript.user_id == user_id, Transcript.transcript_type == "input",
            Transcript.words_per_minute.isnot(None),
            Transcript.audio_duration_seconds >= 5,
            Transcript.word_count >= 10,
        )

        result = await self.db.execute(
            select(
                func.max(Transcript.words_per_minute),
                func.avg(Transcript.words_per_minute),
            )
            .where(speed_filter)
        )
        row = result.one()

        # Count high-speed transcriptions (150+ WPM)
        result = await self.db.execute(
            select(func.count(Transcript.id))
            .where(
                and_(
                    speed_filter,
                    Transcript.words_per_minute >= 150
                )
            )
        )
        high_speed_count = result.scalar() or 0

        result = await self.db.execute(
            select(func.count(Transcript.id))
            .where(
                and_(
                    speed_filter,
                    Transcript.words_per_minute >= 200
                )
            )
        )
        ultra_speed_count = result.scalar() or 0

        return {
            "fastest_wpm": row[0] or 0,
            "average_wpm": row[1] or 0,
            "high_speed_count": high_speed_count,
            "ultra_speed_count": ultra_speed_count,
        }

    async def _calculate_context_metrics(self, user_id: int) -> dict[str, Any]:
        """Calculate context-related metrics."""
        result = await self.db.execute(
            select(
                Transcript.context,
                func.count(Transcript.id),
                func.sum(Transcript.word_count),
            )
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
            .group_by(Transcript.context)
        )
        rows = result.all()

        metrics = {}
        for context, count, words in rows:
            if context:
                ctx_key = context.lower().replace(" ", "_").replace("-", "_")
                metrics[f"context_{ctx_key}_count"] = count
                metrics[f"context_{ctx_key}_words"] = words or 0

        return metrics

    async def _calculate_formality_metrics(self, user_id: int) -> dict[str, Any]:
        """Calculate formality-related metrics."""
        result = await self.db.execute(
            select(
                Transcript.formality,
                func.count(Transcript.id),
                func.sum(Transcript.word_count),
            )
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
            .group_by(Transcript.formality)
        )
        rows = result.all()

        metrics = {}
        for formality, count, words in rows:
            if formality:
                form_key = formality.lower()
                metrics[f"formality_{form_key}_count"] = count
                metrics[f"formality_{form_key}_words"] = words or 0

        return metrics

    async def _calculate_learning_metrics(self, user_id: int) -> dict[str, Any]:
        """Calculate learning/AI training metrics."""
        # Corrections count
        result = await self.db.execute(
            select(func.count(CorrectionEmbedding.id))
            .where(CorrectionEmbedding.user_id == user_id)
        )
        total_corrections = result.scalar() or 0

        # Audio samples
        result = await self.db.execute(
            select(func.count(AudioSample.id))
            .where(AudioSample.user_id == user_id)
        )
        audio_samples = result.scalar() or 0

        # Dictionary entries
        result = await self.db.execute(
            select(func.count(DictionaryEntry.id))
            .where(DictionaryEntry.user_id == user_id)
        )
        dictionary_entries = result.scalar() or 0

        # Correction rules
        result = await self.db.execute(
            select(func.count(CorrectionRule.id))
            .where(CorrectionRule.user_id == user_id)
        )
        correction_rules = result.scalar() or 0

        return {
            "total_corrections": total_corrections,
            "spelling_corrections": total_corrections // 2,  # Simplified split
            "grammar_corrections": total_corrections // 2,
            "audio_samples": audio_samples,
            "dictionary_entries": dictionary_entries,
            "correction_rules": correction_rules,
        }

    async def _calculate_temporal_metrics(self, user_id: int) -> dict[str, Any]:
        """Calculate time-based metrics."""
        metrics = {}

        # Hour of day counts
        result = await self.db.execute(
            select(
                func.extract("hour", Transcript.created_at).label("hour"),
                func.count(Transcript.id),
            )
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
            .group_by("hour")
        )
        for hour, count in result.all():
            metrics[f"hour_{int(hour)}_count"] = count

        # Day of week counts
        result = await self.db.execute(
            select(
                func.extract("dow", Transcript.created_at).label("dow"),
                func.count(Transcript.id),
            )
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
            .group_by("dow")
        )
        for dow, count in result.all():
            metrics[f"day_{int(dow)}_count"] = count

        # Early bird (before 7 AM)
        result = await self.db.execute(
            select(func.count(Transcript.id))
            .where(
                and_(
                    Transcript.user_id == user_id, Transcript.transcript_type == "input",
                    func.extract("hour", Transcript.created_at) < 7
                )
            )
        )
        metrics["early_bird_count"] = result.scalar() or 0

        # Night owl (after 10 PM)
        result = await self.db.execute(
            select(func.count(Transcript.id))
            .where(
                and_(
                    Transcript.user_id == user_id, Transcript.transcript_type == "input",
                    func.extract("hour", Transcript.created_at) >= 22
                )
            )
        )
        metrics["night_owl_count"] = result.scalar() or 0

        # Weekend count
        result = await self.db.execute(
            select(func.count(Transcript.id))
            .where(
                and_(
                    Transcript.user_id == user_id, Transcript.transcript_type == "input",
                    or_(
                        func.extract("dow", Transcript.created_at) == 0,
                        func.extract("dow", Transcript.created_at) == 6,
                    )
                )
            )
        )
        metrics["weekend_count"] = result.scalar() or 0

        # Workweek count
        result = await self.db.execute(
            select(func.count(Transcript.id))
            .where(
                and_(
                    Transcript.user_id == user_id, Transcript.transcript_type == "input",
                    func.extract("dow", Transcript.created_at).between(1, 5)
                )
            )
        )
        metrics["workweek_count"] = result.scalar() or 0

        return metrics

    async def _calculate_record_metrics(self, user_id: int) -> dict[str, Any]:
        """Calculate record/milestone metrics."""
        # Longest single transcription
        result = await self.db.execute(
            select(func.max(Transcript.word_count))
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
        )
        longest_transcription = result.scalar() or 0

        # Time saved (based on typing WPM)
        user_result = await self.db.execute(
            select(User.typing_wpm, User.total_words, User.created_at)
            .where(User.id == user_id)
        )
        user_row = user_result.one_or_none()

        time_saved_minutes = 0
        account_age_months = 0
        if user_row:
            typing_wpm = user_row[0] or 40
            total_words = user_row[1] or 0
            time_saved_minutes = total_words / typing_wpm if typing_wpm > 0 else 0

            # Account age
            created_at = user_row[2]
            if created_at:
                age_days = (datetime.now(timezone.utc) - created_at.replace(tzinfo=None)).days
                account_age_months = age_days // 30

        # Daily records — use subquery to avoid nested aggregates
        daily_sums = (
            select(func.sum(Transcript.word_count).label("daily_words"))
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
            .group_by(func.date(Transcript.created_at))
            .subquery()
        )
        result = await self.db.execute(
            select(func.max(daily_sums.c.daily_words))
        )
        daily_word_record = result.scalar() or 0

        # Weekly records — sum words per ISO year-week (PostgreSQL)
        weekly_sums = (
            select(func.sum(Transcript.word_count).label("weekly_words"))
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
            .group_by(
                func.extract("isoyear", Transcript.created_at),
                func.extract("week", Transcript.created_at),
            )
            .subquery()
        )
        result = await self.db.execute(
            select(func.max(weekly_sums.c.weekly_words))
        )
        weekly_word_record = result.scalar() or 0

        # Monthly records — sum words per year-month (PostgreSQL)
        monthly_sums = (
            select(func.sum(Transcript.word_count).label("monthly_words"))
            .where(Transcript.user_id == user_id, Transcript.transcript_type == "input")
            .group_by(
                func.extract("year", Transcript.created_at),
                func.extract("month", Transcript.created_at),
            )
            .subquery()
        )
        result = await self.db.execute(
            select(func.max(monthly_sums.c.monthly_words))
        )
        monthly_word_record = result.scalar() or 0

        return {
            "longest_transcription": longest_transcription,
            "time_saved_minutes": time_saved_minutes,
            "account_age_months": account_age_months,
            "daily_word_record": daily_word_record,
            "weekly_word_record": weekly_word_record,
            "monthly_word_record": monthly_word_record,
        }

    async def _calculate_achievement_metrics(self, user_id: int) -> dict[str, Any]:
        """Calculate meta-achievement metrics."""
        # Total achievements unlocked
        result = await self.db.execute(
            select(func.count(UserAchievement.id))
            .where(
                and_(
                    UserAchievement.user_id == user_id,
                    UserAchievement.is_unlocked == True
                )
            )
        )
        total_achievements = result.scalar() or 0

        # By rarity
        result = await self.db.execute(
            select(
                AchievementDefinition.rarity,
                func.count(UserAchievement.id)
            )
            .join(UserAchievement, UserAchievement.achievement_id == AchievementDefinition.id)
            .where(
                and_(
                    UserAchievement.user_id == user_id,
                    UserAchievement.is_unlocked == True
                )
            )
            .group_by(AchievementDefinition.rarity)
        )
        rarity_counts = {row[0]: row[1] for row in result.all()}  # rarity is already a string

        return {
            "total_achievements": total_achievements,
            "common_achievements": rarity_counts.get("common", 0),
            "rare_achievements": rarity_counts.get("rare", 0),
            "epic_achievements": rarity_counts.get("epic", 0),
            "legendary_achievements": rarity_counts.get("legendary", 0),
        }

    async def check_achievements(self, user_id: int) -> list[dict[str, Any]]:
        """
        Check all achievements and unlock any that are newly earned.
        Returns list of newly unlocked achievements.
        """
        metrics = await self.get_user_metrics(user_id)
        newly_unlocked = []

        # Get all achievement definitions
        result = await self.db.execute(select(AchievementDefinition))
        definitions = result.scalars().all()

        for definition in definitions:
            # Get or create user achievement record
            result = await self.db.execute(
                select(UserAchievement).where(
                    and_(
                        UserAchievement.user_id == user_id,
                        UserAchievement.achievement_id == definition.id,
                    )
                )
            )
            user_achievement = result.scalar_one_or_none()

            if not user_achievement:
                user_achievement = UserAchievement(
                    user_id=user_id,
                    achievement_id=definition.id,
                    current_value=0,
                    is_unlocked=False,
                )
                self.db.add(user_achievement)
                await self.db.flush()

            # Always update current_value so display stays accurate
            current_value = metrics.get(definition.metric_type, 0)
            user_achievement.current_value = current_value

            # Skip unlock logic if already unlocked
            if user_achievement.is_unlocked:
                continue

            # Check if threshold is met
            if current_value >= definition.threshold:
                user_achievement.is_unlocked = True
                user_achievement.unlocked_at = datetime.now(timezone.utc)

                # Award XP
                await self.gamification_service.award_achievement_xp(
                    user_id=user_id,
                    achievement_id=definition.id,
                    xp_amount=definition.xp_reward,
                )

                # Update achievement count
                gamification = await self.gamification_service.get_or_create_user_gamification(user_id)
                gamification.achievements_unlocked += 1

                newly_unlocked.append({
                    "id": definition.id,
                    "name": definition.name,
                    "description": definition.description,
                    "category": definition.category,  # Already a string
                    "rarity": definition.rarity,  # Already a string
                    "xp_reward": definition.xp_reward,
                    "icon": definition.icon,
                    "tier": definition.tier,
                    "unlocked_at": user_achievement.unlocked_at.isoformat(),
                })

        await self.db.flush()
        return newly_unlocked

    async def get_achievements(
        self,
        user_id: int,
        category: str | None = None,
        rarity: str | None = None,
        unlocked_only: bool = False,
        page: int = 1,
        page_size: int = 50,
    ) -> dict[str, Any]:
        """Get paginated achievements for a user with optional filters."""
        # Build query for definitions
        query = select(AchievementDefinition)

        if category:
            query = query.where(AchievementDefinition.category == category.lower())
        if rarity:
            query = query.where(AchievementDefinition.rarity == rarity.lower())

        # If unlocked_only, filter to only definitions the user has unlocked
        if unlocked_only:
            query = query.where(
                exists(
                    select(UserAchievement.id).where(
                        and_(
                            UserAchievement.achievement_id == AchievementDefinition.id,
                            UserAchievement.user_id == user_id,
                            UserAchievement.is_unlocked == True,
                        )
                    )
                )
            )

        # Get total count
        count_query = select(func.count()).select_from(query.subquery())
        result = await self.db.execute(count_query)
        total = result.scalar() or 0

        # Apply pagination
        query = query.order_by(AchievementDefinition.category, AchievementDefinition.tier)
        query = query.offset((page - 1) * page_size).limit(page_size)

        result = await self.db.execute(query)
        definitions = result.scalars().all()

        # Get user achievements for these definitions
        achievement_ids = [d.id for d in definitions]
        result = await self.db.execute(
            select(UserAchievement)
            .where(
                and_(
                    UserAchievement.user_id == user_id,
                    UserAchievement.achievement_id.in_(achievement_ids),
                )
            )
        )
        user_achievements = {ua.achievement_id: ua for ua in result.scalars().all()}

        achievements = []
        for definition in definitions:
            ua = user_achievements.get(definition.id)
            is_unlocked = ua.is_unlocked if ua else False

            achievements.append({
                "id": definition.id,
                "name": definition.name,
                "description": definition.description,
                "category": definition.category,  # Already a string
                "rarity": definition.rarity,  # Already a string
                "xp_reward": definition.xp_reward,
                "icon": definition.icon,
                "tier": definition.tier,
                "threshold": definition.threshold,
                "metric_type": definition.metric_type,
                "is_hidden": definition.is_hidden,
                "is_unlocked": is_unlocked,
                "current_value": ua.current_value if ua else 0,
                "progress": min((ua.current_value / definition.threshold) if ua and definition.threshold > 0 else 0, 1.0),
                "unlocked_at": ua.unlocked_at.isoformat() if ua and ua.unlocked_at else None,
            })

        return {
            "achievements": achievements,
            "total": total,
            "page": page,
            "page_size": page_size,
            "total_pages": (total + page_size - 1) // page_size,
        }

    async def get_unnotified_achievements(self, user_id: int) -> list[dict[str, Any]]:
        """Get achievements that haven't been notified to the user yet."""
        result = await self.db.execute(
            select(UserAchievement)
            .options(selectinload(UserAchievement.achievement))
            .where(
                and_(
                    UserAchievement.user_id == user_id,
                    UserAchievement.is_unlocked == True,
                    UserAchievement.notified == False,
                )
            )
            .order_by(UserAchievement.unlocked_at.desc())
        )
        user_achievements = result.scalars().all()

        achievements = []
        for ua in user_achievements:
            achievements.append({
                "id": ua.achievement.id,
                "name": ua.achievement.name,
                "description": ua.achievement.description,
                "category": ua.achievement.category,  # Already a string
                "rarity": ua.achievement.rarity,  # Already a string
                "xp_reward": ua.achievement.xp_reward,
                "icon": ua.achievement.icon,
                "tier": ua.achievement.tier,
                "unlocked_at": ua.unlocked_at.isoformat() if ua.unlocked_at else None,
            })

        return achievements

    async def mark_achievements_notified(self, user_id: int, achievement_ids: list[str]) -> None:
        """Mark achievements as notified."""
        await self.db.execute(
            UserAchievement.__table__.update()
            .where(
                and_(
                    UserAchievement.user_id == user_id,
                    UserAchievement.achievement_id.in_(achievement_ids),
                )
            )
            .values(notified=True)
        )
        await self.db.flush()


# =============================================================================
# LEADERBOARD SERVICE
# =============================================================================

class LeaderboardService:
    """Service for leaderboard functionality."""

    def __init__(self, db: AsyncSession):
        self.db = db

    async def get_leaderboard(
        self,
        metric: str = "lifetime_xp",
        limit: int = 100,
    ) -> list[dict[str, Any]]:
        """Get top users by a given metric."""
        if metric == "lifetime_xp":
            query = (
                select(
                    User.id,
                    User.full_name,
                    UserGamification.lifetime_xp,
                    UserGamification.current_level,
                    UserGamification.prestige_tier,
                    UserGamification.achievements_unlocked,
                )
                .join(UserGamification, UserGamification.user_id == User.id)
                .order_by(UserGamification.lifetime_xp.desc())
                .limit(limit)
            )
        elif metric == "achievements":
            query = (
                select(
                    User.id,
                    User.full_name,
                    UserGamification.lifetime_xp,
                    UserGamification.current_level,
                    UserGamification.prestige_tier,
                    UserGamification.achievements_unlocked,
                )
                .join(UserGamification, UserGamification.user_id == User.id)
                .order_by(UserGamification.achievements_unlocked.desc())
                .limit(limit)
            )
        elif metric == "words":
            query = (
                select(
                    User.id,
                    User.full_name,
                    User.total_words,
                    UserGamification.current_level,
                    UserGamification.prestige_tier,
                    UserGamification.achievements_unlocked,
                )
                .outerjoin(UserGamification, UserGamification.user_id == User.id)
                .order_by(User.total_words.desc())
                .limit(limit)
            )
        else:
            return []

        result = await self.db.execute(query)
        rows = result.all()

        leaderboard = []
        for i, row in enumerate(rows, 1):
            entry = {
                "rank": i,
                "user_id": row[0],
                "display_name": row[1] or f"User {row[0]}",
            }

            if metric == "words":
                entry["total_words"] = row[2]
                entry["level"] = row[3] or 1
                entry["tier"] = row[4] if row[4] else "bronze"  # Already a string
                entry["achievements"] = row[5] or 0
            else:
                entry["lifetime_xp"] = row[2]
                entry["level"] = row[3]
                entry["tier"] = row[4]  # Already a string
                entry["achievements"] = row[5]

            leaderboard.append(entry)

        return leaderboard

    async def get_user_rank(self, user_id: int, metric: str = "lifetime_xp") -> dict[str, Any]:
        """Get a specific user's rank."""
        # Ensure user has gamification record (creates with retroactive XP if needed)
        gamification_service = GamificationService(self.db)
        await gamification_service.get_or_create_user_gamification(user_id)
        await self.db.flush()  # Ensure record is visible for count

        if metric == "lifetime_xp":
            # Count users with more XP
            result = await self.db.execute(
                select(func.count(UserGamification.id))
                .where(
                    UserGamification.lifetime_xp > (
                        select(UserGamification.lifetime_xp)
                        .where(UserGamification.user_id == user_id)
                        .scalar_subquery()
                    )
                )
            )
            rank = (result.scalar() or 0) + 1

            # Get user's XP
            result = await self.db.execute(
                select(UserGamification.lifetime_xp)
                .where(UserGamification.user_id == user_id)
            )
            value = result.scalar() or 0
        else:
            rank = 0
            value = 0

        # Get total users
        result = await self.db.execute(select(func.count(UserGamification.id)))
        total = result.scalar() or 0

        return {
            "user_id": user_id,
            "rank": rank,
            "total_users": total,
            "metric": metric,
            "value": value,
            "percentile": ((total - rank) / total * 100) if total > 0 else 0,
        }
