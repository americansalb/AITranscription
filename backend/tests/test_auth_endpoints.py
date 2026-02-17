"""Tests for the auth API endpoints — signup, login, /me, refresh, protected routes.

Uses the async FastAPI test client from conftest.py with mocked DB.
Covers:
  - Signup: happy path, duplicate email, validation errors
  - Login: happy path, wrong password, nonexistent user, inactive user
  - /me: authenticated access, missing token, expired token, invalid token
  - /refresh: token renewal
  - /settings/typing-wpm: update with valid/invalid values
"""
import pytest
from unittest.mock import AsyncMock, MagicMock, patch
from datetime import timedelta

from app.services.auth import create_access_token, hash_password


# === Signup Tests ===

class TestSignup:

    async def test_signup_success(self, client):
        """Successful signup returns 201 with access token."""
        mock_db = client._mock_db

        # get_user_by_email returns None (no existing user)
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = None
        mock_db.execute.return_value = mock_result

        # create_user needs refresh to set user.id
        new_user = MagicMock()
        new_user.id = 1
        new_user.email = "test@example.com"

        async def fake_refresh(obj):
            obj.id = 1

        mock_db.refresh = fake_refresh

        response = await client.post("/api/v1/auth/signup", json={
            "email": "test@example.com",
            "password": "Secure-Pass-123",
            "full_name": "Test User",
        })

        assert response.status_code == 201
        data = response.json()
        assert "access_token" in data
        assert data["token_type"] == "bearer"

    async def test_signup_duplicate_email(self, client):
        """Signup with existing email returns 400."""
        mock_db = client._mock_db

        # Simulate existing user found
        existing_user = MagicMock()
        existing_user.email = "existing@example.com"
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = existing_user
        mock_db.execute.return_value = mock_result

        response = await client.post("/api/v1/auth/signup", json={
            "email": "existing@example.com",
            "password": "Password123",
        })

        assert response.status_code == 400
        assert "already registered" in response.json()["detail"]

    async def test_signup_short_password(self, client):
        """Signup with password < 6 chars returns 422 validation error."""
        response = await client.post("/api/v1/auth/signup", json={
            "email": "test@example.com",
            "password": "short",
        })

        assert response.status_code == 422

    async def test_signup_invalid_email(self, client):
        """Signup with invalid email format returns 422."""
        response = await client.post("/api/v1/auth/signup", json={
            "email": "not-an-email",
            "password": "password123",
        })

        assert response.status_code == 422

    async def test_signup_missing_password(self, client):
        """Signup without password returns 422."""
        response = await client.post("/api/v1/auth/signup", json={
            "email": "test@example.com",
        })

        assert response.status_code == 422

    async def test_signup_missing_email(self, client):
        """Signup without email returns 422."""
        response = await client.post("/api/v1/auth/signup", json={
            "password": "password123",
        })

        assert response.status_code == 422

    async def test_signup_password_too_long(self, client):
        """Signup with password > 72 chars returns 422 (bcrypt limit)."""
        response = await client.post("/api/v1/auth/signup", json={
            "email": "test@example.com",
            "password": "a" * 73,
        })

        assert response.status_code == 422


# === Login Tests ===

class TestLogin:

    async def test_login_success(self, client):
        """Successful login returns access token."""
        mock_db = client._mock_db

        # Mock authenticate_user to return a valid user
        user = MagicMock()
        user.id = 1
        user.email = "test@example.com"
        user.is_active = True

        with patch("app.api.auth.authenticate_user", return_value=user):
            response = await client.post("/api/v1/auth/login", json={
                "email": "test@example.com",
                "password": "correct-password",
            })

        assert response.status_code == 200
        data = response.json()
        assert "access_token" in data
        assert data["token_type"] == "bearer"

    async def test_login_wrong_password(self, client):
        """Login with wrong password returns 401."""
        with patch("app.api.auth.authenticate_user", return_value=None):
            response = await client.post("/api/v1/auth/login", json={
                "email": "test@example.com",
                "password": "wrong-password",
            })

        assert response.status_code == 401
        assert "Invalid email or password" in response.json()["detail"]

    async def test_login_nonexistent_user(self, client):
        """Login with nonexistent email returns 401."""
        with patch("app.api.auth.authenticate_user", return_value=None):
            response = await client.post("/api/v1/auth/login", json={
                "email": "nobody@example.com",
                "password": "password123",
            })

        assert response.status_code == 401

    async def test_login_inactive_user(self, client):
        """Login as inactive user returns 403."""
        user = MagicMock()
        user.id = 1
        user.email = "inactive@example.com"
        user.is_active = False

        with patch("app.api.auth.authenticate_user", return_value=user):
            response = await client.post("/api/v1/auth/login", json={
                "email": "inactive@example.com",
                "password": "correct-password",
            })

        assert response.status_code == 403
        assert "inactive" in response.json()["detail"]

    async def test_login_invalid_email_format(self, client):
        """Login with invalid email format returns 422."""
        response = await client.post("/api/v1/auth/login", json={
            "email": "not-an-email",
            "password": "password123",
        })

        assert response.status_code == 422


# === /me Endpoint Tests ===

class TestGetMe:

    async def test_me_authenticated(self, client, auth_headers):
        """Authenticated /me returns user info."""
        from tests.conftest import make_user
        user = make_user(id=1, email="test@example.com", full_name="Test User")

        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.get("/api/v1/auth/me", headers=auth_headers)

        assert response.status_code == 200
        data = response.json()
        assert data["id"] == 1
        assert data["email"] == "test@example.com"
        assert data["full_name"] == "Test User"
        assert data["is_active"] is True

    async def test_me_no_token(self, client):
        """Request to /me without token returns 401."""
        response = await client.get("/api/v1/auth/me")
        assert response.status_code == 401

    async def test_me_invalid_token(self, client):
        """Request to /me with garbage token returns 401."""
        headers = {"Authorization": "Bearer garbage.invalid.token"}
        response = await client.get("/api/v1/auth/me", headers=headers)
        assert response.status_code == 401

    async def test_me_expired_token(self, client):
        """Request to /me with expired token returns 401."""
        expired_token = create_access_token(user_id=1, expires_delta=timedelta(seconds=-10))
        headers = {"Authorization": f"Bearer {expired_token}"}
        response = await client.get("/api/v1/auth/me", headers=headers)
        assert response.status_code == 401

    async def test_me_user_not_found(self, client, auth_headers):
        """Valid token but user deleted from DB returns 401."""
        with patch("app.api.auth.get_user_by_id", return_value=None):
            response = await client.get("/api/v1/auth/me", headers=auth_headers)
        assert response.status_code == 401

    async def test_me_inactive_user(self, client, auth_headers):
        """Valid token but inactive user returns 403."""
        user = make_user_obj(is_active=False)

        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.get("/api/v1/auth/me", headers=auth_headers)
        assert response.status_code == 403


# === Token Refresh Tests ===

class TestRefreshToken:

    async def test_refresh_success(self, client, auth_headers):
        """Token refresh returns a new valid token."""
        from tests.conftest import make_user
        user = make_user(id=1)

        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post("/api/v1/auth/refresh", headers=auth_headers)

        assert response.status_code == 200
        data = response.json()
        assert "access_token" in data
        assert data["token_type"] == "bearer"

    async def test_refresh_without_token(self, client):
        """Refresh without token returns 401."""
        response = await client.post("/api/v1/auth/refresh")
        assert response.status_code == 401


# === Typing WPM Settings Tests ===

class TestTypingWpmSettings:

    async def test_update_wpm_success(self, client, auth_headers):
        """Update typing WPM with valid value succeeds."""
        from tests.conftest import make_user
        user = make_user(id=1, typing_wpm=40)

        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.patch(
                "/api/v1/auth/settings/typing-wpm",
                json={"wpm": 60},
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["typing_wpm"] == 60

    async def test_update_wpm_too_low(self, client, auth_headers):
        """WPM below 1 returns 422."""
        from tests.conftest import make_user
        user = make_user(id=1)

        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.patch(
                "/api/v1/auth/settings/typing-wpm",
                json={"wpm": 0},
                headers=auth_headers,
            )

        assert response.status_code == 422

    async def test_update_wpm_too_high(self, client, auth_headers):
        """WPM above 200 returns 422."""
        from tests.conftest import make_user
        user = make_user(id=1)

        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.patch(
                "/api/v1/auth/settings/typing-wpm",
                json={"wpm": 201},
                headers=auth_headers,
            )

        assert response.status_code == 422

    async def test_update_wpm_no_auth(self, client):
        """Update WPM without auth returns 401."""
        response = await client.patch(
            "/api/v1/auth/settings/typing-wpm",
            json={"wpm": 60},
        )
        assert response.status_code == 401


# === Helper ===

def make_user_obj(**overrides):
    """Create a mock User object with sensible defaults."""
    user = MagicMock()
    defaults = {
        "id": 1,
        "email": "test@example.com",
        "full_name": "Test User",
        "hashed_password": "$2b$12$fakehash",
        "tier": "standard",
        "is_active": True,
        "is_admin": False,
        "accessibility_verified": False,
        "total_transcriptions": 10,
        "total_words": 500,
        "total_audio_seconds": 120,
        "total_polish_tokens": 1000,
        "typing_wpm": 40,
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(user, key, value)
    return user
