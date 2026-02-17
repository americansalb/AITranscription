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
"""
import os
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
