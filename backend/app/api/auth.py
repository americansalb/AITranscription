import logging
from datetime import datetime, timedelta
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, Query, status
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer
from pydantic import BaseModel, EmailStr, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.database import get_db
from app.models.transcript import Transcript
from app.models.user import SubscriptionTier, User
from app.services.auth import (
    authenticate_user,
    create_access_token,
    create_user,
    decode_access_token,
    get_user_by_email,
    get_user_by_id,
)

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/auth", tags=["auth"])
security = HTTPBearer()


# Request/Response schemas
class SignupRequest(BaseModel):
    """Request body for user signup."""

    email: EmailStr
    password: str = Field(min_length=8, max_length=72, description="Password must be 8-72 characters")
    full_name: str | None = None


class LoginRequest(BaseModel):
    """Request body for user login."""

    email: EmailStr
    password: str


class TokenResponse(BaseModel):
    """Response containing access token."""

    access_token: str
    token_type: str = "bearer"


class UserResponse(BaseModel):
    """Response containing user information."""

    id: int
    email: str
    full_name: str | None
    tier: SubscriptionTier
    is_active: bool
    accessibility_verified: bool


# Dependency to get current user
async def get_current_user(
    credentials: HTTPAuthorizationCredentials = Depends(security),
    db: AsyncSession = Depends(get_db),
) -> User:
    """Dependency that extracts and validates the current user from JWT token."""
    token = credentials.credentials
    user_id = decode_access_token(token)

    if user_id is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid or expired token",
            headers={"WWW-Authenticate": "Bearer"},
        )

    user = await get_user_by_id(db, user_id)
    if user is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="User not found",
            headers={"WWW-Authenticate": "Bearer"},
        )

    if not user.is_active:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="User account is inactive",
        )

    return user


# Optional user dependency (for endpoints that work with or without auth)
async def get_optional_user(
    credentials: HTTPAuthorizationCredentials | None = Depends(
        HTTPBearer(auto_error=False)
    ),
    db: AsyncSession = Depends(get_db),
) -> User | None:
    """Optional dependency that returns user if authenticated, None otherwise."""
    if credentials is None:
        return None

    try:
        return await get_current_user(credentials, db)
    except HTTPException:
        return None


# Routes
@router.post("/signup", response_model=TokenResponse, status_code=status.HTTP_201_CREATED)
async def signup(request: SignupRequest, db: AsyncSession = Depends(get_db)):
    """Create a new user account."""
    try:
        logger.info(f"Signup attempt for email: {request.email}")

        # Check if email already exists
        existing_user = await get_user_by_email(db, request.email)
        if existing_user:
            logger.info(f"Email already registered: {request.email}")
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Email already registered",
            )

        # Create user
        logger.info(f"Creating user: {request.email}")
        user = await create_user(
            db,
            email=request.email,
            password=request.password,
            full_name=request.full_name,
        )
        logger.info(f"User created successfully: {user.id}")

        # Generate token
        access_token = create_access_token(user.id)
        logger.info(f"Token generated for user: {user.id}")

        return TokenResponse(access_token=access_token)
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Signup error: {type(e).__name__}: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Signup failed: {str(e)}",
        )


@router.post("/login", response_model=TokenResponse)
async def login(request: LoginRequest, db: AsyncSession = Depends(get_db)):
    """Authenticate and receive an access token."""
    user = await authenticate_user(db, request.email, request.password)

    if not user:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid email or password",
            headers={"WWW-Authenticate": "Bearer"},
        )

    if not user.is_active:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="User account is inactive",
        )

    access_token = create_access_token(user.id)

    return TokenResponse(access_token=access_token)


@router.get("/me", response_model=UserResponse)
async def get_me(current_user: User = Depends(get_current_user)):
    """Get the current authenticated user's information."""
    return UserResponse(
        id=current_user.id,
        email=current_user.email,
        full_name=current_user.full_name,
        tier=current_user.tier,
        is_active=current_user.is_active,
        accessibility_verified=current_user.accessibility_verified,
    )


@router.post("/refresh", response_model=TokenResponse)
async def refresh_token(current_user: User = Depends(get_current_user)):
    """Refresh the access token."""
    access_token = create_access_token(current_user.id)
    return TokenResponse(access_token=access_token)


# Transcript history schemas
class TranscriptItem(BaseModel):
    """Single transcript item."""

    id: int
    raw_text: str
    polished_text: str
    word_count: int
    audio_duration_seconds: float
    words_per_minute: float
    context: str | None
    created_at: datetime


class UserStatsResponse(BaseModel):
    """User's own statistics."""

    total_transcriptions: int
    total_words: int
    total_audio_seconds: int
    transcriptions_today: int
    words_today: int
    average_words_per_transcription: float
    average_words_per_minute: float


@router.get("/transcripts", response_model=list[TranscriptItem])
async def get_my_transcripts(
    skip: int = Query(0, ge=0),
    limit: int = Query(50, ge=1, le=100),
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    """Get the current user's transcript history."""
    query = (
        select(Transcript)
        .where(Transcript.user_id == current_user.id)
        .order_by(Transcript.created_at.desc())
        .offset(skip)
        .limit(limit)
    )
    result = await db.execute(query)
    transcripts = result.scalars().all()

    return [
        TranscriptItem(
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


@router.get("/stats", response_model=UserStatsResponse)
async def get_my_stats(
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    """Get the current user's statistics."""
    now = datetime.utcnow()
    today = now.replace(hour=0, minute=0, second=0, microsecond=0)

    # Get today's transcripts
    query = select(Transcript).where(
        Transcript.user_id == current_user.id,
        Transcript.created_at >= today,
    )
    result = await db.execute(query)
    today_transcripts = result.scalars().all()

    transcriptions_today = len(today_transcripts)
    words_today = sum(t.word_count for t in today_transcripts)

    # Calculate averages
    avg_words = (
        current_user.total_words / current_user.total_transcriptions
        if current_user.total_transcriptions > 0
        else 0
    )

    # Get average WPM from all transcripts
    all_transcripts_query = select(Transcript).where(
        Transcript.user_id == current_user.id
    )
    result = await db.execute(all_transcripts_query)
    all_transcripts = result.scalars().all()
    total_wpm = sum(t.words_per_minute for t in all_transcripts)
    avg_wpm = total_wpm / len(all_transcripts) if all_transcripts else 0

    return UserStatsResponse(
        total_transcriptions=current_user.total_transcriptions,
        total_words=current_user.total_words,
        total_audio_seconds=current_user.total_audio_seconds,
        transcriptions_today=transcriptions_today,
        words_today=words_today,
        average_words_per_transcription=round(avg_words, 1),
        average_words_per_minute=round(avg_wpm, 1),
    )
