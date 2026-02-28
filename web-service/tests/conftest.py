"""Shared test fixtures — async SQLite engine, test client, auth helpers."""

import os

# Set env vars BEFORE importing app modules (config reads at import time)
os.environ.setdefault("VAAK_WEB_SECRET_KEY", "test-secret-key-for-tests")
os.environ.setdefault("VAAK_WEB_DATABASE_URL", "sqlite+aiosqlite:///:memory:")
os.environ["VAAK_WEB_TESTING"] = "1"  # Bypass rate limiter middleware in tests

import asyncio
import sys

import pytest
from httpx import ASGITransport, AsyncClient
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine

from app.database import Base, get_db  # noqa: E402
from app.main import app  # noqa: E402
from app.api.auth import create_access_token, _rate_limit_store  # noqa: E402
from app.middleware.rate_limiter import _buckets as _middleware_buckets  # noqa: E402

# Use SelectorEventLoop on Windows to avoid ProactorEventLoop cleanup hangs
if sys.platform == "win32":
    asyncio.set_event_loop_policy(asyncio.WindowsSelectorEventLoopPolicy())

# Async SQLite engine for tests (in-memory, fast)
test_engine = create_async_engine("sqlite+aiosqlite:///:memory:", echo=False)
TestSession = async_sessionmaker(test_engine, class_=AsyncSession, expire_on_commit=False)


@pytest.fixture(autouse=True)
async def setup_db():
    """Create all tables before each test, drop after."""
    async with test_engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
    yield
    async with test_engine.begin() as conn:
        await conn.run_sync(Base.metadata.drop_all)


@pytest.fixture(autouse=True)
def clear_rate_limits():
    """Clear in-memory rate limit stores between tests (auth + middleware)."""
    _rate_limit_store.clear()
    _middleware_buckets.clear()
    yield
    _rate_limit_store.clear()
    _middleware_buckets.clear()


async def _override_get_db():
    async with TestSession() as session:
        try:
            yield session
        except Exception:
            await session.rollback()
            raise

app.dependency_overrides[get_db] = _override_get_db


@pytest.fixture
async def client():
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as c:
        yield c


@pytest.fixture
async def db():
    async with TestSession() as session:
        yield session


async def create_test_user(
    client: AsyncClient,
    email: str = "test@example.com",
    password: str = "testpass123",
    full_name: str = "Test User",
) -> dict:
    """Helper: sign up a user and return the token response."""
    resp = await client.post("/api/v1/auth/signup", json={
        "email": email,
        "password": password,
        "full_name": full_name,
    })
    assert resp.status_code == 201, f"Signup failed: {resp.text}"
    return resp.json()


def auth_headers(token: str) -> dict:
    """Helper: return Authorization header dict."""
    return {"Authorization": f"Bearer {token}"}
