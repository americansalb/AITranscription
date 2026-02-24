"""Tests for gamification API endpoints.

Previously only the service layer had tests. This file tests the HTTP endpoints
to verify auth, error handling, and response shape.

Covers:
  - GET /gamification/progress — auth required, service failure handling
  - GET /gamification/transactions — auth, limit bounds
  - POST /gamification/check — auth, graceful degradation on service failure
  - GET /gamification/achievements — auth, category/rarity validation, pagination
  - GET /gamification/achievements/unnotified — auth
  - POST /gamification/achievements/mark-notified — auth, commit
  - GET /gamification/leaderboard — auth, metric validation
  - GET /gamification/categories — public (no auth)
"""
import pytest
from unittest.mock import AsyncMock, MagicMock, patch


def make_user(**overrides):
    """Create a mock User for auth."""
    user = MagicMock()
    defaults = {
        "id": 1,
        "email": "test@example.com",
        "full_name": "Test User",
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


MOCK_PROGRESS = {
    "user_id": 1,
    "current_level": 5,
    "current_xp": 300,
    "xp_to_next_level": 250,
    "level_progress": 0.55,
    "lifetime_xp": 2000,
    "prestige_tier": "specialist",
    "tier_color": "#3b82f6",
    "tier_progress": {
        "current_tier": "specialist",
        "next_tier": "master",
        "tier_start_xp": 1000,
        "tier_end_xp": 10000,
        "xp_in_tier": 1000,
        "progress": 0.11,
        "color": "#3b82f6",
    },
    "xp_multiplier": 1.0,
    "achievements": {
        "unlocked": 5,
        "total": 1000,
        "progress": 0.005,
        "by_rarity": {"common": 3, "uncommon": 2},
    },
    "last_xp_earned_at": "2026-02-24T10:00:00Z",
}


# =============================================================================
# AUTH CHECKS (401)
# =============================================================================

class TestGamificationNoAuth:
    """All gamification endpoints (except categories) require auth."""

    async def test_progress_no_auth(self, client):
        response = await client.get("/api/v1/gamification/progress")
        assert response.status_code == 401

    async def test_transactions_no_auth(self, client):
        response = await client.get("/api/v1/gamification/transactions")
        assert response.status_code == 401

    async def test_check_no_auth(self, client):
        response = await client.post("/api/v1/gamification/check")
        assert response.status_code == 401

    async def test_achievements_no_auth(self, client):
        response = await client.get("/api/v1/gamification/achievements")
        assert response.status_code == 401

    async def test_unnotified_no_auth(self, client):
        response = await client.get("/api/v1/gamification/achievements/unnotified")
        assert response.status_code == 401

    async def test_mark_notified_no_auth(self, client):
        response = await client.post(
            "/api/v1/gamification/achievements/mark-notified",
            json={"achievement_ids": ["test-1"]},
        )
        assert response.status_code == 401

    async def test_leaderboard_no_auth(self, client):
        response = await client.get("/api/v1/gamification/leaderboard")
        assert response.status_code == 401

    async def test_categories_public(self, client):
        """Categories endpoint does NOT require auth."""
        response = await client.get("/api/v1/gamification/categories")
        assert response.status_code == 200
        data = response.json()
        assert "categories" in data
        assert "rarities" in data
        assert len(data["categories"]) > 0
        assert len(data["rarities"]) > 0


# =============================================================================
# PROGRESS ENDPOINT
# =============================================================================

class TestProgressEndpoint:

    async def test_progress_success(self, client, auth_headers):
        """GET /progress returns full gamification state."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.gamification.GamificationService.get_user_progress",
                   return_value=MOCK_PROGRESS):
            response = await client.get(
                "/api/v1/gamification/progress",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["current_level"] == 5
        assert data["lifetime_xp"] == 2000
        assert data["prestige_tier"] == "specialist"
        assert "tier_progress" in data
        assert "achievements" in data

    @pytest.mark.xfail(reason="BUG: /progress has no try/except — unhandled exception crashes request")
    async def test_progress_service_failure_graceful(self, client, auth_headers):
        """GET /progress SHOULD return graceful degradation when service fails.

        Currently the endpoint has no error handling. The exception propagates
        as an unhandled 500 with a stack trace. The /check endpoint shows the
        correct pattern: try/except → return empty data.

        This test will PASS once error handling is added to the endpoint.
        """
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.gamification.GamificationService.get_user_progress",
                   side_effect=Exception("DB connection lost")):
            response = await client.get(
                "/api/v1/gamification/progress",
                headers=auth_headers,
            )

        # When fixed, this should return 200 with default/empty progress data
        assert response.status_code == 200


# =============================================================================
# TRANSACTIONS ENDPOINT
# =============================================================================

class TestTransactionsEndpoint:

    async def test_transactions_success(self, client, auth_headers):
        """GET /transactions returns list of XP events."""
        user = make_user()
        mock_transactions = [
            {
                "id": 1, "amount": 50, "final_amount": 50, "multiplier": 1.0,
                "source": "transcription", "source_id": "123",
                "description": "Transcription XP", "level_before": 4,
                "level_after": 5, "created_at": "2026-02-24T10:00:00Z",
            }
        ]
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.gamification.GamificationService.get_recent_xp_transactions",
                   return_value=mock_transactions):
            response = await client.get(
                "/api/v1/gamification/transactions?limit=10",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert len(data) == 1
        assert data[0]["source"] == "transcription"

    async def test_transactions_limit_bounds(self, client, auth_headers):
        """Limit parameter is validated (1-100)."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            # Too high
            response = await client.get(
                "/api/v1/gamification/transactions?limit=999",
                headers=auth_headers,
            )
            assert response.status_code == 422

            # Too low
            response = await client.get(
                "/api/v1/gamification/transactions?limit=0",
                headers=auth_headers,
            )
            assert response.status_code == 422


# =============================================================================
# CHECK ACHIEVEMENTS ENDPOINT
# =============================================================================

class TestCheckAchievements:

    async def test_check_no_new_achievements(self, client, auth_headers):
        """POST /check when no new achievements returns empty list."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.gamification.AchievementService.check_achievements",
                   return_value=[]):
            response = await client.post(
                "/api/v1/gamification/check",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["achievements"] == []
        assert data["xp_gained"] == 0
        assert data["level_changes"] is None

    async def test_check_graceful_on_service_failure(self, client, auth_headers):
        """POST /check degrades gracefully when service fails (has try/except)."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.gamification.AchievementService.check_achievements",
                   side_effect=Exception("DB error")):
            response = await client.post(
                "/api/v1/gamification/check",
                headers=auth_headers,
            )

        # This endpoint DOES have try/except — should return 200 with empty data
        assert response.status_code == 200
        data = response.json()
        assert data["achievements"] == []
        assert data["xp_gained"] == 0


# =============================================================================
# ACHIEVEMENTS LIST ENDPOINT
# =============================================================================

class TestAchievementsEndpoint:

    async def test_achievements_success(self, client, auth_headers):
        """GET /achievements returns paginated list."""
        user = make_user()
        mock_result = {
            "achievements": [],
            "total": 0,
            "page": 1,
            "page_size": 50,
            "total_pages": 0,
        }
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.gamification.AchievementService.get_achievements",
                   return_value=mock_result):
            response = await client.get(
                "/api/v1/gamification/achievements",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert "achievements" in data
        assert "total" in data
        assert "page" in data

    async def test_achievements_invalid_category(self, client, auth_headers):
        """Invalid category returns 400."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.get(
                "/api/v1/gamification/achievements?category=nonexistent",
                headers=auth_headers,
            )
        assert response.status_code == 400
        assert "Invalid category" in response.json()["detail"]

    async def test_achievements_invalid_rarity(self, client, auth_headers):
        """Invalid rarity returns 400."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.get(
                "/api/v1/gamification/achievements?rarity=mythical",
                headers=auth_headers,
            )
        assert response.status_code == 400
        assert "Invalid rarity" in response.json()["detail"]


# =============================================================================
# LEADERBOARD ENDPOINT
# =============================================================================

class TestLeaderboardEndpoint:

    async def test_leaderboard_success(self, client, auth_headers):
        """GET /leaderboard returns ranked list."""
        user = make_user()
        mock_leaderboard = [
            {
                "rank": 1, "user_id": 2, "display_name": "TopUser",
                "level": 20, "tier": "master", "achievements": 50,
                "lifetime_xp": 50000,
            }
        ]
        mock_rank = {"rank": 5, "user_id": 1, "display_name": "Test User"}
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.gamification.LeaderboardService.get_leaderboard",
                   return_value=mock_leaderboard), \
             patch("app.services.gamification.LeaderboardService.get_user_rank",
                   return_value=mock_rank):
            response = await client.get(
                "/api/v1/gamification/leaderboard",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert "leaderboard" in data
        assert len(data["leaderboard"]) == 1
        assert data["leaderboard"][0]["rank"] == 1
        assert data["user_rank"]["rank"] == 5

    async def test_leaderboard_invalid_metric(self, client, auth_headers):
        """Invalid metric returns 422."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.get(
                "/api/v1/gamification/leaderboard?metric=invalid_metric",
                headers=auth_headers,
            )
        assert response.status_code == 422


# =============================================================================
# MARK NOTIFIED ENDPOINT
# =============================================================================

class TestMarkNotifiedEndpoint:

    async def test_mark_notified_success(self, client, auth_headers):
        """POST /achievements/mark-notified succeeds."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.gamification.AchievementService.mark_achievements_notified",
                   return_value=None):
            response = await client.post(
                "/api/v1/gamification/achievements/mark-notified",
                json={"achievement_ids": ["speed-1", "words-2"]},
                headers=auth_headers,
            )

        assert response.status_code == 200
        assert response.json()["status"] == "ok"

    async def test_mark_notified_empty_list(self, client, auth_headers):
        """Empty achievement_ids list should still succeed."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.gamification.AchievementService.mark_achievements_notified",
                   return_value=None):
            response = await client.post(
                "/api/v1/gamification/achievements/mark-notified",
                json={"achievement_ids": []},
                headers=auth_headers,
            )

        assert response.status_code == 200
