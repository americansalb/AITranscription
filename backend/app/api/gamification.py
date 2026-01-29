"""Gamification API endpoints for XP, levels, achievements, and leaderboards."""

import logging
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Query, status
from pydantic import BaseModel, Field
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.core.database import get_db
from app.models.user import User
from app.models.gamification import AchievementCategory, AchievementRarity
from app.services.gamification import (
    GamificationService,
    AchievementService,
    LeaderboardService,
)

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/gamification", tags=["gamification"])


# =============================================================================
# RESPONSE SCHEMAS
# =============================================================================

class TierProgressResponse(BaseModel):
    """Tier progress information."""
    current_tier: str
    next_tier: str | None
    tier_start_xp: int
    tier_end_xp: int | None
    xp_in_tier: int
    progress: float
    color: str


class AchievementStatsResponse(BaseModel):
    """Achievement statistics."""
    unlocked: int
    total: int
    progress: float
    by_rarity: dict[str, int]


class GamificationProgressResponse(BaseModel):
    """Full gamification progress response."""
    user_id: int
    current_level: int
    current_xp: int
    xp_to_next_level: int
    level_progress: float
    lifetime_xp: int
    prestige_tier: str
    tier_color: str
    tier_progress: TierProgressResponse
    xp_multiplier: float
    achievements: AchievementStatsResponse
    last_xp_earned_at: str | None


class XPTransactionResponse(BaseModel):
    """XP transaction record."""
    id: int
    amount: int
    final_amount: int
    multiplier: float
    source: str
    source_id: str | None
    description: str | None
    level_before: int
    level_after: int
    created_at: str


class AchievementResponse(BaseModel):
    """Single achievement with user progress."""
    id: str
    name: str
    description: str
    category: str
    rarity: str
    xp_reward: int
    icon: str
    tier: int
    threshold: float
    metric_type: str
    is_hidden: bool
    is_unlocked: bool
    current_value: float
    progress: float
    unlocked_at: str | None


class AchievementListResponse(BaseModel):
    """Paginated list of achievements."""
    achievements: list[AchievementResponse]
    total: int
    page: int
    page_size: int
    total_pages: int


class NewlyUnlockedResponse(BaseModel):
    """Response for newly unlocked achievements."""
    achievements: list[dict[str, Any]]
    xp_gained: int
    level_changes: dict[str, Any] | None


class LeaderboardEntryResponse(BaseModel):
    """Single leaderboard entry."""
    rank: int
    user_id: int
    display_name: str
    level: int
    tier: str
    achievements: int
    lifetime_xp: int | None = None
    total_words: int | None = None


class LeaderboardResponse(BaseModel):
    """Leaderboard response."""
    leaderboard: list[LeaderboardEntryResponse]
    user_rank: dict[str, Any] | None = None


class MarkNotifiedRequest(BaseModel):
    """Request to mark achievements as notified."""
    achievement_ids: list[str]


# =============================================================================
# ENDPOINTS
# =============================================================================

@router.get("/progress", response_model=GamificationProgressResponse)
async def get_progress(
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
) -> dict[str, Any]:
    """Get the current user's gamification progress including XP, level, and tier."""
    service = GamificationService(db)
    progress = await service.get_user_progress(current_user.id)
    return progress


@router.get("/transactions", response_model=list[XPTransactionResponse])
async def get_xp_transactions(
    limit: int = Query(default=20, ge=1, le=100),
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
) -> list[dict[str, Any]]:
    """Get recent XP transactions for the current user."""
    service = GamificationService(db)
    transactions = await service.get_recent_xp_transactions(current_user.id, limit)
    return transactions


@router.post("/check", response_model=NewlyUnlockedResponse)
async def check_achievements(
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
) -> dict[str, Any]:
    """Check for and unlock any newly earned achievements."""
    service = AchievementService(db)
    newly_unlocked = await service.check_achievements(current_user.id)

    # Calculate total XP gained from new achievements
    xp_gained = sum(a["xp_reward"] for a in newly_unlocked)

    # Get updated progress if there were new achievements
    level_changes = None
    if newly_unlocked:
        gamification_service = GamificationService(db)
        progress = await gamification_service.get_user_progress(current_user.id)
        level_changes = {
            "current_level": progress["current_level"],
            "prestige_tier": progress["prestige_tier"],
            "lifetime_xp": progress["lifetime_xp"],
        }

    await db.commit()

    return {
        "achievements": newly_unlocked,
        "xp_gained": xp_gained,
        "level_changes": level_changes,
    }


@router.get("/achievements", response_model=AchievementListResponse)
async def get_achievements(
    category: str | None = Query(default=None, description="Filter by category"),
    rarity: str | None = Query(default=None, description="Filter by rarity"),
    unlocked_only: bool = Query(default=False, description="Only show unlocked achievements"),
    page: int = Query(default=1, ge=1),
    page_size: int = Query(default=50, ge=1, le=100),
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
) -> dict[str, Any]:
    """Get paginated list of achievements with user progress."""
    # Validate category if provided
    if category:
        try:
            AchievementCategory(category)
        except ValueError:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=f"Invalid category. Must be one of: {[c.value for c in AchievementCategory]}",
            )

    # Validate rarity if provided
    if rarity:
        try:
            AchievementRarity(rarity)
        except ValueError:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=f"Invalid rarity. Must be one of: {[r.value for r in AchievementRarity]}",
            )

    service = AchievementService(db)
    result = await service.get_achievements(
        user_id=current_user.id,
        category=category,
        rarity=rarity,
        unlocked_only=unlocked_only,
        page=page,
        page_size=page_size,
    )
    return result


@router.get("/achievements/unnotified", response_model=list[AchievementResponse])
async def get_unnotified_achievements(
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
) -> list[dict[str, Any]]:
    """Get achievements that haven't been shown to the user yet."""
    service = AchievementService(db)
    achievements = await service.get_unnotified_achievements(current_user.id)
    return achievements


@router.post("/achievements/mark-notified")
async def mark_achievements_notified(
    request: MarkNotifiedRequest,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
) -> dict[str, str]:
    """Mark achievements as notified (user has seen them)."""
    service = AchievementService(db)
    await service.mark_achievements_notified(current_user.id, request.achievement_ids)
    await db.commit()
    return {"status": "ok"}


@router.get("/leaderboard", response_model=LeaderboardResponse)
async def get_leaderboard(
    metric: str = Query(default="lifetime_xp", description="Metric to rank by: lifetime_xp, achievements, words"),
    limit: int = Query(default=100, ge=1, le=500),
    include_user_rank: bool = Query(default=True, description="Include current user's rank"),
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
) -> dict[str, Any]:
    """Get the leaderboard with optional user rank."""
    if metric not in ["lifetime_xp", "achievements", "words"]:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid metric. Must be one of: lifetime_xp, achievements, words",
        )

    service = LeaderboardService(db)
    leaderboard = await service.get_leaderboard(metric=metric, limit=limit)

    user_rank = None
    if include_user_rank:
        user_rank = await service.get_user_rank(current_user.id, metric=metric)

    return {
        "leaderboard": leaderboard,
        "user_rank": user_rank,
    }


@router.get("/categories")
async def get_categories() -> dict[str, list[str]]:
    """Get available achievement categories and rarities."""
    return {
        "categories": [c.value for c in AchievementCategory],
        "rarities": [r.value for r in AchievementRarity],
    }
