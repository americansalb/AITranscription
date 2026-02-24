"""Authentication endpoints — signup, login, token refresh, user info."""

import logging
import time
from collections import defaultdict
from datetime import datetime, timedelta, timezone

from fastapi import APIRouter, Depends, HTTPException, Request, status
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer
from jose import JWTError, jwt
from passlib.context import CryptContext
from pydantic import BaseModel, EmailStr, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.config import settings
from app.database import get_db
from app.models import SubscriptionTier, WebUser

logger = logging.getLogger(__name__)
router = APIRouter()
security = HTTPBearer()
pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")

ALGORITHM = "HS256"

# --- Rate limiting ---

_rate_limit_store: dict[str, list[float]] = defaultdict(list)
def _check_rate_limit(key: str, max_attempts: int, window_seconds: int = 60) -> None:
    now = time.monotonic()
    cutoff = now - window_seconds
    _rate_limit_store[key] = [t for t in _rate_limit_store[key] if t > cutoff]
    if len(_rate_limit_store[key]) >= max_attempts:
        raise HTTPException(
            status_code=status.HTTP_429_TOO_MANY_REQUESTS,
            detail=f"Too many attempts. Try again in {window_seconds} seconds.",
        )
    _rate_limit_store[key].append(now)


# --- JWT helpers ---

def create_access_token(user_id: int, expires_minutes: int | None = None) -> str:
    expire = datetime.now(timezone.utc) + timedelta(
        minutes=expires_minutes or settings.access_token_expire_minutes
    )
    return jwt.encode(
        {"sub": str(user_id), "exp": expire, "iat": datetime.now(timezone.utc)},
        settings.secret_key,
        algorithm=ALGORITHM,
    )


def decode_access_token(token: str) -> int | None:
    try:
        payload = jwt.decode(token, settings.secret_key, algorithms=[ALGORITHM])
        user_id = payload.get("sub")
        return int(user_id) if user_id else None
    except JWTError:
        return None


# --- Dependencies ---

async def get_current_user(
    credentials: HTTPAuthorizationCredentials = Depends(security),
    db: AsyncSession = Depends(get_db),
) -> WebUser:
    """Extract and validate the current user from JWT."""
    user_id = decode_access_token(credentials.credentials)
    if user_id is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid or expired token",
            headers={"WWW-Authenticate": "Bearer"},
        )

    result = await db.execute(select(WebUser).where(WebUser.id == user_id))
    user = result.scalar_one_or_none()
    if user is None:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="User not found")
    if not user.is_active:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Account inactive")
    return user


async def get_optional_user(
    credentials: HTTPAuthorizationCredentials | None = Depends(HTTPBearer(auto_error=False)),
    db: AsyncSession = Depends(get_db),
) -> WebUser | None:
    """Optional auth — returns user if token present, None otherwise."""
    if credentials is None:
        return None
    try:
        return await get_current_user(credentials, db)
    except HTTPException:
        return None


# --- Request/Response schemas ---

class SignupRequest(BaseModel):
    email: EmailStr
    password: str = Field(min_length=8, max_length=72)
    full_name: str | None = None


class LoginRequest(BaseModel):
    email: EmailStr
    password: str


class TokenResponse(BaseModel):
    access_token: str
    token_type: str = "bearer"
    expires_in: int


class UserResponse(BaseModel):
    id: int
    email: str
    full_name: str | None
    tier: str
    created_at: datetime


# --- Endpoints ---

@router.post("/signup", response_model=TokenResponse, status_code=status.HTTP_201_CREATED)
async def signup(request: SignupRequest, raw: Request, db: AsyncSession = Depends(get_db)):
    """Create a new user account."""
    client_ip = raw.client.host if raw.client else "unknown"
    _check_rate_limit(f"signup:{client_ip}", max_attempts=5, window_seconds=300)

    # Check email uniqueness
    existing = await db.execute(select(WebUser).where(WebUser.email == request.email))
    if existing.scalar_one_or_none():
        raise HTTPException(status_code=400, detail="Email already registered")

    # Create user
    user = WebUser(
        email=request.email,
        hashed_password=pwd_context.hash(request.password),
        full_name=request.full_name,
    )
    db.add(user)
    await db.commit()
    await db.refresh(user)

    token = create_access_token(user.id)
    logger.info("User signed up: %d %s", user.id, user.email)

    return TokenResponse(
        access_token=token,
        expires_in=settings.access_token_expire_minutes * 60,
    )


@router.post("/login", response_model=TokenResponse)
async def login(request: LoginRequest, raw: Request, db: AsyncSession = Depends(get_db)):
    """Log in with email and password."""
    client_ip = raw.client.host if raw.client else "unknown"
    _check_rate_limit(f"login:{client_ip}", max_attempts=10, window_seconds=60)

    result = await db.execute(select(WebUser).where(WebUser.email == request.email))
    user = result.scalar_one_or_none()

    if not user or not pwd_context.verify(request.password, user.hashed_password):
        raise HTTPException(status_code=401, detail="Invalid email or password")

    if not user.is_active:
        raise HTTPException(status_code=403, detail="Account inactive")

    token = create_access_token(user.id)
    logger.info("User logged in: %d %s", user.id, user.email)

    return TokenResponse(
        access_token=token,
        expires_in=settings.access_token_expire_minutes * 60,
    )


@router.get("/me", response_model=UserResponse)
async def get_me(user: WebUser = Depends(get_current_user)):
    """Get current user info."""
    return UserResponse(
        id=user.id,
        email=user.email,
        full_name=user.full_name,
        tier=user.tier.value,
        created_at=user.created_at,
    )


@router.post("/refresh", response_model=TokenResponse)
async def refresh_token(user: WebUser = Depends(get_current_user)):
    """Refresh an expiring JWT."""
    token = create_access_token(user.id)
    return TokenResponse(
        access_token=token,
        expires_in=settings.access_token_expire_minutes * 60,
    )


# --- Profile management ---

class UpdateProfileRequest(BaseModel):
    full_name: str | None = None


class ChangePasswordRequest(BaseModel):
    current_password: str
    new_password: str = Field(min_length=8, max_length=72)


class UpdateApiKeysRequest(BaseModel):
    anthropic: str | None = None
    openai: str | None = None
    google: str | None = None


@router.patch("/me", response_model=UserResponse)
async def update_profile(
    request: UpdateProfileRequest,
    user: WebUser = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """Update current user's profile (name)."""
    user.full_name = request.full_name
    await db.commit()
    await db.refresh(user)
    return UserResponse(
        id=user.id,
        email=user.email,
        full_name=user.full_name,
        tier=user.tier.value,
        created_at=user.created_at,
    )


@router.post("/change-password")
async def change_password(
    request: ChangePasswordRequest,
    user: WebUser = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """Change the current user's password."""
    if not pwd_context.verify(request.current_password, user.hashed_password):
        raise HTTPException(status_code=400, detail="Current password is incorrect")

    user.hashed_password = pwd_context.hash(request.new_password)
    await db.commit()
    return {"status": "password_changed"}


@router.put("/api-keys")
async def update_api_keys(
    request: UpdateApiKeysRequest,
    user: WebUser = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """Update BYOK API keys for the current user."""
    # Only BYOK-tier users (via Stripe subscription) can set API keys
    if user.tier != SubscriptionTier.BYOK:
        raise HTTPException(
            status_code=403,
            detail="API keys can only be set by BYOK-tier subscribers. Upgrade your plan first.",
        )

    from app.services.key_encryption import encrypt_key

    if request.anthropic is not None:
        user.byok_anthropic_key = encrypt_key(request.anthropic) if request.anthropic else None
    if request.openai is not None:
        user.byok_openai_key = encrypt_key(request.openai) if request.openai else None
    if request.google is not None:
        user.byok_google_key = encrypt_key(request.google) if request.google else None

    await db.commit()
    return {"status": "keys_updated", "tier": user.tier.value}
