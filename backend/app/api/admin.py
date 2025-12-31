"""Admin API routes for user management and statistics."""

from datetime import datetime, timedelta
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, Query, status
from pydantic import BaseModel, EmailStr, Field
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.core.database import get_db
from app.models.user import SubscriptionTier, User
from app.models.transcript import Transcript
from app.services.auth import create_user, hash_password

router = APIRouter(prefix="/admin", tags=["admin"])


# Dependency to require admin access
async def require_admin(current_user: User = Depends(get_current_user)) -> User:
    """Dependency that requires the current user to be an admin."""
    if not current_user.is_admin:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Admin access required",
        )
    return current_user


# Schemas
class UserListResponse(BaseModel):
    """Response for user list."""

    id: int
    email: str
    full_name: str | None
    tier: SubscriptionTier
    is_active: bool
    is_admin: bool
    daily_transcription_limit: int
    daily_transcriptions_used: int
    total_transcriptions: int
    total_words: int
    total_audio_seconds: int
    created_at: datetime


class UserDetailResponse(UserListResponse):
    """Detailed user response with more stats."""

    accessibility_verified: bool
    last_usage_reset: datetime | None
    updated_at: datetime


class CreateUserRequest(BaseModel):
    """Request to create a new user."""

    email: EmailStr
    password: str = Field(min_length=8)
    full_name: str | None = None
    tier: SubscriptionTier = SubscriptionTier.STANDARD
    is_active: bool = True
    daily_transcription_limit: int = 0  # 0 = unlimited


class UpdateUserRequest(BaseModel):
    """Request to update a user."""

    email: EmailStr | None = None
    full_name: str | None = None
    tier: SubscriptionTier | None = None
    is_active: bool | None = None
    is_admin: bool | None = None
    daily_transcription_limit: int | None = None
    password: str | None = Field(None, min_length=8)


class UserStatsResponse(BaseModel):
    """User statistics response."""

    user_id: int
    email: str
    total_transcriptions: int
    total_words: int
    total_audio_seconds: float
    total_characters: int
    average_words_per_minute: float
    average_words_per_transcription: float
    transcriptions_today: int
    transcriptions_this_week: int
    transcriptions_this_month: int
    words_today: int
    words_this_week: int
    words_this_month: int


class GlobalStatsResponse(BaseModel):
    """Global statistics response."""

    total_users: int
    active_users: int
    total_transcriptions: int
    total_words: int
    total_audio_hours: float
    transcriptions_today: int
    transcriptions_this_week: int
    users_by_tier: dict[str, int]


class TranscriptListItem(BaseModel):
    """Item in transcript list."""

    id: int
    raw_text: str
    polished_text: str
    word_count: int
    audio_duration_seconds: float
    words_per_minute: float
    context: str | None
    created_at: datetime


# Routes

@router.get("/users", response_model=list[UserListResponse])
async def list_users(
    skip: int = Query(0, ge=0),
    limit: int = Query(50, ge=1, le=100),
    active_only: bool = False,
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """List all users (admin only)."""
    query = select(User).order_by(User.created_at.desc()).offset(skip).limit(limit)
    if active_only:
        query = query.where(User.is_active == True)

    result = await db.execute(query)
    users = result.scalars().all()

    return [
        UserListResponse(
            id=u.id,
            email=u.email,
            full_name=u.full_name,
            tier=u.tier,
            is_active=u.is_active,
            is_admin=u.is_admin,
            daily_transcription_limit=u.daily_transcription_limit,
            daily_transcriptions_used=u.daily_transcriptions_used,
            total_transcriptions=u.total_transcriptions,
            total_words=u.total_words,
            total_audio_seconds=u.total_audio_seconds,
            created_at=u.created_at,
        )
        for u in users
    ]


@router.get("/users/{user_id}", response_model=UserDetailResponse)
async def get_user(
    user_id: int,
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """Get user details (admin only)."""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()

    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    return UserDetailResponse(
        id=user.id,
        email=user.email,
        full_name=user.full_name,
        tier=user.tier,
        is_active=user.is_active,
        is_admin=user.is_admin,
        daily_transcription_limit=user.daily_transcription_limit,
        daily_transcriptions_used=user.daily_transcriptions_used,
        total_transcriptions=user.total_transcriptions,
        total_words=user.total_words,
        total_audio_seconds=user.total_audio_seconds,
        accessibility_verified=user.accessibility_verified,
        last_usage_reset=user.last_usage_reset,
        created_at=user.created_at,
        updated_at=user.updated_at,
    )


@router.post("/users", response_model=UserDetailResponse, status_code=status.HTTP_201_CREATED)
async def create_user_admin(
    request: CreateUserRequest,
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """Create a new user (admin only)."""
    # Check if email exists
    result = await db.execute(select(User).where(User.email == request.email))
    if result.scalar_one_or_none():
        raise HTTPException(status_code=400, detail="Email already registered")

    # Create user
    user = User(
        email=request.email,
        hashed_password=hash_password(request.password),
        full_name=request.full_name,
        tier=request.tier,
        is_active=request.is_active,
        daily_transcription_limit=request.daily_transcription_limit,
    )
    db.add(user)
    await db.commit()
    await db.refresh(user)

    return UserDetailResponse(
        id=user.id,
        email=user.email,
        full_name=user.full_name,
        tier=user.tier,
        is_active=user.is_active,
        is_admin=user.is_admin,
        daily_transcription_limit=user.daily_transcription_limit,
        daily_transcriptions_used=user.daily_transcriptions_used,
        total_transcriptions=user.total_transcriptions,
        total_words=user.total_words,
        total_audio_seconds=user.total_audio_seconds,
        accessibility_verified=user.accessibility_verified,
        last_usage_reset=user.last_usage_reset,
        created_at=user.created_at,
        updated_at=user.updated_at,
    )


@router.patch("/users/{user_id}", response_model=UserDetailResponse)
async def update_user(
    user_id: int,
    request: UpdateUserRequest,
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """Update a user (admin only)."""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()

    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    # Update fields
    if request.email is not None:
        # Check if email is taken by another user
        existing = await db.execute(
            select(User).where(User.email == request.email, User.id != user_id)
        )
        if existing.scalar_one_or_none():
            raise HTTPException(status_code=400, detail="Email already in use")
        user.email = request.email

    if request.full_name is not None:
        user.full_name = request.full_name
    if request.tier is not None:
        user.tier = request.tier
    if request.is_active is not None:
        user.is_active = request.is_active
    if request.is_admin is not None:
        user.is_admin = request.is_admin
    if request.daily_transcription_limit is not None:
        user.daily_transcription_limit = request.daily_transcription_limit
    if request.password is not None:
        user.hashed_password = hash_password(request.password)

    await db.commit()
    await db.refresh(user)

    return UserDetailResponse(
        id=user.id,
        email=user.email,
        full_name=user.full_name,
        tier=user.tier,
        is_active=user.is_active,
        is_admin=user.is_admin,
        daily_transcription_limit=user.daily_transcription_limit,
        daily_transcriptions_used=user.daily_transcriptions_used,
        total_transcriptions=user.total_transcriptions,
        total_words=user.total_words,
        total_audio_seconds=user.total_audio_seconds,
        accessibility_verified=user.accessibility_verified,
        last_usage_reset=user.last_usage_reset,
        created_at=user.created_at,
        updated_at=user.updated_at,
    )


@router.delete("/users/{user_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_user(
    user_id: int,
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """Delete a user (admin only)."""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()

    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    if user.is_admin:
        raise HTTPException(status_code=400, detail="Cannot delete admin users")

    await db.delete(user)
    await db.commit()


@router.get("/users/{user_id}/stats", response_model=UserStatsResponse)
async def get_user_stats(
    user_id: int,
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """Get detailed statistics for a user (admin only)."""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()

    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    now = datetime.utcnow()
    today = now.replace(hour=0, minute=0, second=0, microsecond=0)
    week_ago = today - timedelta(days=7)
    month_ago = today - timedelta(days=30)

    # Get transcript stats
    transcripts_query = select(Transcript).where(Transcript.user_id == user_id)
    result = await db.execute(transcripts_query)
    transcripts = result.scalars().all()

    # Calculate stats
    total_characters = sum(t.character_count for t in transcripts)
    total_wpm = sum(t.words_per_minute for t in transcripts)
    avg_wpm = total_wpm / len(transcripts) if transcripts else 0
    avg_words = user.total_words / user.total_transcriptions if user.total_transcriptions else 0

    # Time-based stats
    transcripts_today = len([t for t in transcripts if t.created_at >= today])
    transcripts_week = len([t for t in transcripts if t.created_at >= week_ago])
    transcripts_month = len([t for t in transcripts if t.created_at >= month_ago])

    words_today = sum(t.word_count for t in transcripts if t.created_at >= today)
    words_week = sum(t.word_count for t in transcripts if t.created_at >= week_ago)
    words_month = sum(t.word_count for t in transcripts if t.created_at >= month_ago)

    return UserStatsResponse(
        user_id=user.id,
        email=user.email,
        total_transcriptions=user.total_transcriptions,
        total_words=user.total_words,
        total_audio_seconds=user.total_audio_seconds,
        total_characters=total_characters,
        average_words_per_minute=round(avg_wpm, 1),
        average_words_per_transcription=round(avg_words, 1),
        transcriptions_today=transcripts_today,
        transcriptions_this_week=transcripts_week,
        transcriptions_this_month=transcripts_month,
        words_today=words_today,
        words_this_week=words_week,
        words_this_month=words_month,
    )


@router.get("/users/{user_id}/transcripts", response_model=list[TranscriptListItem])
async def get_user_transcripts(
    user_id: int,
    skip: int = Query(0, ge=0),
    limit: int = Query(50, ge=1, le=100),
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """Get transcripts for a user (admin only)."""
    result = await db.execute(select(User).where(User.id == user_id))
    if not result.scalar_one_or_none():
        raise HTTPException(status_code=404, detail="User not found")

    query = (
        select(Transcript)
        .where(Transcript.user_id == user_id)
        .order_by(Transcript.created_at.desc())
        .offset(skip)
        .limit(limit)
    )
    result = await db.execute(query)
    transcripts = result.scalars().all()

    return [
        TranscriptListItem(
            id=t.id,
            raw_text=t.raw_text,
            polished_text=t.polished_text,
            word_count=t.word_count,
            audio_duration_seconds=t.audio_duration_seconds,
            words_per_minute=t.words_per_minute,
            context=t.context,
            created_at=t.created_at,
        )
        for t in transcripts
    ]


@router.get("/stats", response_model=GlobalStatsResponse)
async def get_global_stats(
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """Get global statistics (admin only)."""
    now = datetime.utcnow()
    today = now.replace(hour=0, minute=0, second=0, microsecond=0)
    week_ago = today - timedelta(days=7)

    # User counts
    total_users_result = await db.execute(select(func.count(User.id)))
    total_users = total_users_result.scalar() or 0

    active_users_result = await db.execute(
        select(func.count(User.id)).where(User.is_active == True)
    )
    active_users = active_users_result.scalar() or 0

    # Totals from users
    totals_result = await db.execute(
        select(
            func.sum(User.total_transcriptions),
            func.sum(User.total_words),
            func.sum(User.total_audio_seconds),
        )
    )
    totals = totals_result.one()
    total_transcriptions = totals[0] or 0
    total_words = totals[1] or 0
    total_audio_seconds = totals[2] or 0

    # Today's transcripts
    today_result = await db.execute(
        select(func.count(Transcript.id)).where(Transcript.created_at >= today)
    )
    transcriptions_today = today_result.scalar() or 0

    # This week's transcripts
    week_result = await db.execute(
        select(func.count(Transcript.id)).where(Transcript.created_at >= week_ago)
    )
    transcriptions_week = week_result.scalar() or 0

    # Users by tier
    tier_result = await db.execute(
        select(User.tier, func.count(User.id)).group_by(User.tier)
    )
    users_by_tier = {str(row[0].value): row[1] for row in tier_result.all()}

    return GlobalStatsResponse(
        total_users=total_users,
        active_users=active_users,
        total_transcriptions=total_transcriptions,
        total_words=total_words,
        total_audio_hours=round(total_audio_seconds / 3600, 2),
        transcriptions_today=transcriptions_today,
        transcriptions_this_week=transcriptions_week,
        users_by_tier=users_by_tier,
    )


@router.post("/make-admin/{user_id}")
async def make_user_admin(
    user_id: int,
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """Make a user an admin (admin only)."""
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()

    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    user.is_admin = True
    await db.commit()

    return {"message": f"User {user.email} is now an admin"}
