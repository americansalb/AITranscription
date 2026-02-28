"""Admin API routes for user management and statistics."""

import os
from datetime import datetime, timedelta, timezone
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, Query, Request, status
from fastapi.responses import HTMLResponse
from pydantic import BaseModel, EmailStr, Field, field_validator as pydantic_field_validator

from app.core.password import validate_password_strength as _validate_password
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.core.database import get_db
from app.core.config import settings
from app.models.user import SubscriptionTier, User
from app.models.transcript import Transcript
from app.services.auth import create_user, hash_password, get_user_by_email

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
    password: str = Field(min_length=8, max_length=72)
    full_name: str | None = None
    tier: SubscriptionTier = SubscriptionTier.STANDARD
    is_active: bool = True
    daily_transcription_limit: int = 0  # 0 = unlimited

    @pydantic_field_validator("password")
    @classmethod
    def validate_password_strength(cls, v: str) -> str:
        return _validate_password(v)


class UpdateUserRequest(BaseModel):
    """Request to update a user."""

    email: EmailStr | None = None
    full_name: str | None = None
    tier: SubscriptionTier | None = None
    is_active: bool | None = None
    is_admin: bool | None = None
    daily_transcription_limit: int | None = None
    password: str | None = Field(None, min_length=8, max_length=72)

    @pydantic_field_validator("password")
    @classmethod
    def validate_password_strength(cls, v: str | None) -> str | None:
        if v is None:
            return v
        return _validate_password(v)


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

    from datetime import timezone as tz
    now = datetime.now(tz.utc)
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
    now = datetime.now(timezone.utc)
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


@router.post("/reset-all-stats")
async def reset_all_user_stats(
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """
    Reset statistics for ALL users while keeping transcript history.
    This recalculates stats from actual transcript records.
    """
    # Get all users
    result = await db.execute(select(User))
    users = result.scalars().all()

    reset_count = 0
    for user in users:
        # Get actual transcript data for this user
        transcripts_result = await db.execute(
            select(Transcript).where(Transcript.user_id == user.id)
        )
        transcripts = transcripts_result.scalars().all()

        # Calculate real stats from transcripts
        total_transcriptions = len(transcripts)
        total_words = sum(t.word_count for t in transcripts)
        total_audio_seconds = sum(int(t.audio_duration_seconds) for t in transcripts)

        # Update user with recalculated stats
        user.total_transcriptions = total_transcriptions
        user.total_words = total_words
        user.total_audio_seconds = total_audio_seconds
        user.total_polish_tokens = 0  # Reset polish tokens (no way to recalculate)
        user.daily_transcriptions_used = 0  # Reset daily counter

        reset_count += 1

    await db.commit()

    return {
        "message": f"Reset stats for {reset_count} users based on actual transcript history",
        "users_reset": reset_count,
    }


@router.post("/reset-user-stats/{user_id}")
async def reset_single_user_stats(
    user_id: int,
    db: AsyncSession = Depends(get_db),
    admin: User = Depends(require_admin),
):
    """
    Reset statistics for a single user while keeping transcript history.
    This recalculates stats from actual transcript records.
    """
    result = await db.execute(select(User).where(User.id == user_id))
    user = result.scalar_one_or_none()

    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    # Get actual transcript data for this user
    transcripts_result = await db.execute(
        select(Transcript).where(Transcript.user_id == user.id)
    )
    transcripts = transcripts_result.scalars().all()

    # Calculate real stats from transcripts
    total_transcriptions = len(transcripts)
    total_words = sum(t.word_count for t in transcripts)
    total_audio_seconds = sum(int(t.audio_duration_seconds) for t in transcripts)

    # Update user with recalculated stats
    user.total_transcriptions = total_transcriptions
    user.total_words = total_words
    user.total_audio_seconds = total_audio_seconds
    user.total_polish_tokens = 0
    user.daily_transcriptions_used = 0

    await db.commit()

    return {
        "message": f"Reset stats for {user.email}",
        "stats": {
            "total_transcriptions": total_transcriptions,
            "total_words": total_words,
            "total_audio_seconds": total_audio_seconds,
        }
    }


class BootstrapRequest(BaseModel):
    """Request to bootstrap the first admin."""
    email: EmailStr
    secret: str


@router.post("/bootstrap")
async def bootstrap_admin(
    request: BootstrapRequest,
    raw_request: Request,
    db: AsyncSession = Depends(get_db),
):
    """
    Bootstrap endpoint to make an existing user an admin.
    Requires the SECRET_KEY from environment to authorize.
    This is a one-time setup endpoint for initial admin creation.
    """
    # Rate limit to prevent brute-force on secret key
    from app.api.auth import _check_rate_limit
    client_ip = raw_request.client.host if raw_request.client else "unknown"
    _check_rate_limit(f"bootstrap:{client_ip}", max_attempts=3, window_seconds=300)

    # Verify the secret matches our app secret key
    if request.secret != settings.secret_key:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Invalid secret key",
        )

    # Find the user
    user = await get_user_by_email(db, request.email)
    if not user:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="User not found. Please sign up first.",
        )

    # Make them admin and developer tier
    user.is_admin = True
    user.tier = SubscriptionTier.DEVELOPER
    user.daily_transcription_limit = 0  # 0 = unlimited
    await db.commit()

    return {
        "message": f"User {user.email} is now an admin with developer tier",
        "user_id": user.id,
    }


class SeedAdminsRequest(BaseModel):
    """Request to seed admin accounts."""
    secret: str


@router.post("/seed-admins")
async def seed_admin_accounts(
    request: SeedAdminsRequest,
    raw_request: Request,
    db: AsyncSession = Depends(get_db),
):
    """
    One-time endpoint to create the initial admin accounts.
    Requires SECRET_KEY to authorize.
    """
    # Rate limit to prevent brute-force on secret key
    from app.api.auth import _check_rate_limit
    client_ip = raw_request.client.host if raw_request.client else "unknown"
    _check_rate_limit(f"seed-admins:{client_ip}", max_attempts=3, window_seconds=300)

    if request.secret != settings.secret_key:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Invalid secret key",
        )

    # Admin accounts to create — password from environment variable
    admin_password = os.environ.get("ADMIN_BOOTSTRAP_PASSWORD")
    if not admin_password:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail="ADMIN_BOOTSTRAP_PASSWORD environment variable not set",
        )

    # Validate password meets strength requirements
    try:
        _validate_password(admin_password)
    except ValueError as e:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Bootstrap password too weak: {e}",
        )

    admin_accounts = [
        {"email": "iris@aalb.org", "password": admin_password},
        {"email": "kenil.thakkar@gmail.com", "password": admin_password},
        {"email": "happy102785@gmail.com", "password": admin_password},
    ]

    created = []
    skipped = []

    for account in admin_accounts:
        # Check if user already exists
        existing = await get_user_by_email(db, account["email"])
        if existing:
            # Update to admin if not already
            if not existing.is_admin:
                existing.is_admin = True
                existing.tier = SubscriptionTier.DEVELOPER
                existing.daily_transcription_limit = 0
                skipped.append(f"{account['email']} (updated to admin)")
            else:
                skipped.append(f"{account['email']} (already exists)")
            continue

        # Create new admin user
        user = User(
            email=account["email"],
            hashed_password=hash_password(account["password"]),
            is_admin=True,
            tier=SubscriptionTier.DEVELOPER,
            daily_transcription_limit=0,
            is_active=True,
        )
        db.add(user)
        created.append(account["email"])

    await db.commit()

    return {
        "message": "Admin accounts seeded",
        "created": created,
        "skipped": skipped,
    }


# Admin Dashboard HTML
ADMIN_DASHBOARD_HTML = '''
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Vaak Admin Dashboard</title>
    <style>
        :root {
            --bg-primary: #0f0f0f;
            --bg-secondary: #1a1a1a;
            --bg-tertiary: #252525;
            --text-primary: #ffffff;
            --text-secondary: #a0a0a0;
            --accent: #6366f1;
            --accent-hover: #818cf8;
            --success: #22c55e;
            --warning: #f59e0b;
            --danger: #ef4444;
            --border: #333;
        }

        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }

        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: var(--bg-primary);
            color: var(--text-primary);
            min-height: 100vh;
        }

        .login-container {
            display: flex;
            justify-content: center;
            align-items: center;
            min-height: 100vh;
            padding: 20px;
        }

        .login-box {
            background: var(--bg-secondary);
            padding: 40px;
            border-radius: 12px;
            width: 100%;
            max-width: 400px;
            border: 1px solid var(--border);
        }

        .login-box h1 {
            text-align: center;
            margin-bottom: 30px;
            font-size: 24px;
        }

        .form-group {
            margin-bottom: 20px;
        }

        .form-group label {
            display: block;
            margin-bottom: 8px;
            color: var(--text-secondary);
            font-size: 14px;
        }

        .form-group input, .form-group select {
            width: 100%;
            padding: 12px 16px;
            background: var(--bg-tertiary);
            border: 1px solid var(--border);
            border-radius: 8px;
            color: var(--text-primary);
            font-size: 16px;
        }

        .form-group input:focus, .form-group select:focus {
            outline: none;
            border-color: var(--accent);
        }

        .btn {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            padding: 12px 24px;
            border-radius: 8px;
            font-size: 14px;
            font-weight: 500;
            cursor: pointer;
            border: none;
            transition: all 0.2s;
        }

        .btn-primary {
            background: var(--accent);
            color: white;
            width: 100%;
        }

        .btn-primary:hover {
            background: var(--accent-hover);
        }

        .btn-secondary {
            background: var(--bg-tertiary);
            color: var(--text-primary);
            border: 1px solid var(--border);
        }

        .btn-secondary:hover {
            background: var(--bg-secondary);
        }

        .btn-danger {
            background: var(--danger);
            color: white;
        }

        .btn-danger:hover {
            background: #dc2626;
        }

        .btn-success {
            background: var(--success);
            color: white;
        }

        .btn-sm {
            padding: 6px 12px;
            font-size: 12px;
        }

        .error-message {
            background: rgba(239, 68, 68, 0.1);
            border: 1px solid var(--danger);
            color: var(--danger);
            padding: 12px;
            border-radius: 8px;
            margin-bottom: 20px;
            text-align: center;
        }

        .dashboard {
            display: none;
        }

        .header {
            background: var(--bg-secondary);
            padding: 16px 24px;
            border-bottom: 1px solid var(--border);
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .header h1 {
            font-size: 20px;
        }

        .header-actions {
            display: flex;
            gap: 12px;
            align-items: center;
        }

        .user-info {
            color: var(--text-secondary);
            font-size: 14px;
        }

        .main-content {
            padding: 24px;
            max-width: 1400px;
            margin: 0 auto;
        }

        .tabs {
            display: flex;
            gap: 8px;
            margin-bottom: 24px;
            border-bottom: 1px solid var(--border);
            padding-bottom: 16px;
        }

        .tab {
            padding: 10px 20px;
            background: transparent;
            border: none;
            color: var(--text-secondary);
            cursor: pointer;
            font-size: 14px;
            border-radius: 8px;
            transition: all 0.2s;
        }

        .tab:hover {
            background: var(--bg-tertiary);
            color: var(--text-primary);
        }

        .tab.active {
            background: var(--accent);
            color: white;
        }

        .stats-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 16px;
            margin-bottom: 32px;
        }

        .stat-card {
            background: var(--bg-secondary);
            padding: 24px;
            border-radius: 12px;
            border: 1px solid var(--border);
        }

        .stat-card.accent {
            border-color: var(--accent);
            background: linear-gradient(135deg, rgba(99, 102, 241, 0.1), transparent);
        }

        .stat-value {
            font-size: 32px;
            font-weight: 700;
            margin-bottom: 8px;
        }

        .stat-label {
            color: var(--text-secondary);
            font-size: 14px;
        }

        .panel {
            display: none;
        }

        .panel.active {
            display: block;
        }

        .table-container {
            background: var(--bg-secondary);
            border-radius: 12px;
            border: 1px solid var(--border);
            overflow: hidden;
        }

        .table-header {
            padding: 16px 20px;
            border-bottom: 1px solid var(--border);
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .table-header h2 {
            font-size: 16px;
        }

        table {
            width: 100%;
            border-collapse: collapse;
        }

        th, td {
            text-align: left;
            padding: 16px 20px;
            border-bottom: 1px solid var(--border);
        }

        th {
            color: var(--text-secondary);
            font-weight: 500;
            font-size: 12px;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }

        tr:last-child td {
            border-bottom: none;
        }

        tr:hover td {
            background: var(--bg-tertiary);
        }

        .badge {
            display: inline-block;
            padding: 4px 10px;
            border-radius: 20px;
            font-size: 12px;
            font-weight: 500;
        }

        .badge-developer {
            background: rgba(99, 102, 241, 0.2);
            color: var(--accent);
        }

        .badge-standard {
            background: rgba(34, 197, 94, 0.2);
            color: var(--success);
        }

        .badge-enterprise {
            background: rgba(245, 158, 11, 0.2);
            color: var(--warning);
        }

        .badge-access {
            background: rgba(160, 160, 160, 0.2);
            color: var(--text-secondary);
        }

        .badge-admin {
            background: rgba(239, 68, 68, 0.2);
            color: var(--danger);
        }

        .badge-active {
            background: rgba(34, 197, 94, 0.2);
            color: var(--success);
        }

        .badge-inactive {
            background: rgba(239, 68, 68, 0.2);
            color: var(--danger);
        }

        .actions {
            display: flex;
            gap: 8px;
        }

        .modal-overlay {
            display: none;
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0, 0, 0, 0.7);
            justify-content: center;
            align-items: center;
            z-index: 1000;
        }

        .modal-overlay.active {
            display: flex;
        }

        .modal {
            background: var(--bg-secondary);
            padding: 32px;
            border-radius: 12px;
            width: 100%;
            max-width: 500px;
            max-height: 90vh;
            overflow-y: auto;
            border: 1px solid var(--border);
        }

        .modal h2 {
            margin-bottom: 24px;
        }

        .modal-actions {
            display: flex;
            gap: 12px;
            justify-content: flex-end;
            margin-top: 24px;
        }

        .loading {
            text-align: center;
            padding: 40px;
            color: var(--text-secondary);
        }

        .spinner {
            width: 40px;
            height: 40px;
            border: 3px solid var(--border);
            border-top-color: var(--accent);
            border-radius: 50%;
            animation: spin 1s linear infinite;
            margin: 0 auto 16px;
        }

        @keyframes spin {
            to { transform: rotate(360deg); }
        }

        .tier-breakdown {
            display: flex;
            gap: 16px;
            flex-wrap: wrap;
        }

        .tier-item {
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .search-box {
            display: flex;
            gap: 12px;
            margin-bottom: 16px;
        }

        .search-box input {
            flex: 1;
            padding: 10px 16px;
            background: var(--bg-tertiary);
            border: 1px solid var(--border);
            border-radius: 8px;
            color: var(--text-primary);
        }

        .empty-state {
            text-align: center;
            padding: 60px 20px;
            color: var(--text-secondary);
        }

        .user-detail-panel {
            background: var(--bg-secondary);
            border-radius: 12px;
            border: 1px solid var(--border);
            padding: 24px;
            margin-bottom: 24px;
        }

        .detail-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
        }

        .detail-item label {
            display: block;
            color: var(--text-secondary);
            font-size: 12px;
            margin-bottom: 4px;
        }

        .detail-item .value {
            font-size: 16px;
            font-weight: 500;
        }
    </style>
</head>
<body>
    <!-- Login Screen -->
    <div id="loginScreen" class="login-container">
        <div class="login-box">
            <h1>Vaak Admin</h1>
            <div id="loginError" class="error-message" style="display: none;"></div>
            <form id="loginForm">
                <div class="form-group">
                    <label for="email">Email</label>
                    <input type="email" id="email" required autocomplete="email">
                </div>
                <div class="form-group">
                    <label for="password">Password</label>
                    <input type="password" id="password" required autocomplete="current-password">
                </div>
                <button type="submit" class="btn btn-primary">Sign In</button>
            </form>
        </div>
    </div>

    <!-- Dashboard -->
    <div id="dashboard" class="dashboard">
        <header class="header">
            <h1>Vaak Admin Dashboard</h1>
            <div class="header-actions">
                <span class="user-info" id="currentUserEmail"></span>
                <button class="btn btn-secondary btn-sm" onclick="logout()">Logout</button>
            </div>
        </header>

        <main class="main-content">
            <div class="tabs">
                <button class="tab active" data-panel="overview">Overview</button>
                <button class="tab" data-panel="users">Users</button>
            </div>

            <!-- Overview Panel -->
            <div id="overview" class="panel active">
                <div style="display: flex; justify-content: flex-end; margin-bottom: 16px; gap: 12px;">
                    <button class="btn btn-danger" onclick="resetAllStats()">Reset All User Stats</button>
                </div>
                <div class="stats-grid" id="globalStats">
                    <div class="loading">
                        <div class="spinner"></div>
                        Loading statistics...
                    </div>
                </div>
            </div>

            <!-- Users Panel -->
            <div id="users" class="panel">
                <div class="search-box">
                    <input type="text" id="userSearch" placeholder="Search users by email...">
                    <button class="btn btn-primary" onclick="searchUsers()">Search</button>
                    <button class="btn btn-secondary" onclick="loadUsers()">Refresh</button>
                </div>
                <div class="table-container">
                    <div class="table-header">
                        <h2>All Users</h2>
                        <span id="userCount"></span>
                    </div>
                    <div id="usersTable">
                        <div class="loading">
                            <div class="spinner"></div>
                            Loading users...
                        </div>
                    </div>
                </div>
            </div>
        </main>
    </div>

    <!-- Edit User Modal -->
    <div id="editUserModal" class="modal-overlay">
        <div class="modal">
            <h2>Edit User</h2>
            <form id="editUserForm">
                <input type="hidden" id="editUserId">
                <div class="form-group">
                    <label for="editEmail">Email</label>
                    <input type="email" id="editEmail" required>
                </div>
                <div class="form-group">
                    <label for="editFullName">Full Name</label>
                    <input type="text" id="editFullName">
                </div>
                <div class="form-group">
                    <label for="editTier">Tier</label>
                    <select id="editTier">
                        <option value="developer">Developer (Unlimited)</option>
                        <option value="standard">Standard</option>
                        <option value="enterprise">Enterprise</option>
                        <option value="access">Access (Accessibility)</option>
                    </select>
                </div>
                <div class="form-group">
                    <label for="editLimit">Daily Limit (0 = unlimited)</label>
                    <input type="number" id="editLimit" min="0">
                </div>
                <div class="form-group">
                    <label>
                        <input type="checkbox" id="editIsAdmin"> Is Admin
                    </label>
                </div>
                <div class="form-group">
                    <label>
                        <input type="checkbox" id="editIsActive"> Is Active
                    </label>
                </div>
                <div class="form-group">
                    <label for="editPassword">New Password (leave empty to keep current)</label>
                    <input type="password" id="editPassword" minlength="6">
                </div>
                <div class="modal-actions">
                    <button type="button" class="btn btn-secondary" onclick="closeModal('editUserModal')">Cancel</button>
                    <button type="submit" class="btn btn-primary">Save Changes</button>
                </div>
            </form>
        </div>
    </div>

    <!-- User Detail Modal -->
    <div id="userDetailModal" class="modal-overlay">
        <div class="modal" style="max-width: 700px;">
            <h2>User Details</h2>
            <div id="userDetailContent">
                <div class="loading">
                    <div class="spinner"></div>
                    Loading...
                </div>
            </div>
            <div class="modal-actions">
                <button type="button" class="btn btn-secondary" onclick="closeModal('userDetailModal')">Close</button>
            </div>
        </div>
    </div>

    <script>
        const API_BASE = window.location.origin + '/api/v1';
        let authToken = localStorage.getItem('admin_token');
        let currentUser = null;

        // XSS protection: escape user-provided strings before innerHTML injection
        function escapeHtml(str) {
            if (!str) return '';
            const div = document.createElement('div');
            div.textContent = str;
            return div.innerHTML;
        }

        // Check if already logged in
        if (authToken) {
            checkAuth();
        }

        // Tab switching
        document.querySelectorAll('.tab').forEach(tab => {
            tab.addEventListener('click', () => {
                document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
                document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
                tab.classList.add('active');
                document.getElementById(tab.dataset.panel).classList.add('active');
            });
        });

        // Login form
        document.getElementById('loginForm').addEventListener('submit', async (e) => {
            e.preventDefault();
            const email = document.getElementById('email').value;
            const password = document.getElementById('password').value;

            try {
                const response = await fetch(`${API_BASE}/auth/login`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ email, password })
                });

                if (!response.ok) {
                    const error = await response.json();
                    throw new Error(error.detail || 'Login failed');
                }

                const data = await response.json();
                authToken = data.access_token;
                localStorage.setItem('admin_token', authToken);
                await checkAuth();
            } catch (err) {
                document.getElementById('loginError').textContent = err.message;
                document.getElementById('loginError').style.display = 'block';
            }
        });

        async function checkAuth() {
            try {
                // Get current user
                const response = await fetch(`${API_BASE}/auth/me`, {
                    headers: { 'Authorization': `Bearer ${authToken}` }
                });

                if (!response.ok) {
                    throw new Error('Not authenticated');
                }

                currentUser = await response.json();

                // Check if admin by trying to access admin endpoint
                const adminCheck = await fetch(`${API_BASE}/admin/stats`, {
                    headers: { 'Authorization': `Bearer ${authToken}` }
                });

                if (!adminCheck.ok) {
                    throw new Error('Admin access required');
                }

                // Show dashboard
                document.getElementById('loginScreen').style.display = 'none';
                document.getElementById('dashboard').style.display = 'block';
                document.getElementById('currentUserEmail').textContent = currentUser.email;

                // Load data
                loadGlobalStats();
                loadUsers();
            } catch (err) {
                localStorage.removeItem('admin_token');
                authToken = null;
                document.getElementById('loginError').textContent = err.message;
                document.getElementById('loginError').style.display = 'block';
                document.getElementById('loginScreen').style.display = 'flex';
                document.getElementById('dashboard').style.display = 'none';
            }
        }

        function logout() {
            localStorage.removeItem('admin_token');
            authToken = null;
            currentUser = null;
            document.getElementById('loginScreen').style.display = 'flex';
            document.getElementById('dashboard').style.display = 'none';
            document.getElementById('loginError').style.display = 'none';
        }

        async function loadGlobalStats() {
            try {
                const response = await fetch(`${API_BASE}/admin/stats`, {
                    headers: { 'Authorization': `Bearer ${authToken}` }
                });
                const stats = await response.json();

                document.getElementById('globalStats').innerHTML = `
                    <div class="stat-card accent">
                        <div class="stat-value">${stats.total_users}</div>
                        <div class="stat-label">Total Users</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-value">${stats.active_users}</div>
                        <div class="stat-label">Active Users</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-value">${stats.total_transcriptions.toLocaleString()}</div>
                        <div class="stat-label">Total Transcriptions</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-value">${stats.total_words.toLocaleString()}</div>
                        <div class="stat-label">Total Words</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-value">${stats.total_audio_hours.toFixed(1)}h</div>
                        <div class="stat-label">Audio Transcribed</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-value">${stats.transcriptions_today}</div>
                        <div class="stat-label">Transcriptions Today</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-value">${stats.transcriptions_this_week}</div>
                        <div class="stat-label">This Week</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">Users by Tier</div>
                        <div class="tier-breakdown">
                            ${Object.entries(stats.users_by_tier).map(([tier, count]) => `
                                <div class="tier-item">
                                    <span class="badge badge-${escapeHtml(tier)}">${escapeHtml(tier)}</span>
                                    <span>${count}</span>
                                </div>
                            `).join('')}
                        </div>
                    </div>
                `;
            } catch (err) {
                console.error('Failed to load stats:', err);
            }
        }

        let allUsers = [];

        async function loadUsers() {
            try {
                const response = await fetch(`${API_BASE}/admin/users?limit=100`, {
                    headers: { 'Authorization': `Bearer ${authToken}` }
                });
                allUsers = await response.json();
                renderUsers(allUsers);
            } catch (err) {
                console.error('Failed to load users:', err);
            }
        }

        function searchUsers() {
            const query = document.getElementById('userSearch').value.toLowerCase();
            const filtered = allUsers.filter(u =>
                u.email.toLowerCase().includes(query) ||
                (u.full_name && u.full_name.toLowerCase().includes(query))
            );
            renderUsers(filtered);
        }

        function renderUsers(users) {
            document.getElementById('userCount').textContent = `${users.length} users`;

            if (users.length === 0) {
                document.getElementById('usersTable').innerHTML = `
                    <div class="empty-state">No users found</div>
                `;
                return;
            }

            document.getElementById('usersTable').innerHTML = `
                <table>
                    <thead>
                        <tr>
                            <th>Email</th>
                            <th>Name</th>
                            <th>Tier</th>
                            <th>Status</th>
                            <th>Transcriptions</th>
                            <th>Words</th>
                            <th>Joined</th>
                            <th>Actions</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${users.map(user => `
                            <tr>
                                <td>
                                    ${escapeHtml(user.email)}
                                    ${user.is_admin ? '<span class="badge badge-admin">Admin</span>' : ''}
                                </td>
                                <td>${escapeHtml(user.full_name) || '-'}</td>
                                <td><span class="badge badge-${escapeHtml(user.tier)}">${escapeHtml(user.tier)}</span></td>
                                <td><span class="badge badge-${user.is_active ? 'active' : 'inactive'}">${user.is_active ? 'Active' : 'Inactive'}</span></td>
                                <td>${user.total_transcriptions.toLocaleString()}</td>
                                <td>${user.total_words.toLocaleString()}</td>
                                <td>${new Date(user.created_at).toLocaleDateString()}</td>
                                <td class="actions">
                                    <button class="btn btn-secondary btn-sm" onclick="viewUser(${user.id})">View</button>
                                    <button class="btn btn-secondary btn-sm" onclick="editUser(${user.id})">Edit</button>
                                </td>
                            </tr>
                        `).join('')}
                    </tbody>
                </table>
            `;
        }

        async function viewUser(userId) {
            document.getElementById('userDetailModal').classList.add('active');
            document.getElementById('userDetailContent').innerHTML = `
                <div class="loading"><div class="spinner"></div>Loading...</div>
            `;

            try {
                const [userRes, statsRes, transcriptsRes] = await Promise.all([
                    fetch(`${API_BASE}/admin/users/${userId}`, {
                        headers: { 'Authorization': `Bearer ${authToken}` }
                    }),
                    fetch(`${API_BASE}/admin/users/${userId}/stats`, {
                        headers: { 'Authorization': `Bearer ${authToken}` }
                    }),
                    fetch(`${API_BASE}/admin/users/${userId}/transcripts?limit=10`, {
                        headers: { 'Authorization': `Bearer ${authToken}` }
                    })
                ]);

                const user = await userRes.json();
                const stats = await statsRes.json();
                const transcripts = await transcriptsRes.json();

                document.getElementById('userDetailContent').innerHTML = `
                    <div class="user-detail-panel">
                        <h3 style="margin-bottom: 16px;">${escapeHtml(user.email)}</h3>
                        <div class="detail-grid">
                            <div class="detail-item">
                                <label>Full Name</label>
                                <div class="value">${escapeHtml(user.full_name) || '-'}</div>
                            </div>
                            <div class="detail-item">
                                <label>Tier</label>
                                <div class="value"><span class="badge badge-${escapeHtml(user.tier)}">${escapeHtml(user.tier)}</span></div>
                            </div>
                            <div class="detail-item">
                                <label>Status</label>
                                <div class="value">
                                    <span class="badge badge-${user.is_active ? 'active' : 'inactive'}">${user.is_active ? 'Active' : 'Inactive'}</span>
                                    ${user.is_admin ? '<span class="badge badge-admin">Admin</span>' : ''}
                                </div>
                            </div>
                            <div class="detail-item">
                                <label>Daily Limit</label>
                                <div class="value">${user.daily_transcription_limit === 0 ? 'Unlimited' : user.daily_transcription_limit}</div>
                            </div>
                            <div class="detail-item">
                                <label>Used Today</label>
                                <div class="value">${user.daily_transcriptions_used}</div>
                            </div>
                            <div class="detail-item">
                                <label>Joined</label>
                                <div class="value">${new Date(user.created_at).toLocaleString()}</div>
                            </div>
                        </div>
                    </div>

                    <div class="user-detail-panel">
                        <h3 style="margin-bottom: 16px;">Usage Statistics</h3>
                        <div class="detail-grid">
                            <div class="detail-item">
                                <label>Total Transcriptions</label>
                                <div class="value">${stats.total_transcriptions.toLocaleString()}</div>
                            </div>
                            <div class="detail-item">
                                <label>Total Words</label>
                                <div class="value">${stats.total_words.toLocaleString()}</div>
                            </div>
                            <div class="detail-item">
                                <label>Audio Transcribed</label>
                                <div class="value">${formatDuration(stats.total_audio_seconds)}</div>
                            </div>
                            <div class="detail-item">
                                <label>Avg WPM</label>
                                <div class="value">${stats.average_words_per_minute}</div>
                            </div>
                            <div class="detail-item">
                                <label>Today</label>
                                <div class="value">${stats.transcriptions_today} transcriptions / ${stats.words_today.toLocaleString()} words</div>
                            </div>
                            <div class="detail-item">
                                <label>This Week</label>
                                <div class="value">${stats.transcriptions_this_week} transcriptions / ${stats.words_this_week.toLocaleString()} words</div>
                            </div>
                            <div class="detail-item">
                                <label>This Month</label>
                                <div class="value">${stats.transcriptions_this_month} transcriptions / ${stats.words_this_month.toLocaleString()} words</div>
                            </div>
                        </div>
                    </div>

                    <div class="user-detail-panel">
                        <h3 style="margin-bottom: 16px;">Recent Transcripts</h3>
                        ${transcripts.length === 0 ? '<p>No transcripts yet</p>' : `
                            <table>
                                <thead>
                                    <tr>
                                        <th>Date</th>
                                        <th>Preview</th>
                                        <th>Words</th>
                                        <th>Duration</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    ${transcripts.map(t => `
                                        <tr>
                                            <td>${new Date(t.created_at).toLocaleString()}</td>
                                            <td>${escapeHtml(t.polished_text.slice(0, 60))}${t.polished_text.length > 60 ? '...' : ''}</td>
                                            <td>${t.word_count}</td>
                                            <td>${t.audio_duration_seconds.toFixed(1)}s</td>
                                        </tr>
                                    `).join('')}
                                </tbody>
                            </table>
                        `}
                    </div>
                `;
            } catch (err) {
                document.getElementById('userDetailContent').innerHTML = `
                    <div class="error-message">Failed to load user details: ${escapeHtml(err.message)}</div>
                `;
            }
        }

        function formatDuration(seconds) {
            const hours = Math.floor(seconds / 3600);
            const mins = Math.floor((seconds % 3600) / 60);
            const secs = Math.floor(seconds % 60);
            if (hours > 0) return `${hours}h ${mins}m`;
            if (mins > 0) return `${mins}m ${secs}s`;
            return `${secs}s`;
        }

        async function editUser(userId) {
            try {
                const response = await fetch(`${API_BASE}/admin/users/${userId}`, {
                    headers: { 'Authorization': `Bearer ${authToken}` }
                });
                const user = await response.json();

                document.getElementById('editUserId').value = user.id;
                document.getElementById('editEmail').value = user.email;
                document.getElementById('editFullName').value = user.full_name || '';
                document.getElementById('editTier').value = user.tier;
                document.getElementById('editLimit').value = user.daily_transcription_limit;
                document.getElementById('editIsAdmin').checked = user.is_admin;
                document.getElementById('editIsActive').checked = user.is_active;
                document.getElementById('editPassword').value = '';

                document.getElementById('editUserModal').classList.add('active');
            } catch (err) {
                alert('Failed to load user: ' + err.message);
            }
        }

        document.getElementById('editUserForm').addEventListener('submit', async (e) => {
            e.preventDefault();

            const userId = document.getElementById('editUserId').value;
            const data = {
                email: document.getElementById('editEmail').value,
                full_name: document.getElementById('editFullName').value || null,
                tier: document.getElementById('editTier').value,
                daily_transcription_limit: parseInt(document.getElementById('editLimit').value),
                is_admin: document.getElementById('editIsAdmin').checked,
                is_active: document.getElementById('editIsActive').checked,
            };

            const password = document.getElementById('editPassword').value;
            if (password) {
                data.password = password;
            }

            try {
                const response = await fetch(`${API_BASE}/admin/users/${userId}`, {
                    method: 'PATCH',
                    headers: {
                        'Authorization': `Bearer ${authToken}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify(data)
                });

                if (!response.ok) {
                    const error = await response.json();
                    throw new Error(error.detail || 'Update failed');
                }

                closeModal('editUserModal');
                loadUsers();
                loadGlobalStats();
                alert('User updated successfully');
            } catch (err) {
                alert('Failed to update user: ' + err.message);
            }
        });

        function closeModal(modalId) {
            document.getElementById(modalId).classList.remove('active');
        }

        // Close modals on overlay click
        document.querySelectorAll('.modal-overlay').forEach(overlay => {
            overlay.addEventListener('click', (e) => {
                if (e.target === overlay) {
                    overlay.classList.remove('active');
                }
            });
        });

        // Search on enter
        document.getElementById('userSearch').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') searchUsers();
        });

        // Reset all user stats
        async function resetAllStats() {
            if (!confirm('Are you sure you want to reset stats for ALL users?\\n\\nThis will recalculate stats from actual transcript history.\\nTranscript history will be preserved.')) {
                return;
            }

            try {
                const response = await fetch(`${API_BASE}/admin/reset-all-stats`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${authToken}` }
                });

                if (!response.ok) {
                    const error = await response.json();
                    throw new Error(error.detail || 'Reset failed');
                }

                const result = await response.json();
                alert(result.message);
                loadGlobalStats();
                loadUsers();
            } catch (err) {
                alert('Failed to reset stats: ' + err.message);
            }
        }
    </script>
</body>
</html>
'''


@router.get("/dashboard", response_class=HTMLResponse)
async def admin_dashboard():
    """Serve the admin dashboard HTML page."""
    return ADMIN_DASHBOARD_HTML
