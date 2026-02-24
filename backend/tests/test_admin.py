"""Tests for admin API endpoints.

Covers:
  - Permission checks (non-admin users get 403)
  - User CRUD: list, get, create, update, delete
  - User stats and transcripts
  - Global stats
  - Make admin, reset stats
  - Bootstrap and seed-admins (SECRET_KEY auth)
  - Dashboard HTML endpoint
  - Authorization: unauthenticated → 401
  - Happy-path: admin successfully performing all operations
"""
import os
import pytest
from datetime import datetime, timedelta, timezone
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
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(user, key, value)
    return user


def make_admin_user(**overrides):
    """Create a mock admin User."""
    defaults = {"is_admin": True, "tier": "developer"}
    defaults.update(overrides)
    return make_user(**defaults)


# =============================================================================
# UNAUTHENTICATED REQUESTS (401)
# =============================================================================

class TestAdminNoAuth:
    """All admin endpoints (except bootstrap/seed/dashboard) require auth."""

    async def test_list_users_no_auth(self, client):
        response = await client.get("/api/v1/admin/users")
        assert response.status_code == 401

    async def test_get_user_no_auth(self, client):
        response = await client.get("/api/v1/admin/users/1")
        assert response.status_code == 401

    async def test_create_user_no_auth(self, client):
        response = await client.post(
            "/api/v1/admin/users",
            json={"email": "new@test.com", "password": "password123"},
        )
        assert response.status_code == 401

    async def test_update_user_no_auth(self, client):
        response = await client.patch(
            "/api/v1/admin/users/1",
            json={"full_name": "Updated"},
        )
        assert response.status_code == 401

    async def test_delete_user_no_auth(self, client):
        response = await client.delete("/api/v1/admin/users/1")
        assert response.status_code == 401

    async def test_user_stats_no_auth(self, client):
        response = await client.get("/api/v1/admin/users/1/stats")
        assert response.status_code == 401

    async def test_user_transcripts_no_auth(self, client):
        response = await client.get("/api/v1/admin/users/1/transcripts")
        assert response.status_code == 401

    async def test_global_stats_no_auth(self, client):
        response = await client.get("/api/v1/admin/stats")
        assert response.status_code == 401

    async def test_make_admin_no_auth(self, client):
        response = await client.post("/api/v1/admin/make-admin/1")
        assert response.status_code == 401

    async def test_reset_all_stats_no_auth(self, client):
        response = await client.post("/api/v1/admin/reset-all-stats")
        assert response.status_code == 401

    async def test_reset_user_stats_no_auth(self, client):
        response = await client.post("/api/v1/admin/reset-user-stats/1")
        assert response.status_code == 401


# =============================================================================
# NON-ADMIN PERMISSION CHECKS (403)
# =============================================================================

class TestAdminForbiddenForNonAdmin:
    """Non-admin authenticated users get 403 on admin endpoints."""

    async def test_list_users_non_admin(self, client, auth_headers):
        """Regular user cannot list all users."""
        user = make_user(is_admin=False)
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.get(
                "/api/v1/admin/users",
                headers=auth_headers,
            )
        assert response.status_code == 403
        assert response.json()["detail"] == "Admin access required"

    async def test_get_user_non_admin(self, client, auth_headers):
        """Regular user cannot get other user details."""
        user = make_user(is_admin=False)
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.get(
                "/api/v1/admin/users/2",
                headers=auth_headers,
            )
        assert response.status_code == 403

    async def test_create_user_non_admin(self, client, auth_headers):
        """Regular user cannot create users."""
        user = make_user(is_admin=False)
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/admin/users",
                json={"email": "new@test.com", "password": "password123"},
                headers=auth_headers,
            )
        assert response.status_code == 403

    async def test_delete_user_non_admin(self, client, auth_headers):
        """Regular user cannot delete users."""
        user = make_user(is_admin=False)
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.delete(
                "/api/v1/admin/users/2",
                headers=auth_headers,
            )
        assert response.status_code == 403

    async def test_global_stats_non_admin(self, client, auth_headers):
        """Regular user cannot view global stats."""
        user = make_user(is_admin=False)
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.get(
                "/api/v1/admin/stats",
                headers=auth_headers,
            )
        assert response.status_code == 403

    async def test_make_admin_non_admin(self, client, auth_headers):
        """Regular user cannot promote others to admin."""
        user = make_user(is_admin=False)
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/admin/make-admin/2",
                headers=auth_headers,
            )
        assert response.status_code == 403

    async def test_reset_all_stats_non_admin(self, client, auth_headers):
        """Regular user cannot reset all stats."""
        user = make_user(is_admin=False)
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/admin/reset-all-stats",
                headers=auth_headers,
            )
        assert response.status_code == 403

    async def test_reset_user_stats_non_admin(self, client, auth_headers):
        """Regular user cannot reset another user's stats."""
        user = make_user(is_admin=False)
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/admin/reset-user-stats/2",
                headers=auth_headers,
            )
        assert response.status_code == 403


# =============================================================================
# BOOTSTRAP AND SEED-ADMINS (SECRET_KEY auth, no JWT)
# =============================================================================

class TestBootstrapEndpoint:
    """Tests for POST /admin/bootstrap — SECRET_KEY auth."""

    async def test_bootstrap_wrong_secret(self, client):
        """Bootstrap with wrong secret returns 403."""
        from app.core.config import settings

        with patch.object(settings, "secret_key", "correct-secret"):
            response = await client.post(
                "/api/v1/admin/bootstrap",
                json={"email": "test@example.com", "secret": "wrong-secret"},
            )
        assert response.status_code == 403
        assert response.json()["detail"] == "Invalid secret key"

    async def test_bootstrap_user_not_found(self, client):
        """Bootstrap for nonexistent user returns 404."""
        from app.core.config import settings

        with patch.object(settings, "secret_key", "test-secret"), \
             patch("app.api.admin.get_user_by_email", new=AsyncMock(return_value=None)):
            response = await client.post(
                "/api/v1/admin/bootstrap",
                json={
                    "email": "nonexistent@example.com",
                    "secret": "test-secret",
                },
            )

        assert response.status_code == 404
        assert "not found" in response.json()["detail"].lower()

    async def test_bootstrap_missing_email(self, client):
        """Bootstrap without email returns 422."""
        response = await client.post(
            "/api/v1/admin/bootstrap",
            json={"secret": "some-secret"},
        )
        assert response.status_code == 422

    async def test_bootstrap_invalid_email(self, client):
        """Bootstrap with invalid email format returns 422."""
        response = await client.post(
            "/api/v1/admin/bootstrap",
            json={"email": "not-an-email", "secret": "some-secret"},
        )
        assert response.status_code == 422


class TestSeedAdminsEndpoint:
    """Tests for POST /admin/seed-admins — SECRET_KEY auth."""

    async def test_seed_wrong_secret(self, client):
        """Seed with wrong secret returns 403."""
        from app.core.config import settings

        with patch.object(settings, "secret_key", "correct-secret"):
            response = await client.post(
                "/api/v1/admin/seed-admins",
                json={"secret": "wrong-secret"},
            )
        assert response.status_code == 403

    async def test_seed_missing_env_password(self, client):
        """Seed without ADMIN_BOOTSTRAP_PASSWORD env var returns 500."""
        from app.core.config import settings

        with patch.object(settings, "secret_key", "test-secret"), \
             patch.dict(os.environ, {}, clear=False):
            os.environ.pop("ADMIN_BOOTSTRAP_PASSWORD", None)
            response = await client.post(
                "/api/v1/admin/seed-admins",
                json={"secret": "test-secret"},
            )

        assert response.status_code == 500
        assert "ADMIN_BOOTSTRAP_PASSWORD" in response.json()["detail"]


# =============================================================================
# DASHBOARD (public HTML endpoint)
# =============================================================================

class TestAdminDashboard:
    """Tests for GET /admin/dashboard."""

    async def test_dashboard_returns_html(self, client):
        """Dashboard returns HTML page (no auth required)."""
        response = await client.get("/api/v1/admin/dashboard")
        assert response.status_code == 200
        assert "text/html" in response.headers.get("content-type", "")

    async def test_dashboard_contains_login_form(self, client):
        """Dashboard HTML includes a login mechanism."""
        response = await client.get("/api/v1/admin/dashboard")
        html = response.text
        assert "login" in html.lower() or "password" in html.lower()


# =============================================================================
# HELPER FACTORIES FOR HAPPY-PATH TESTS
# =============================================================================

def make_detailed_user(**overrides):
    """Create a mock User with all fields needed by admin response models."""
    user = MagicMock()
    defaults = {
        "id": 2,
        "email": "target@example.com",
        "full_name": "Target User",
        "tier": "standard",
        "is_active": True,
        "is_admin": False,
        "accessibility_verified": False,
        "daily_transcription_limit": 0,
        "daily_transcriptions_used": 0,
        "total_transcriptions": 5,
        "total_words": 250,
        "total_audio_seconds": 60,
        "total_polish_tokens": 500,
        "typing_wpm": 40,
        "created_at": datetime(2026, 1, 15, tzinfo=timezone.utc),
        "updated_at": datetime(2026, 2, 1, tzinfo=timezone.utc),
        "last_usage_reset": None,
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(user, key, value)
    return user


def make_transcript(**overrides):
    """Create a mock Transcript for admin tests."""
    t = MagicMock()
    now = datetime.now(timezone.utc)
    defaults = {
        "id": 1,
        "user_id": 2,
        "raw_text": "hello world",
        "polished_text": "Hello, world.",
        "word_count": 2,
        "character_count": 13,
        "audio_duration_seconds": 3.0,
        "words_per_minute": 40.0,
        "context": "general",
        "formality": "neutral",
        "created_at": now,
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(t, key, value)
    return t


# =============================================================================
# ADMIN HAPPY-PATH TESTS
# =============================================================================

class TestAdminListUsers:
    """Happy-path tests for GET /admin/users."""

    async def test_list_users_success(self, client, auth_headers):
        """Admin can list all users."""
        admin = make_admin_user()
        user1 = make_detailed_user(id=1, email="user1@example.com")
        user2 = make_detailed_user(id=2, email="user2@example.com")

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = [user1, user2]
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/users", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert len(data) == 2
        assert data[0]["email"] == "user1@example.com"
        assert data[1]["email"] == "user2@example.com"

    async def test_list_users_empty(self, client, auth_headers):
        """Admin gets empty list when no users."""
        admin = make_admin_user()

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = []
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/users", headers=auth_headers)

        assert resp.status_code == 200
        assert resp.json() == []


class TestAdminGetUser:
    """Happy-path tests for GET /admin/users/{id}."""

    async def test_get_user_success(self, client, auth_headers):
        """Admin can get user details."""
        admin = make_admin_user()
        target = make_detailed_user(id=2, email="target@example.com")

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = target
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/users/2", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["id"] == 2
        assert data["email"] == "target@example.com"
        assert "accessibility_verified" in data

    async def test_get_user_not_found(self, client, auth_headers):
        """Admin gets 404 for nonexistent user."""
        admin = make_admin_user()

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = None
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/users/999", headers=auth_headers)

        assert resp.status_code == 404
        assert "not found" in resp.json()["detail"].lower()


class TestAdminDeleteUser:
    """Happy-path tests for DELETE /admin/users/{id}."""

    async def test_delete_user_success(self, client, auth_headers):
        """Admin can delete a non-admin user."""
        admin = make_admin_user()
        target = make_detailed_user(id=2, is_admin=False)

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = target
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.delete("/api/v1/admin/users/2", headers=auth_headers)

        assert resp.status_code == 204

    async def test_delete_admin_user_blocked(self, client, auth_headers):
        """Cannot delete an admin user — returns 400."""
        admin = make_admin_user()
        target = make_detailed_user(id=2, is_admin=True)

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = target
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.delete("/api/v1/admin/users/2", headers=auth_headers)

        assert resp.status_code == 400
        assert "admin" in resp.json()["detail"].lower()

    async def test_delete_user_not_found(self, client, auth_headers):
        """Delete nonexistent user returns 404."""
        admin = make_admin_user()

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = None
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.delete("/api/v1/admin/users/999", headers=auth_headers)

        assert resp.status_code == 404


class TestAdminMakeAdmin:
    """Happy-path tests for POST /admin/make-admin/{id} — CRITICAL."""

    async def test_make_admin_success(self, client, auth_headers):
        """Admin can promote a user to admin."""
        admin = make_admin_user()
        target = make_detailed_user(id=2, email="promote@example.com", is_admin=False)

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = target
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.post("/api/v1/admin/make-admin/2", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert "admin" in data["message"].lower()
        assert target.is_admin is True

    async def test_make_admin_not_found(self, client, auth_headers):
        """Make-admin for nonexistent user returns 404."""
        admin = make_admin_user()

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = None
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.post("/api/v1/admin/make-admin/999", headers=auth_headers)

        assert resp.status_code == 404

    async def test_make_admin_already_admin(self, client, auth_headers):
        """Make-admin on already-admin user still succeeds (idempotent)."""
        admin = make_admin_user()
        target = make_detailed_user(id=2, email="already@example.com", is_admin=True)

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = target
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.post("/api/v1/admin/make-admin/2", headers=auth_headers)

        assert resp.status_code == 200
        assert target.is_admin is True


class TestAdminUserStats:
    """Happy-path tests for GET /admin/users/{id}/stats — CRITICAL."""

    async def test_user_stats_success(self, client, auth_headers):
        """Admin can get user stats with transcript data."""
        admin = make_admin_user()
        target = make_detailed_user(
            id=2, email="stats@example.com",
            total_transcriptions=3, total_words=150, total_audio_seconds=30,
        )
        now = datetime.now(timezone.utc)
        t1 = make_transcript(
            word_count=60, character_count=300,
            audio_duration_seconds=12, words_per_minute=120.0, created_at=now,
        )
        t2 = make_transcript(
            word_count=50, character_count=250,
            audio_duration_seconds=10, words_per_minute=100.0, created_at=now,
        )
        t3 = make_transcript(
            word_count=40, character_count=200,
            audio_duration_seconds=8, words_per_minute=80.0,
            created_at=now - timedelta(days=10),
        )

        mock_db = client._mock_db
        call_count = 0

        async def execute_side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            result = MagicMock()
            if call_count == 1:
                result.scalar_one_or_none.return_value = target
            elif call_count == 2:
                result.scalars.return_value.all.return_value = [t1, t2, t3]
            return result

        mock_db.execute = AsyncMock(side_effect=execute_side_effect)

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/users/2/stats", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["user_id"] == 2
        assert data["email"] == "stats@example.com"
        assert data["total_transcriptions"] == 3
        assert data["total_words"] == 150
        assert data["total_characters"] == 750  # 300 + 250 + 200
        assert data["average_words_per_minute"] == 100.0  # (120+100+80)/3

    async def test_user_stats_not_found(self, client, auth_headers):
        """User stats for nonexistent user returns 404."""
        admin = make_admin_user()

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = None
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/users/999/stats", headers=auth_headers)

        assert resp.status_code == 404

    async def test_user_stats_no_transcripts(self, client, auth_headers):
        """User stats with no transcripts returns zeroed values."""
        admin = make_admin_user()
        target = make_detailed_user(
            id=2, email="empty@example.com",
            total_transcriptions=0, total_words=0, total_audio_seconds=0,
        )

        mock_db = client._mock_db
        call_count = 0

        async def execute_side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            result = MagicMock()
            if call_count == 1:
                result.scalar_one_or_none.return_value = target
            elif call_count == 2:
                result.scalars.return_value.all.return_value = []
            return result

        mock_db.execute = AsyncMock(side_effect=execute_side_effect)

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/users/2/stats", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["total_characters"] == 0
        assert data["average_words_per_minute"] == 0
        assert data["transcriptions_today"] == 0


class TestAdminUserTranscripts:
    """Happy-path tests for GET /admin/users/{id}/transcripts."""

    async def test_user_transcripts_success(self, client, auth_headers):
        """Admin can get a user's transcripts."""
        admin = make_admin_user()
        target = make_detailed_user(id=2)
        now = datetime.now(timezone.utc)
        t1 = make_transcript(id=10, raw_text="hello", polished_text="Hello.", created_at=now)
        t2 = make_transcript(id=11, raw_text="world", polished_text="World.", created_at=now)

        mock_db = client._mock_db
        call_count = 0

        async def execute_side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            result = MagicMock()
            if call_count == 1:
                result.scalar_one_or_none.return_value = target
            elif call_count == 2:
                result.scalars.return_value.all.return_value = [t1, t2]
            return result

        mock_db.execute = AsyncMock(side_effect=execute_side_effect)

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/users/2/transcripts", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert len(data) == 2
        assert data[0]["id"] == 10
        assert data[0]["raw_text"] == "hello"

    async def test_user_transcripts_not_found(self, client, auth_headers):
        """Transcripts for nonexistent user returns 404."""
        admin = make_admin_user()

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = None
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/users/999/transcripts", headers=auth_headers)

        assert resp.status_code == 404


class TestAdminGlobalStats:
    """Happy-path tests for GET /admin/stats — CRITICAL."""

    async def test_global_stats_success(self, client, auth_headers):
        """Admin gets global system statistics."""
        admin = make_admin_user()

        mock_db = client._mock_db
        call_count = 0

        tier_standard = MagicMock()
        tier_standard.value = "standard"
        tier_developer = MagicMock()
        tier_developer.value = "developer"

        async def execute_side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            result = MagicMock()
            if call_count == 1:
                result.scalar.return_value = 10  # total_users
            elif call_count == 2:
                result.scalar.return_value = 8  # active_users
            elif call_count == 3:
                result.one.return_value = (50, 2500, 3600)  # totals
            elif call_count == 4:
                result.scalar.return_value = 5  # transcriptions_today
            elif call_count == 5:
                result.scalar.return_value = 25  # transcriptions_this_week
            elif call_count == 6:
                result.all.return_value = [
                    (tier_standard, 8),
                    (tier_developer, 2),
                ]
            return result

        mock_db.execute = AsyncMock(side_effect=execute_side_effect)

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/stats", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["total_users"] == 10
        assert data["active_users"] == 8
        assert data["total_transcriptions"] == 50
        assert data["total_words"] == 2500
        assert data["total_audio_hours"] == 1.0  # 3600 / 3600
        assert data["transcriptions_today"] == 5
        assert data["transcriptions_this_week"] == 25
        assert data["users_by_tier"]["standard"] == 8
        assert data["users_by_tier"]["developer"] == 2

    async def test_global_stats_empty_system(self, client, auth_headers):
        """Global stats on empty system returns zeroed values."""
        admin = make_admin_user()

        mock_db = client._mock_db
        call_count = 0

        async def execute_side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            result = MagicMock()
            if call_count == 1:
                result.scalar.return_value = 0
            elif call_count == 2:
                result.scalar.return_value = 0
            elif call_count == 3:
                result.one.return_value = (None, None, None)
            elif call_count == 4:
                result.scalar.return_value = 0
            elif call_count == 5:
                result.scalar.return_value = 0
            elif call_count == 6:
                result.all.return_value = []
            return result

        mock_db.execute = AsyncMock(side_effect=execute_side_effect)

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.get("/api/v1/admin/stats", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["total_users"] == 0
        assert data["total_transcriptions"] == 0
        assert data["total_audio_hours"] == 0.0
        assert data["users_by_tier"] == {}


class TestAdminResetAllStats:
    """Happy-path tests for POST /admin/reset-all-stats — CRITICAL."""

    async def test_reset_all_stats_success(self, client, auth_headers):
        """Admin can reset all user stats from transcript history."""
        admin = make_admin_user()
        now = datetime.now(timezone.utc)

        user1 = make_detailed_user(
            id=1, email="u1@test.com",
            total_transcriptions=100, total_words=5000,
        )
        user2 = make_detailed_user(
            id=2, email="u2@test.com",
            total_transcriptions=50, total_words=2500,
        )

        t1 = make_transcript(word_count=30, audio_duration_seconds=5, created_at=now)
        t2 = make_transcript(word_count=20, audio_duration_seconds=3, created_at=now)

        mock_db = client._mock_db
        call_count = 0

        async def execute_side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            result = MagicMock()
            if call_count == 1:
                result.scalars.return_value.all.return_value = [user1, user2]
            elif call_count == 2:
                result.scalars.return_value.all.return_value = [t1, t2]
            elif call_count == 3:
                result.scalars.return_value.all.return_value = [t1]
            return result

        mock_db.execute = AsyncMock(side_effect=execute_side_effect)

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.post("/api/v1/admin/reset-all-stats", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["users_reset"] == 2
        assert "reset" in data["message"].lower()
        # Verify user stats were recalculated from transcripts
        assert user1.total_transcriptions == 2
        assert user1.total_words == 50  # 30 + 20
        assert user1.total_audio_seconds == 8  # int(5) + int(3)
        assert user1.daily_transcriptions_used == 0
        assert user2.total_transcriptions == 1
        assert user2.total_words == 30
        mock_db.commit.assert_awaited()


class TestAdminResetUserStats:
    """Happy-path tests for POST /admin/reset-user-stats/{id} — CRITICAL."""

    async def test_reset_user_stats_success(self, client, auth_headers):
        """Admin can reset a single user's stats from transcript history."""
        admin = make_admin_user()
        now = datetime.now(timezone.utc)
        target = make_detailed_user(
            id=2, email="reset-me@test.com",
            total_transcriptions=999, total_words=99999,
        )
        t1 = make_transcript(word_count=50, audio_duration_seconds=10, created_at=now)
        t2 = make_transcript(word_count=30, audio_duration_seconds=6, created_at=now)

        mock_db = client._mock_db
        call_count = 0

        async def execute_side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            result = MagicMock()
            if call_count == 1:
                result.scalar_one_or_none.return_value = target
            elif call_count == 2:
                result.scalars.return_value.all.return_value = [t1, t2]
            return result

        mock_db.execute = AsyncMock(side_effect=execute_side_effect)

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.post("/api/v1/admin/reset-user-stats/2", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert "reset" in data["message"].lower()
        assert data["stats"]["total_transcriptions"] == 2
        assert data["stats"]["total_words"] == 80  # 50 + 30
        assert data["stats"]["total_audio_seconds"] == 16  # int(10) + int(6)
        assert target.total_transcriptions == 2
        assert target.daily_transcriptions_used == 0
        mock_db.commit.assert_awaited()

    async def test_reset_user_stats_not_found(self, client, auth_headers):
        """Reset stats for nonexistent user returns 404."""
        admin = make_admin_user()

        mock_db = client._mock_db
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = None
        mock_db.execute.return_value = mock_result

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.post("/api/v1/admin/reset-user-stats/999", headers=auth_headers)

        assert resp.status_code == 404

    async def test_reset_user_stats_no_transcripts(self, client, auth_headers):
        """Reset stats for user with no transcripts zeros everything."""
        admin = make_admin_user()
        target = make_detailed_user(
            id=2, email="no-data@test.com",
            total_transcriptions=5, total_words=250,
        )

        mock_db = client._mock_db
        call_count = 0

        async def execute_side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            result = MagicMock()
            if call_count == 1:
                result.scalar_one_or_none.return_value = target
            elif call_count == 2:
                result.scalars.return_value.all.return_value = []
            return result

        mock_db.execute = AsyncMock(side_effect=execute_side_effect)

        with patch("app.api.auth.get_user_by_id", return_value=admin):
            resp = await client.post("/api/v1/admin/reset-user-stats/2", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["stats"]["total_transcriptions"] == 0
        assert data["stats"]["total_words"] == 0
        assert data["stats"]["total_audio_seconds"] == 0
