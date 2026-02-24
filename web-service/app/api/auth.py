"""Authentication endpoints — signup, login, OAuth, token refresh."""

from datetime import datetime, timedelta, timezone

from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel, EmailStr, Field

router = APIRouter()


# --- Request/Response schemas ---

class SignupRequest(BaseModel):
    email: EmailStr
    password: str = Field(min_length=8)
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
async def signup(request: SignupRequest):
    """Create a new user account."""
    # TODO: hash password, create user in DB, generate JWT
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.post("/login", response_model=TokenResponse)
async def login(request: LoginRequest):
    """Log in with email and password."""
    # TODO: verify password, generate JWT
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.get("/me", response_model=UserResponse)
async def get_current_user():
    """Get current user info from JWT."""
    # TODO: decode JWT, fetch user
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.post("/refresh", response_model=TokenResponse)
async def refresh_token():
    """Refresh an expiring JWT."""
    # TODO: validate refresh token, issue new access token
    raise HTTPException(status_code=501, detail="Not implemented yet")
