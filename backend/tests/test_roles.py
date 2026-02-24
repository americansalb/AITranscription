"""Tests for the role design API endpoint.

Covers:
  - POST /roles/design (LLM-driven role creation)
  - Empty messages validation
  - Invalid message role validation
  - Service unavailable (RuntimeError → 502)
  - Internal error (Exception → 500)
  - Successful conversation turn (reply + no config)
  - Successful config generation (reply + role_config)
  - Authentication required
"""
import pytest
from unittest.mock import AsyncMock, patch, MagicMock


# =============================================================================
# VALIDATION TESTS
# =============================================================================

class TestRoleDesignValidation:

    async def test_empty_messages(self, client, auth_headers):
        """Empty messages list returns 400."""
        response = await client.post(
            "/api/v1/roles/design",
            json={"messages": []},
            headers=auth_headers,
        )
        assert response.status_code == 400
        assert "At least one message" in response.json()["detail"]

    async def test_invalid_message_role(self, client, auth_headers):
        """Message with role other than 'user'/'assistant' returns 400."""
        response = await client.post(
            "/api/v1/roles/design",
            json={
                "messages": [
                    {"role": "system", "content": "You are a role designer."}
                ]
            },
            headers=auth_headers,
        )
        assert response.status_code == 400
        assert "Invalid message role" in response.json()["detail"]

    async def test_missing_messages_field(self, client, auth_headers):
        """Request without messages field returns 422."""
        response = await client.post(
            "/api/v1/roles/design",
            json={},
            headers=auth_headers,
        )
        assert response.status_code == 422

    async def test_missing_content_field(self, client, auth_headers):
        """Message without content returns 422."""
        response = await client.post(
            "/api/v1/roles/design",
            json={
                "messages": [{"role": "user"}]
            },
            headers=auth_headers,
        )
        assert response.status_code == 422


# =============================================================================
# SUCCESSFUL RESPONSES
# =============================================================================

class TestRoleDesignSuccess:

    async def test_conversation_turn_no_config(self, client, auth_headers):
        """LLM asks follow-up question — role_config is null."""
        from tests.conftest import make_user
        user = make_user()

        mock_result = {
            "reply": "What kind of tasks should this role handle?",
            "role_config": None,
        }

        with patch("app.api.roles.design_role", new=AsyncMock(return_value=mock_result)):
            with patch("app.api.auth.get_user_by_id", new=AsyncMock(return_value=user)):
                response = await client.post(
                    "/api/v1/roles/design",
                    json={
                        "messages": [
                            {"role": "user", "content": "I want to create a QA role"}
                        ]
                    },
                    headers=auth_headers,
                )

        assert response.status_code == 200
        data = response.json()
        assert data["reply"] == "What kind of tasks should this role handle?"
        assert data["role_config"] is None

    async def test_config_generated(self, client, auth_headers):
        """LLM generates complete role config after conversation."""
        from tests.conftest import make_user
        user = make_user()

        mock_result = {
            "reply": "Here's the role configuration I designed:",
            "role_config": {
                "title": "QA Engineer",
                "slug": "qa-engineer",
                "description": "Quality assurance and testing specialist",
                "tags": ["testing", "code-review"],
                "permissions": ["status", "review"],
                "max_instances": 2,
                "briefing": "# QA Engineer\n\nYou are the QA Engineer...",
            },
        }

        with patch("app.api.roles.design_role", new=AsyncMock(return_value=mock_result)):
            with patch("app.api.auth.get_user_by_id", new=AsyncMock(return_value=user)):
                response = await client.post(
                    "/api/v1/roles/design",
                    json={
                        "messages": [
                            {"role": "user", "content": "Create a QA role"},
                            {"role": "assistant", "content": "What testing focus?"},
                            {"role": "user", "content": "Integration and E2E testing"},
                        ]
                    },
                    headers=auth_headers,
                )

        assert response.status_code == 200
        data = response.json()
        assert data["role_config"] is not None
        config = data["role_config"]
        assert config["slug"] == "qa-engineer"
        assert config["max_instances"] == 2
        assert "testing" in config["tags"]

    async def test_auth_required(self, client):
        """Role design endpoint requires authentication."""
        response = await client.post(
            "/api/v1/roles/design",
            json={
                "messages": [
                    {"role": "user", "content": "Design a developer role"}
                ]
            },
        )

        assert response.status_code == 403

    async def test_with_project_context(self, client, auth_headers):
        """Project context with existing roles is passed through."""
        from tests.conftest import make_user
        user = make_user()

        mock_result = {
            "reply": "I see you already have a developer role.",
            "role_config": None,
        }

        with patch("app.api.roles.design_role", new=AsyncMock(return_value=mock_result)) as mock_design:
            with patch("app.api.auth.get_user_by_id", new=AsyncMock(return_value=user)):
                response = await client.post(
                    "/api/v1/roles/design",
                    json={
                        "messages": [
                            {"role": "user", "content": "Create a tester role"}
                        ],
                        "project_context": {
                            "roles": {
                                "developer": {
                                    "title": "Developer",
                                    "description": "Writes code",
                                    "tags": ["implementation"],
                                    "permissions": ["status"],
                                    "max_instances": 3,
                                }
                            }
                        },
                    },
                    headers=auth_headers,
                )

        assert response.status_code == 200
        # Verify project_context was passed to the service
        mock_design.assert_called_once()
        call_args = mock_design.call_args
        assert "roles" in call_args.kwargs.get("project_context", {}) or \
               (len(call_args.args) > 1 and "roles" in call_args.args[1])


# =============================================================================
# ERROR HANDLING
# =============================================================================

class TestRoleDesignErrors:

    async def test_service_runtime_error(self, client, auth_headers):
        """RuntimeError from design_role returns graceful response."""
        from tests.conftest import make_user
        user = make_user()

        with patch("app.api.roles.design_role",
                    new=AsyncMock(side_effect=RuntimeError("API key not configured"))):
            with patch("app.api.auth.get_user_by_id", new=AsyncMock(return_value=user)):
                response = await client.post(
                    "/api/v1/roles/design",
                    json={
                        "messages": [
                            {"role": "user", "content": "Create a role"}
                        ]
                    },
                    headers=auth_headers,
                )

        assert response.status_code == 200
        data = response.json()
        assert "trouble connecting" in data["reply"].lower()
        assert data["role_config"] is None

    async def test_unexpected_exception(self, client, auth_headers):
        """Unexpected exception returns graceful response."""
        from tests.conftest import make_user
        user = make_user()

        with patch("app.api.roles.design_role",
                    new=AsyncMock(side_effect=Exception("Unexpected error"))):
            with patch("app.api.auth.get_user_by_id", new=AsyncMock(return_value=user)):
                response = await client.post(
                    "/api/v1/roles/design",
                    json={
                        "messages": [
                            {"role": "user", "content": "Create a role"}
                        ]
                    },
                    headers=auth_headers,
                )

        assert response.status_code == 200
        data = response.json()
        assert "trouble connecting" in data["reply"].lower()
        assert data["role_config"] is None
