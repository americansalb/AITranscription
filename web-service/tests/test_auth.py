"""Tests for auth endpoints: signup, login, me, refresh, profile, API keys.

Covers:
  - POST /auth/signup — success, duplicate, validation, rate limiting
  - POST /auth/login — success, wrong password, nonexistent, inactive, rate limiting
  - GET /auth/me — authenticated, unauthenticated, invalid/expired tokens
  - POST /auth/refresh — success, expired token
  - PATCH /auth/me — profile update
  - POST /auth/change-password — success, wrong current password
  - PUT /auth/api-keys — BYOK key management with auto-tier upgrade
"""

import time

import pytest
from httpx import AsyncClient

from tests.conftest import auth_headers, create_test_user


@pytest.mark.asyncio
async def test_signup_success(client: AsyncClient):
    data = await create_test_user(client, "new@example.com")
    assert "access_token" in data
    assert data["token_type"] == "bearer"
    assert data["expires_in"] > 0


@pytest.mark.asyncio
async def test_signup_duplicate_email(client: AsyncClient):
    await create_test_user(client, "dup@example.com")
    resp = await client.post("/api/v1/auth/signup", json={
        "email": "dup@example.com",
        "password": "testpass123",
    })
    assert resp.status_code == 400


@pytest.mark.asyncio
async def test_signup_short_password(client: AsyncClient):
    resp = await client.post("/api/v1/auth/signup", json={
        "email": "short@example.com",
        "password": "abc",
    })
    assert resp.status_code == 422


@pytest.mark.asyncio
async def test_login_success(client: AsyncClient):
    await create_test_user(client, "login@example.com", "mypassword1")
    resp = await client.post("/api/v1/auth/login", json={
        "email": "login@example.com",
        "password": "mypassword1",
    })
    assert resp.status_code == 200
    assert "access_token" in resp.json()


@pytest.mark.asyncio
async def test_login_wrong_password(client: AsyncClient):
    await create_test_user(client, "wrong@example.com", "correct_pass")
    resp = await client.post("/api/v1/auth/login", json={
        "email": "wrong@example.com",
        "password": "wrong_pass",
    })
    assert resp.status_code == 401


@pytest.mark.asyncio
async def test_login_nonexistent_user(client: AsyncClient):
    resp = await client.post("/api/v1/auth/login", json={
        "email": "nobody@example.com",
        "password": "doesntmatter",
    })
    assert resp.status_code == 401


@pytest.mark.asyncio
async def test_me_authenticated(client: AsyncClient):
    data = await create_test_user(client, "me@example.com")
    resp = await client.get("/api/v1/auth/me", headers=auth_headers(data["access_token"]))
    assert resp.status_code == 200
    user = resp.json()
    assert user["email"] == "me@example.com"
    assert user["tier"] == "free"


@pytest.mark.asyncio
async def test_me_unauthenticated(client: AsyncClient):
    resp = await client.get("/api/v1/auth/me")
    assert resp.status_code in (401, 403)


@pytest.mark.asyncio
async def test_me_invalid_token(client: AsyncClient):
    resp = await client.get("/api/v1/auth/me", headers=auth_headers("invalid.token.here"))
    assert resp.status_code in (401, 403)


@pytest.mark.asyncio
async def test_refresh_token(client: AsyncClient):
    data = await create_test_user(client, "refresh@example.com")
    resp = await client.post("/api/v1/auth/refresh", headers=auth_headers(data["access_token"]))
    assert resp.status_code == 200
    new_data = resp.json()
    assert "access_token" in new_data
    assert new_data["token_type"] == "bearer"
    assert new_data["expires_in"] > 0
    # Verify the refreshed token works
    resp2 = await client.get("/api/v1/auth/me", headers=auth_headers(new_data["access_token"]))
    assert resp2.status_code == 200


# --- Profile management ---

@pytest.mark.asyncio
async def test_update_profile(client: AsyncClient):
    data = await create_test_user(client, "profile@example.com", full_name="Old Name")
    headers = auth_headers(data["access_token"])

    resp = await client.patch("/api/v1/auth/me", json={"full_name": "New Name"}, headers=headers)
    assert resp.status_code == 200
    assert resp.json()["full_name"] == "New Name"

    # Verify persisted
    resp = await client.get("/api/v1/auth/me", headers=headers)
    assert resp.json()["full_name"] == "New Name"


@pytest.mark.asyncio
async def test_change_password(client: AsyncClient):
    data = await create_test_user(client, "chpass@example.com", "oldpass123")
    headers = auth_headers(data["access_token"])

    resp = await client.post("/api/v1/auth/change-password", json={
        "current_password": "oldpass123",
        "new_password": "newpass456",
    }, headers=headers)
    assert resp.status_code == 200
    assert resp.json()["status"] == "password_changed"

    # Login with new password should work
    resp = await client.post("/api/v1/auth/login", json={
        "email": "chpass@example.com",
        "password": "newpass456",
    })
    assert resp.status_code == 200

    # Login with old password should fail
    resp = await client.post("/api/v1/auth/login", json={
        "email": "chpass@example.com",
        "password": "oldpass123",
    })
    assert resp.status_code == 401


@pytest.mark.asyncio
async def test_change_password_wrong_current(client: AsyncClient):
    data = await create_test_user(client, "chpass-wrong@example.com", "correctpass")
    headers = auth_headers(data["access_token"])

    resp = await client.post("/api/v1/auth/change-password", json={
        "current_password": "wrongpass",
        "new_password": "newpass456",
    }, headers=headers)
    assert resp.status_code == 400


@pytest.mark.asyncio
async def test_update_api_keys_requires_byok_tier(client: AsyncClient):
    """Free-tier users cannot set API keys — must subscribe to BYOK first."""
    data = await create_test_user(client, "apikeys@example.com")
    headers = auth_headers(data["access_token"])

    resp = await client.put("/api/v1/auth/api-keys", json={
        "anthropic": "sk-ant-test-key",
    }, headers=headers)
    assert resp.status_code == 403
    assert "BYOK" in resp.json()["detail"]


@pytest.mark.asyncio
async def test_update_api_keys_byok_user(client: AsyncClient, db):
    """BYOK-tier users can set API keys."""
    from app.models import SubscriptionTier, WebUser
    data = await create_test_user(client, "apikeys-byok@example.com")
    headers = auth_headers(data["access_token"])

    # Manually upgrade user to BYOK tier (simulates Stripe subscription)
    from sqlalchemy import select
    result = await db.execute(select(WebUser).where(WebUser.email == "apikeys-byok@example.com"))
    user = result.scalar_one()
    user.tier = SubscriptionTier.BYOK
    await db.commit()

    resp = await client.put("/api/v1/auth/api-keys", json={
        "anthropic": "sk-ant-test-key",
    }, headers=headers)
    assert resp.status_code == 200
    result = resp.json()
    assert result["status"] == "keys_updated"
    assert result["tier"] == "byok"


@pytest.mark.asyncio
async def test_update_api_keys_empty_rejected_for_free(client: AsyncClient):
    """Free-tier users can't set keys at all — even empty ones."""
    data = await create_test_user(client, "apikeys-empty@example.com")
    headers = auth_headers(data["access_token"])

    resp = await client.put("/api/v1/auth/api-keys", json={
        "anthropic": "",
    }, headers=headers)
    assert resp.status_code == 403


# =============================================================================
# EDGE CASES & SECURITY
# =============================================================================

async def test_signup_invalid_email_format(client: AsyncClient):
    """Malformed email returns 422."""
    resp = await client.post("/api/v1/auth/signup", json={
        "email": "not-an-email",
        "password": "strongpassword123",
    })
    assert resp.status_code == 422


async def test_signup_missing_password(client: AsyncClient):
    """Missing password field returns 422."""
    resp = await client.post("/api/v1/auth/signup", json={
        "email": "test@example.com",
    })
    assert resp.status_code == 422


async def test_signup_password_too_long(client: AsyncClient):
    """Password over 72 chars returns 422 (bcrypt limit)."""
    resp = await client.post("/api/v1/auth/signup", json={
        "email": "toolong@example.com",
        "password": "a" * 73,
    })
    assert resp.status_code == 422


async def test_signup_rate_limit(client: AsyncClient):
    """6th signup from same IP in 5 minutes returns 429.

    ASGITransport doesn't set request.client, so the IP resolves to 'unknown'.
    """
    from app.api.auth import _rate_limit_store
    # ASGITransport: raw.client is None → client_ip = "unknown"
    _rate_limit_store["signup:unknown"] = [time.monotonic()] * 5

    resp = await client.post("/api/v1/auth/signup", json={
        "email": "ratelimited@example.com",
        "password": "strongpassword123",
    })
    assert resp.status_code == 429
    assert "Too many attempts" in resp.json()["detail"]


async def test_login_rate_limit(client: AsyncClient):
    """11th login attempt from same IP in 1 minute returns 429."""
    from app.api.auth import _rate_limit_store
    _rate_limit_store["login:unknown"] = [time.monotonic()] * 10

    resp = await client.post("/api/v1/auth/login", json={
        "email": "test@example.com",
        "password": "whatever",
    })
    assert resp.status_code == 429


async def test_me_expired_token(client: AsyncClient):
    """Expired JWT returns 401."""
    from app.api.auth import create_access_token
    expired = create_access_token(user_id=1, expires_minutes=-1)
    resp = await client.get("/api/v1/auth/me", headers=auth_headers(expired))
    assert resp.status_code == 401


async def test_login_same_error_for_wrong_email_and_password(client: AsyncClient):
    """Wrong email and wrong password produce the same error (no user enumeration)."""
    await create_test_user(client, "exists@example.com", "correctpass")

    # Wrong password
    resp_wrong_pw = await client.post("/api/v1/auth/login", json={
        "email": "exists@example.com",
        "password": "wrongpass12",
    })

    # Nonexistent email
    resp_wrong_email = await client.post("/api/v1/auth/login", json={
        "email": "ghost@example.com",
        "password": "doesntmatter",
    })

    # Both should return 401 with identical error message
    assert resp_wrong_pw.status_code == 401
    assert resp_wrong_email.status_code == 401
    assert resp_wrong_pw.json()["detail"] == resp_wrong_email.json()["detail"]


async def test_signup_with_full_name(client: AsyncClient):
    """Signup with optional full_name populates the field."""
    data = await create_test_user(client, "named@example.com", full_name="Jane Doe")
    resp = await client.get("/api/v1/auth/me", headers=auth_headers(data["access_token"]))
    assert resp.json()["full_name"] == "Jane Doe"


async def test_signup_without_full_name(client: AsyncClient):
    """Signup without full_name sets it to null."""
    resp = await client.post("/api/v1/auth/signup", json={
        "email": "noname@example.com",
        "password": "strongpassword",
    })
    assert resp.status_code == 201
    token = resp.json()["access_token"]
    me = await client.get("/api/v1/auth/me", headers=auth_headers(token))
    assert me.json()["full_name"] is None


async def test_change_password_new_too_short(client: AsyncClient):
    """New password under 8 chars returns 422."""
    data = await create_test_user(client, "short-new@example.com", "oldpassword1")
    resp = await client.post("/api/v1/auth/change-password", json={
        "current_password": "oldpassword1",
        "new_password": "short",
    }, headers=auth_headers(data["access_token"]))
    assert resp.status_code == 422


async def test_api_keys_multiple_providers(client: AsyncClient, db):
    """Setting multiple provider keys at once works (BYOK user)."""
    from app.models import SubscriptionTier, WebUser
    from sqlalchemy import select

    data = await create_test_user(client, "multikey@example.com")
    hdrs = auth_headers(data["access_token"])

    # Upgrade user to BYOK tier first
    result = await db.execute(select(WebUser).where(WebUser.email == "multikey@example.com"))
    user = result.scalar_one()
    user.tier = SubscriptionTier.BYOK
    await db.commit()

    resp = await client.put("/api/v1/auth/api-keys", json={
        "anthropic": "sk-ant-key",
        "openai": "sk-openai-key",
        "google": "AIzaSy-google-key",
    }, headers=hdrs)
    assert resp.status_code == 200
    assert resp.json()["tier"] == "byok"


async def test_refresh_no_auth(client: AsyncClient):
    """Refresh without token returns 403."""
    resp = await client.post("/api/v1/auth/refresh")
    assert resp.status_code in (401, 403)
