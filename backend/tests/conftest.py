"""Shared test fixtures for Vaak backend tests.

Provides:
- Async FastAPI test client (no real DB)
- Mock database session
- Auth helpers (token generation for authenticated requests)
- Common test data factories
"""
import pytest
from unittest.mock import AsyncMock, MagicMock, patch

from httpx import ASGITransport, AsyncClient

from app.core.config import Settings


# Override settings before importing the app to avoid real DB connections
@pytest.fixture(autouse=True)
def override_settings(monkeypatch):
    """Override settings for all tests to avoid external dependencies."""
    monkeypatch.setenv("GROQ_API_KEY", "test-groq-key")
    monkeypatch.setenv("ANTHROPIC_API_KEY", "test-anthropic-key")
    monkeypatch.setenv("ELEVENLABS_API_KEY", "test-elevenlabs-key")
    monkeypatch.setenv("DATABASE_URL", "sqlite+aiosqlite:///test.db")
    monkeypatch.setenv("SECRET_KEY", "test-secret-key-for-jwt")


@pytest.fixture
def mock_db():
    """Mock async database session."""
    session = AsyncMock()
    session.execute = AsyncMock()
    session.commit = AsyncMock()
    session.flush = AsyncMock()
    session.rollback = AsyncMock()
    session.close = AsyncMock()
    session.refresh = AsyncMock()
    session.add = MagicMock()
    session.delete = AsyncMock()
    return session


@pytest.fixture
async def client():
    """Async HTTP test client for FastAPI app.

    Uses httpx AsyncClient with ASGI transport — no real server needed.
    Database dependency is overridden with a mock.
    """
    from app.main import app
    from app.core.database import get_db

    mock_session = AsyncMock()
    mock_session.execute = AsyncMock()
    mock_session.commit = AsyncMock()
    mock_session.flush = AsyncMock()
    mock_session.close = AsyncMock()
    mock_session.add = MagicMock()

    async def override_get_db():
        yield mock_session

    app.dependency_overrides[get_db] = override_get_db

    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as ac:
        ac._mock_db = mock_session  # expose for test access
        yield ac

    app.dependency_overrides.clear()


@pytest.fixture
def auth_token():
    """Generate a valid JWT token for authenticated test requests."""
    from app.services.auth import create_access_token
    return create_access_token(user_id=1)


@pytest.fixture
def auth_headers(auth_token):
    """Authorization headers with a valid bearer token."""
    return {"Authorization": f"Bearer {auth_token}"}


# --- Test data factories ---

def make_user(**overrides):
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
