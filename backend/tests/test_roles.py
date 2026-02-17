"""Tests for the role design API endpoint.

Covers:
  - POST /roles/design (LLM-driven role creation)
  - Empty messages validation
  - Invalid message role validation
  - Service unavailable (RuntimeError → 502)
  - Internal error (Exception → 500)
  - Successful conversation turn (reply + no config)
  - Successful config generation (reply + role_config)
  - No auth required
"""
import pytest
from unittest.mock import AsyncMock, patch


# =============================================================================
# VALIDATION TESTS
# =============================================================================

class TestRoleDesignValidation:

    async def test_empty_messages(self, client):
        """Empty messages list returns 400."""
        response = await client.post(
            "/api/v1/roles/design",
            json={"messages": []},
        )
        assert response.status_code == 400
        assert "At least one message" in response.json()["detail"]

    async def test_invalid_message_role(self, client):
        """Message with role other than 'user'/'assistant' returns 400."""
        response = await client.post(
            "/api/v1/roles/design",
            json={
                "messages": [
                    {"role": "system", "content": "You are a role designer."}
                ]
            },
        )
        assert response.status_code == 400
        assert "Invalid message role" in response.json()["detail"]

    async def test_missing_messages_field(self, client):
        """Request without messages field returns 422."""
        response = await client.post(
            "/api/v1/roles/design",
            json={},
        )
        assert response.status_code == 422

    async def test_missing_content_field(self, client):
        """Message without content returns 422."""
        response = await client.post(
            "/api/v1/roles/design",
            json={
                "messages": [{"role": "user"}]
            },
        )
        assert response.status_code == 422


# =============================================================================
# SUCCESSFUL RESPONSES
# =============================================================================

class TestRoleDesignSuccess:

    async def test_conversation_turn_no_config(self, client):
        """LLM asks follow-up question — role_config is null."""
        mock_result = {
            "reply": "What kind of tasks should this role handle?",
            "role_config": None,
        }

        with patch("app.api.roles.design_role", new=AsyncMock(return_value=mock_result)):
            response = await client.post(
                "/api/v1/roles/design",
                json={
                    "messages": [
                        {"role": "user", "content": "I want to create a QA role"}
                    ]
                },
            )

        assert response.status_code == 200
        data = response.json()
        assert data["reply"] == "What kind of tasks should this role handle?"
        assert data["role_config"] is None

    async def test_config_generated(self, client):
        """LLM generates complete role config after conversation."""
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
            response = await client.post(
                "/api/v1/roles/design",
                json={
                    "messages": [
                        {"role": "user", "content": "Create a QA role"},
                        {"role": "assistant", "content": "What testing focus?"},
                        {"role": "user", "content": "Integration and E2E testing"},
                    ]
                },
            )

        assert response.status_code == 200
        data = response.json()
        assert data["role_config"] is not None
        config = data["role_config"]
        assert config["slug"] == "qa-engineer"
        assert config["max_instances"] == 2
        assert "testing" in config["tags"]

    async def test_no_auth_required(self, client):
        """Role design endpoint works without authentication."""
        mock_result = {
            "reply": "Tell me about the role you want to create.",
            "role_config": None,
        }

        with patch("app.api.roles.design_role", new=AsyncMock(return_value=mock_result)):
            # No auth headers
            response = await client.post(
                "/api/v1/roles/design",
                json={
                    "messages": [
                        {"role": "user", "content": "Design a developer role"}
                    ]
                },
            )

        assert response.status_code == 200

    async def test_with_project_context(self, client):
        """Project context with existing roles is passed through."""
        mock_result = {
            "reply": "I see you already have a developer role.",
            "role_config": None,
        }

        with patch("app.api.roles.design_role", new=AsyncMock(return_value=mock_result)) as mock_design:
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

    async def test_service_runtime_error(self, client):
        """RuntimeError from design_role → 502 Bad Gateway."""
        with patch("app.api.roles.design_role",
                    new=AsyncMock(side_effect=RuntimeError("API key not configured"))):
            response = await client.post(
                "/api/v1/roles/design",
                json={
                    "messages": [
                        {"role": "user", "content": "Create a role"}
                    ]
                },
            )

        assert response.status_code == 502
        assert "service unavailable" in response.json()["detail"].lower()

    async def test_unexpected_exception(self, client):
        """Unexpected exception → 500 Internal Error."""
        with patch("app.api.roles.design_role",
                    new=AsyncMock(side_effect=Exception("Unexpected error"))):
            response = await client.post(
                "/api/v1/roles/design",
                json={
                    "messages": [
                        {"role": "user", "content": "Create a role"}
                    ]
                },
            )

        assert response.status_code == 500
        assert "Internal error" in response.json()["detail"]
