"""Tests for auth endpoints: signup, login, me, refresh."""

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
    assert resp.status_code == 409


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
    assert new_data["access_token"] != data["access_token"]
