"""Tests for auth stats and transcript history endpoints.

Covers:
  - GET /auth/transcripts — user's transcript history with pagination
  - GET /auth/stats — user's basic statistics
  - GET /auth/stats/detailed — detailed statistics with insights
"""
import pytest
from datetime import datetime, timedelta, timezone
from unittest.mock import AsyncMock, MagicMock, patch


def make_user(**overrides):
    """Create a mock User with defaults."""
    user = MagicMock()
    defaults = {
        "id": 1,
        "email": "stats@example.com",
        "full_name": "Stats User",
        "tier": "standard",
        "is_active": True,
        "is_admin": False,
        "accessibility_verified": False,
        "total_transcriptions": 5,
        "total_words": 250,
        "total_audio_seconds": 60,
        "total_polish_tokens": 500,
        "typing_wpm": 40,
        "created_at": datetime(2026, 1, 1, tzinfo=timezone.utc),
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(user, key, value)
    return user


def make_transcript(**overrides):
    """Create a mock Transcript with defaults."""
    t = MagicMock()
    defaults = {
        "id": 1,
        "raw_text": "hello world",
        "polished_text": "Hello, world.",
        "word_count": 2,
        "character_count": 13,
        "audio_duration_seconds": 3.0,
        "words_per_minute": 40.0,
        "context": "general",
        "formality": "neutral",
        "transcript_type": "input",
        "created_at": datetime.now(timezone.utc),
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(t, key, value)
    return t


# === GET /auth/transcripts ===

class TestGetTranscripts:

    async def test_transcripts_empty(self, client, auth_headers):
        """User with no transcripts gets empty list."""
        user = make_user()
        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = []
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.get("/api/v1/auth/transcripts", headers=auth_headers)

        assert resp.status_code == 200
        assert resp.json() == []

    async def test_transcripts_returns_items(self, client, auth_headers):
        """Transcripts endpoint returns formatted transcript items."""
        user = make_user()
        now = datetime.now(timezone.utc)
        t1 = make_transcript(id=1, raw_text="hello", polished_text="Hello.", created_at=now)
        t2 = make_transcript(id=2, raw_text="test", polished_text="Test.", created_at=now)

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = [t1, t2]
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.get("/api/v1/auth/transcripts", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert len(data) == 2
        assert data[0]["id"] == 1
        assert data[0]["raw_text"] == "hello"
        assert data[0]["polished_text"] == "Hello."

    async def test_transcripts_pagination(self, client, auth_headers):
        """Transcripts endpoint respects skip and limit params."""
        user = make_user()
        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = []
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.get(
                "/api/v1/auth/transcripts?skip=10&limit=5",
                headers=auth_headers,
            )

        assert resp.status_code == 200

    async def test_transcripts_limit_validation(self, client, auth_headers):
        """Limit > 100 returns 422 validation error."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.get(
                "/api/v1/auth/transcripts?limit=101",
                headers=auth_headers,
            )
        assert resp.status_code == 422

    async def test_transcripts_no_auth(self, client):
        """Transcripts without auth returns 401."""
        resp = await client.get("/api/v1/auth/transcripts")
        assert resp.status_code == 401


# === GET /auth/stats ===

class TestGetStats:

    async def test_stats_empty_user(self, client, auth_headers):
        """New user with no transcripts gets zeroed stats."""
        user = make_user(total_transcriptions=0, total_words=0, total_audio_seconds=0)
        mock_db = client._mock_db

        # Both queries return empty results
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = []
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.get("/api/v1/auth/stats", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["total_transcriptions"] == 0
        assert data["total_words"] == 0
        assert data["transcriptions_today"] == 0
        assert data["words_today"] == 0
        assert data["average_words_per_transcription"] == 0
        assert data["average_words_per_minute"] == 0
        assert "typing_wpm" in data

    async def test_stats_with_transcripts(self, client, auth_headers):
        """User with transcripts gets computed stats."""
        user = make_user(total_transcriptions=3, total_words=150, total_audio_seconds=30)
        now = datetime.now(timezone.utc)

        t1 = make_transcript(
            word_count=50, audio_duration_seconds=10, words_per_minute=120.0,
            transcript_type="input", created_at=now,
        )
        t2 = make_transcript(
            word_count=50, audio_duration_seconds=10, words_per_minute=130.0,
            transcript_type="input", created_at=now,
        )
        t3 = make_transcript(
            word_count=50, audio_duration_seconds=10, words_per_minute=140.0,
            transcript_type="input", created_at=now - timedelta(days=2),
        )

        mock_db = client._mock_db
        call_count = 0

        def side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            mock_result = MagicMock()
            if call_count == 1:
                # Today's input transcripts (last 24h)
                mock_result.scalars.return_value.all.return_value = [t1, t2]
            else:
                # All input transcripts
                mock_result.scalars.return_value.all.return_value = [t1, t2, t3]
            return mock_result

        mock_db.execute = AsyncMock(side_effect=side_effect)

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.get("/api/v1/auth/stats", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["total_transcriptions"] == 3
        assert data["total_words"] == 150
        assert data["transcriptions_today"] == 2
        assert data["words_today"] == 100
        assert data["typing_wpm"] == 40

    async def test_stats_no_auth(self, client):
        """Stats without auth returns 401."""
        resp = await client.get("/api/v1/auth/stats")
        assert resp.status_code == 401


# === GET /auth/stats/detailed ===

class TestGetDetailedStats:

    async def test_detailed_stats_empty(self, client, auth_headers):
        """Detailed stats for user with no transcripts returns valid structure."""
        user = make_user(
            total_transcriptions=0, total_words=0,
            total_audio_seconds=0, typing_wpm=40,
            created_at=datetime(2026, 1, 1, tzinfo=timezone.utc),
        )
        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = []
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.get("/api/v1/auth/stats/detailed", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        # Verify response structure
        assert data["total_transcriptions"] == 0
        assert data["total_words"] == 0
        assert data["total_audio_seconds"] == 0
        assert data["total_characters"] == 0
        assert isinstance(data["context_breakdown"], list)
        assert isinstance(data["formality_breakdown"], list)
        assert isinstance(data["daily_activity"], list)
        assert len(data["daily_activity"]) == 7  # Last 7 days
        assert isinstance(data["hourly_activity"], list)
        assert len(data["hourly_activity"]) == 24  # 24 hours
        assert isinstance(data["day_of_week_breakdown"], list)
        assert len(data["day_of_week_breakdown"]) == 7  # 7 days
        assert isinstance(data["monthly_trends"], list)
        assert isinstance(data["word_length_distribution"], list)
        assert isinstance(data["achievements"], list)
        assert data["current_streak_days"] == 0
        assert data["longest_streak_days"] == 0
        assert "growth" in data
        assert "productivity" in data
        assert data["member_since"] is not None
        assert data["days_as_member"] >= 1

    async def test_detailed_stats_with_data(self, client, auth_headers):
        """Detailed stats with transcripts computes breakdowns correctly."""
        user = make_user(
            total_transcriptions=2, total_words=100,
            total_audio_seconds=20, typing_wpm=40,
            created_at=datetime(2026, 2, 1, tzinfo=timezone.utc),
        )
        now = datetime.now(timezone.utc)

        t1 = make_transcript(
            id=1, word_count=60, character_count=300,
            audio_duration_seconds=12, words_per_minute=120.0,
            context="email", formality="formal",
            transcript_type="input", created_at=now,
        )
        t2 = make_transcript(
            id=2, word_count=40, character_count=200,
            audio_duration_seconds=8, words_per_minute=100.0,
            context="slack", formality="casual",
            transcript_type="input", created_at=now - timedelta(hours=2),
        )

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = [t1, t2]
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.get("/api/v1/auth/stats/detailed", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["total_transcriptions"] == 2
        assert data["total_words"] == 100
        assert data["total_characters"] == 500
        assert data["transcriptions_today"] == 2
        assert data["words_today"] == 100
        assert len(data["context_breakdown"]) == 2  # email + slack
        assert len(data["formality_breakdown"]) == 2  # formal + casual
        # Growth metrics should exist
        assert data["growth"]["last_week_words"] == 100
        # Productivity insights should exist
        assert "peak_hour" in data["productivity"]

    async def test_detailed_stats_no_auth(self, client):
        """Detailed stats without auth returns 401."""
        resp = await client.get("/api/v1/auth/stats/detailed")
        assert resp.status_code == 401
