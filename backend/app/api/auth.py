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
    password: str = Field(min_length=6, max_length=72, description="Password must be 6-72 characters")
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


class ContextStats(BaseModel):
    """Statistics for a specific context."""
    context: str
    count: int
    words: int
    percentage: float


class DailyStats(BaseModel):
    """Daily activity statistics."""
    date: str
    transcriptions: int
    words: int


class HourlyStats(BaseModel):
    """Hourly activity for heatmap."""
    hour: int  # 0-23
    transcriptions: int
    words: int


class DayOfWeekStats(BaseModel):
    """Day of week breakdown."""
    day: str  # Monday, Tuesday, etc.
    day_index: int  # 0-6
    transcriptions: int
    words: int
    percentage: float


class MonthlyStats(BaseModel):
    """Monthly trends."""
    month: str  # YYYY-MM
    month_label: str  # Jan, Feb, etc.
    transcriptions: int
    words: int
    audio_minutes: float


class FormalityStats(BaseModel):
    """Formality level breakdown."""
    formality: str
    count: int
    words: int
    percentage: float


class WordLengthDistribution(BaseModel):
    """Distribution of transcription lengths."""
    range_label: str  # e.g., "1-25", "26-50", etc.
    min_words: int
    max_words: int
    count: int
    percentage: float


class Achievement(BaseModel):
    """User achievement/milestone."""
    id: str
    name: str
    description: str
    icon: str
    earned: bool
    earned_at: datetime | None = None
    progress: float  # 0-100
    target: int | None = None
    current: int | None = None


class GrowthMetrics(BaseModel):
    """Week over week and month over month growth."""
    words_wow_change: float  # Week over week percentage change
    words_mom_change: float  # Month over month percentage change
    transcriptions_wow_change: float
    transcriptions_mom_change: float
    last_week_words: int
    prev_week_words: int
    last_month_words: int
    prev_month_words: int


class ProductivityInsights(BaseModel):
    """Productivity analysis."""
    peak_hour: int  # Most productive hour (0-23)
    peak_hour_label: str  # e.g., "2:00 PM"
    peak_day: str  # Most productive day of week
    avg_session_words: float  # Average words per session
    avg_session_duration_seconds: float
    busiest_week_ever: str | None  # YYYY-Www
    busiest_week_words: int
    efficiency_score: float  # Words per minute of audio (higher = more efficient speech)


class DetailedStatsResponse(BaseModel):
    """Detailed user statistics with insights."""
    # Totals
    total_transcriptions: int
    total_words: int
    total_audio_seconds: float
    total_characters: int

    # Time-based
    transcriptions_today: int
    words_today: int
    transcriptions_this_week: int
    words_this_week: int
    transcriptions_this_month: int
    words_this_month: int

    # Averages
    average_words_per_transcription: float
    average_words_per_minute: float
    average_transcriptions_per_day: float
    average_audio_duration_seconds: float

    # Time saved (assuming 40 WPM typing vs ~150 WPM speaking)
    estimated_time_saved_minutes: float

    # Context breakdown
    context_breakdown: list[ContextStats]

    # Formality breakdown
    formality_breakdown: list[FormalityStats]

    # Daily activity (last 7 days)
    daily_activity: list[DailyStats]

    # Hourly activity (24 hours aggregate)
    hourly_activity: list[HourlyStats]

    # Day of week breakdown
    day_of_week_breakdown: list[DayOfWeekStats]

    # Monthly trends (last 12 months)
    monthly_trends: list[MonthlyStats]

    # Word length distribution
    word_length_distribution: list[WordLengthDistribution]

    # Streaks
    current_streak_days: int
    longest_streak_days: int

    # Records
    most_productive_day: str | None
    most_productive_day_words: int
    longest_transcription_words: int
    shortest_transcription_words: int
    fastest_wpm: float
    slowest_wpm: float

    # Growth metrics
    growth: GrowthMetrics

    # Productivity insights
    productivity: ProductivityInsights

    # Achievements
    achievements: list[Achievement]

    # Member since
    member_since: datetime
    days_as_member: int
    total_active_days: int


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


@router.get("/stats/detailed", response_model=DetailedStatsResponse)
async def get_detailed_stats(
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
):
    """Get extremely detailed statistics with deep insights for the current user."""
    from collections import defaultdict
    from datetime import timezone
    import calendar

    now = datetime.now(timezone.utc)
    today = now.replace(hour=0, minute=0, second=0, microsecond=0)
    week_ago = today - timedelta(days=7)
    two_weeks_ago = today - timedelta(days=14)
    month_ago = today - timedelta(days=30)
    two_months_ago = today - timedelta(days=60)
    year_ago = today - timedelta(days=365)

    # Get all user transcripts
    all_transcripts_query = select(Transcript).where(
        Transcript.user_id == current_user.id
    ).order_by(Transcript.created_at.desc())
    result = await db.execute(all_transcripts_query)
    all_transcripts = result.scalars().all()

    # Basic calculations
    total_characters = sum(t.character_count for t in all_transcripts)
    total_audio = sum(t.audio_duration_seconds or 0 for t in all_transcripts)
    total_wpm_sum = sum(t.words_per_minute for t in all_transcripts if t.words_per_minute)
    avg_wpm = total_wpm_sum / len(all_transcripts) if all_transcripts else 0
    avg_words = (
        current_user.total_words / current_user.total_transcriptions
        if current_user.total_transcriptions > 0
        else 0
    )
    avg_audio_duration = total_audio / len(all_transcripts) if all_transcripts else 0

    # Time-based filtering
    today_transcripts = [t for t in all_transcripts if t.created_at >= today]
    week_transcripts = [t for t in all_transcripts if t.created_at >= week_ago]
    prev_week_transcripts = [t for t in all_transcripts if two_weeks_ago <= t.created_at < week_ago]
    month_transcripts = [t for t in all_transcripts if t.created_at >= month_ago]
    prev_month_transcripts = [t for t in all_transcripts if two_months_ago <= t.created_at < month_ago]

    transcriptions_today = len(today_transcripts)
    words_today = sum(t.word_count for t in today_transcripts)
    transcriptions_week = len(week_transcripts)
    words_week = sum(t.word_count for t in week_transcripts)
    transcriptions_month = len(month_transcripts)
    words_month = sum(t.word_count for t in month_transcripts)

    # Previous period stats for growth
    prev_week_words = sum(t.word_count for t in prev_week_transcripts)
    prev_month_words = sum(t.word_count for t in prev_month_transcripts)
    prev_week_count = len(prev_week_transcripts)
    prev_month_count = len(prev_month_transcripts)

    # Growth calculations
    words_wow_change = ((words_week - prev_week_words) / prev_week_words * 100) if prev_week_words > 0 else 0
    words_mom_change = ((words_month - prev_month_words) / prev_month_words * 100) if prev_month_words > 0 else 0
    trans_wow_change = ((transcriptions_week - prev_week_count) / prev_week_count * 100) if prev_week_count > 0 else 0
    trans_mom_change = ((transcriptions_month - prev_month_count) / prev_month_count * 100) if prev_month_count > 0 else 0

    growth = GrowthMetrics(
        words_wow_change=round(words_wow_change, 1),
        words_mom_change=round(words_mom_change, 1),
        transcriptions_wow_change=round(trans_wow_change, 1),
        transcriptions_mom_change=round(trans_mom_change, 1),
        last_week_words=words_week,
        prev_week_words=prev_week_words,
        last_month_words=words_month,
        prev_month_words=prev_month_words,
    )

    # Time saved calculation
    typing_time_minutes = current_user.total_words / 40 if current_user.total_words > 0 else 0
    speaking_time_minutes = current_user.total_audio_seconds / 60
    time_saved = max(0, typing_time_minutes - speaking_time_minutes)

    # Context breakdown
    context_counts: dict[str, dict] = defaultdict(lambda: {"count": 0, "words": 0})
    for t in all_transcripts:
        ctx = t.context or "general"
        context_counts[ctx]["count"] += 1
        context_counts[ctx]["words"] += t.word_count

    total_count = len(all_transcripts) or 1
    context_breakdown = [
        ContextStats(
            context=ctx,
            count=data["count"],
            words=data["words"],
            percentage=round(data["count"] / total_count * 100, 1),
        )
        for ctx, data in sorted(context_counts.items(), key=lambda x: -x[1]["count"])
    ]

    # Formality breakdown
    formality_counts: dict[str, dict] = defaultdict(lambda: {"count": 0, "words": 0})
    for t in all_transcripts:
        formality = t.formality or "neutral"
        formality_counts[formality]["count"] += 1
        formality_counts[formality]["words"] += t.word_count

    formality_breakdown = [
        FormalityStats(
            formality=f,
            count=data["count"],
            words=data["words"],
            percentage=round(data["count"] / total_count * 100, 1),
        )
        for f, data in sorted(formality_counts.items(), key=lambda x: -x[1]["count"])
    ]

    # Daily activity (last 7 days)
    daily_counts: dict[str, dict] = defaultdict(lambda: {"transcriptions": 0, "words": 0})
    for i in range(7):
        day = today - timedelta(days=i)
        day_str = day.strftime("%Y-%m-%d")
        daily_counts[day_str] = {"transcriptions": 0, "words": 0}

    for t in week_transcripts:
        day_str = t.created_at.strftime("%Y-%m-%d")
        if day_str in daily_counts:
            daily_counts[day_str]["transcriptions"] += 1
            daily_counts[day_str]["words"] += t.word_count

    daily_activity = [
        DailyStats(
            date=date,
            transcriptions=data["transcriptions"],
            words=data["words"],
        )
        for date, data in sorted(daily_counts.items())
    ]

    # Hourly activity (24 hours aggregate - all time)
    hourly_counts: dict[int, dict] = {h: {"transcriptions": 0, "words": 0} for h in range(24)}
    for t in all_transcripts:
        hour = t.created_at.hour
        hourly_counts[hour]["transcriptions"] += 1
        hourly_counts[hour]["words"] += t.word_count

    hourly_activity = [
        HourlyStats(
            hour=h,
            transcriptions=data["transcriptions"],
            words=data["words"],
        )
        for h, data in sorted(hourly_counts.items())
    ]

    # Day of week breakdown
    day_names = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]
    dow_counts: dict[int, dict] = {i: {"transcriptions": 0, "words": 0} for i in range(7)}
    for t in all_transcripts:
        dow = t.created_at.weekday()
        dow_counts[dow]["transcriptions"] += 1
        dow_counts[dow]["words"] += t.word_count

    day_of_week_breakdown = [
        DayOfWeekStats(
            day=day_names[i],
            day_index=i,
            transcriptions=data["transcriptions"],
            words=data["words"],
            percentage=round(data["transcriptions"] / total_count * 100, 1) if total_count > 0 else 0,
        )
        for i, data in sorted(dow_counts.items())
    ]

    # Monthly trends (last 12 months)
    monthly_counts: dict[str, dict] = {}
    for i in range(12):
        month_date = today - timedelta(days=30 * i)
        month_key = month_date.strftime("%Y-%m")
        month_label = month_date.strftime("%b")
        monthly_counts[month_key] = {"transcriptions": 0, "words": 0, "audio": 0.0, "label": month_label}

    for t in all_transcripts:
        month_key = t.created_at.strftime("%Y-%m")
        if month_key in monthly_counts:
            monthly_counts[month_key]["transcriptions"] += 1
            monthly_counts[month_key]["words"] += t.word_count
            monthly_counts[month_key]["audio"] += (t.audio_duration_seconds or 0) / 60

    monthly_trends = [
        MonthlyStats(
            month=month,
            month_label=data["label"],
            transcriptions=data["transcriptions"],
            words=data["words"],
            audio_minutes=round(data["audio"], 1),
        )
        for month, data in sorted(monthly_counts.items())
    ]

    # Word length distribution
    length_ranges = [
        (1, 25, "1-25"),
        (26, 50, "26-50"),
        (51, 100, "51-100"),
        (101, 200, "101-200"),
        (201, 500, "201-500"),
        (501, 1000, "501-1000"),
        (1001, float('inf'), "1000+"),
    ]
    length_dist: dict[str, int] = {r[2]: 0 for r in length_ranges}
    for t in all_transcripts:
        for min_w, max_w, label in length_ranges:
            if min_w <= t.word_count <= max_w:
                length_dist[label] += 1
                break

    word_length_distribution = [
        WordLengthDistribution(
            range_label=label,
            min_words=r[0],
            max_words=int(r[1]) if r[1] != float('inf') else 99999,
            count=length_dist[label],
            percentage=round(length_dist[label] / total_count * 100, 1) if total_count > 0 else 0,
        )
        for r in length_ranges
        for label in [r[2]]
    ]

    # Streaks calculation
    unique_days = set()
    for t in all_transcripts:
        unique_days.add(t.created_at.date())

    current_streak = 0
    check_date = today.date()
    while check_date in unique_days:
        current_streak += 1
        check_date -= timedelta(days=1)

    if unique_days:
        sorted_days = sorted(unique_days)
        longest_streak = 1
        current_run = 1
        for i in range(1, len(sorted_days)):
            if (sorted_days[i] - sorted_days[i - 1]).days == 1:
                current_run += 1
                longest_streak = max(longest_streak, current_run)
            else:
                current_run = 1
    else:
        longest_streak = 0

    # Most productive day
    words_by_day: dict[str, int] = defaultdict(int)
    for t in all_transcripts:
        day_str = t.created_at.strftime("%Y-%m-%d")
        words_by_day[day_str] += t.word_count

    most_productive_day = None
    most_productive_day_words = 0
    if words_by_day:
        most_productive_day = max(words_by_day, key=words_by_day.get)
        most_productive_day_words = words_by_day[most_productive_day]

    # Records
    longest_transcription = max((t.word_count for t in all_transcripts), default=0)
    shortest_transcription = min((t.word_count for t in all_transcripts), default=0) if all_transcripts else 0
    fastest_wpm = max((t.words_per_minute for t in all_transcripts if t.words_per_minute), default=0)
    slowest_wpm = min((t.words_per_minute for t in all_transcripts if t.words_per_minute), default=0) if all_transcripts else 0

    # Productivity insights
    peak_hour = max(hourly_counts, key=lambda h: hourly_counts[h]["words"]) if hourly_counts else 12
    peak_hour_label = f"{peak_hour % 12 or 12}:00 {'AM' if peak_hour < 12 else 'PM'}"
    peak_dow = max(dow_counts, key=lambda d: dow_counts[d]["words"]) if dow_counts else 0
    peak_day = day_names[peak_dow]

    # Busiest week ever
    words_by_week: dict[str, int] = defaultdict(int)
    for t in all_transcripts:
        week_key = t.created_at.strftime("%Y-W%W")
        words_by_week[week_key] += t.word_count

    busiest_week = max(words_by_week, key=words_by_week.get) if words_by_week else None
    busiest_week_words = words_by_week.get(busiest_week, 0) if busiest_week else 0

    # Efficiency score (words per minute of audio)
    efficiency = (current_user.total_words / (current_user.total_audio_seconds / 60)) if current_user.total_audio_seconds > 0 else 0

    productivity = ProductivityInsights(
        peak_hour=peak_hour,
        peak_hour_label=peak_hour_label,
        peak_day=peak_day,
        avg_session_words=round(avg_words, 1),
        avg_session_duration_seconds=round(avg_audio_duration, 1),
        busiest_week_ever=busiest_week,
        busiest_week_words=busiest_week_words,
        efficiency_score=round(efficiency, 1),
    )

    # Achievements
    achievements = []

    # Word milestones
    word_milestones = [
        (100, "First Steps", "Transcribed 100 words", "🚀"),
        (500, "Getting Started", "Transcribed 500 words", "📝"),
        (1000, "Word Warrior", "Transcribed 1,000 words", "⚔️"),
        (5000, "Prolific Speaker", "Transcribed 5,000 words", "🎤"),
        (10000, "Word Master", "Transcribed 10,000 words", "🏆"),
        (25000, "Eloquent", "Transcribed 25,000 words", "✨"),
        (50000, "Voice Champion", "Transcribed 50,000 words", "👑"),
        (100000, "Legendary", "Transcribed 100,000 words", "🌟"),
    ]
    for target, name, desc, icon in word_milestones:
        earned = current_user.total_words >= target
        progress = min(100, (current_user.total_words / target) * 100)
        achievements.append(Achievement(
            id=f"words_{target}",
            name=name,
            description=desc,
            icon=icon,
            earned=earned,
            progress=round(progress, 1),
            target=target,
            current=current_user.total_words,
        ))

    # Transcription count milestones
    trans_milestones = [
        (10, "Beginner", "Completed 10 transcriptions", "🌱"),
        (50, "Regular", "Completed 50 transcriptions", "🌿"),
        (100, "Dedicated", "Completed 100 transcriptions", "🌳"),
        (500, "Power User", "Completed 500 transcriptions", "⚡"),
        (1000, "Transcription Master", "Completed 1,000 transcriptions", "🎯"),
    ]
    for target, name, desc, icon in trans_milestones:
        earned = current_user.total_transcriptions >= target
        progress = min(100, (current_user.total_transcriptions / target) * 100)
        achievements.append(Achievement(
            id=f"trans_{target}",
            name=name,
            description=desc,
            icon=icon,
            earned=earned,
            progress=round(progress, 1),
            target=target,
            current=current_user.total_transcriptions,
        ))

    # Streak achievements
    streak_milestones = [
        (3, "Consistent", "3-day streak", "🔥"),
        (7, "Week Warrior", "7-day streak", "📅"),
        (14, "Fortnight Fighter", "14-day streak", "💪"),
        (30, "Monthly Master", "30-day streak", "🗓️"),
        (100, "Centurion", "100-day streak", "💯"),
    ]
    for target, name, desc, icon in streak_milestones:
        earned = longest_streak >= target
        progress = min(100, (longest_streak / target) * 100)
        achievements.append(Achievement(
            id=f"streak_{target}",
            name=name,
            description=desc,
            icon=icon,
            earned=earned,
            progress=round(progress, 1),
            target=target,
            current=longest_streak,
        ))

    # Time saved achievements
    time_milestones = [
        (60, "Time Saver", "Saved 1 hour of typing", "⏱️"),
        (300, "Efficiency Expert", "Saved 5 hours of typing", "⏰"),
        (600, "Productivity Pro", "Saved 10 hours of typing", "🕐"),
        (1800, "Time Lord", "Saved 30 hours of typing", "⌛"),
    ]
    for target, name, desc, icon in time_milestones:
        earned = time_saved >= target
        progress = min(100, (time_saved / target) * 100)
        achievements.append(Achievement(
            id=f"time_{target}",
            name=name,
            description=desc,
            icon=icon,
            earned=earned,
            progress=round(progress, 1),
            target=target,
            current=int(time_saved),
        ))

    # Special achievements
    if longest_transcription >= 500:
        achievements.append(Achievement(
            id="long_form",
            name="Long Form",
            description="Single transcription over 500 words",
            icon="📜",
            earned=True,
            progress=100,
        ))

    if fastest_wpm >= 200:
        achievements.append(Achievement(
            id="speed_demon",
            name="Speed Demon",
            description="Achieved 200+ words per minute",
            icon="💨",
            earned=True,
            progress=100,
        ))

    # Sort achievements: earned first, then by progress
    achievements.sort(key=lambda a: (-a.earned, -a.progress))

    # Average transcriptions per day
    days_since_signup = max(1, (now - current_user.created_at).days)
    avg_per_day = current_user.total_transcriptions / days_since_signup

    return DetailedStatsResponse(
        total_transcriptions=current_user.total_transcriptions,
        total_words=current_user.total_words,
        total_audio_seconds=current_user.total_audio_seconds,
        total_characters=total_characters,
        transcriptions_today=transcriptions_today,
        words_today=words_today,
        transcriptions_this_week=transcriptions_week,
        words_this_week=words_week,
        transcriptions_this_month=transcriptions_month,
        words_this_month=words_month,
        average_words_per_transcription=round(avg_words, 1),
        average_words_per_minute=round(avg_wpm, 1),
        average_transcriptions_per_day=round(avg_per_day, 2),
        average_audio_duration_seconds=round(avg_audio_duration, 1),
        estimated_time_saved_minutes=round(time_saved, 1),
        context_breakdown=context_breakdown,
        formality_breakdown=formality_breakdown,
        daily_activity=daily_activity,
        hourly_activity=hourly_activity,
        day_of_week_breakdown=day_of_week_breakdown,
        monthly_trends=monthly_trends,
        word_length_distribution=word_length_distribution,
        current_streak_days=current_streak,
        longest_streak_days=longest_streak,
        most_productive_day=most_productive_day,
        most_productive_day_words=most_productive_day_words,
        longest_transcription_words=longest_transcription,
        shortest_transcription_words=shortest_transcription,
        fastest_wpm=round(fastest_wpm, 1),
        slowest_wpm=round(slowest_wpm, 1),
        growth=growth,
        productivity=productivity,
        achievements=achievements,
        member_since=current_user.created_at,
        days_as_member=days_since_signup,
        total_active_days=len(unique_days),
    )
