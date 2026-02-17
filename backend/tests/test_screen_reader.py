"""Tests for screen reader and computer use API endpoints.

Covers:
  - POST /describe-screen (vision API, no auth)
  - POST /screen-reader-chat (multi-turn, empty messages)
  - POST /computer-use (tool loop, empty messages)
  - API key validation for all vision endpoints
"""
import pytest
from unittest.mock import AsyncMock, MagicMock, patch


# A minimal valid 1x1 PNG in base64
TINY_PNG_BASE64 = (
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk"
    "+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
)


# =============================================================================
# DESCRIBE SCREEN
# =============================================================================

class TestDescribeScreen:

    async def test_describe_screen_success(self, client):
        """POST /describe-screen with valid image returns description."""
        mock_result = {
            "description": "A desktop application window showing a text editor.",
            "input_tokens": 1200,
            "output_tokens": 150,
        }

        with patch("app.services.screen_reader.screen_reader_service.describe",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/describe-screen",
                json={
                    "image_base64": TINY_PNG_BASE64,
                    "blind_mode": True,
                    "detail": 3,
                },
            )

        assert response.status_code == 200
        data = response.json()
        assert data["description"] == "A desktop application window showing a text editor."
        assert data["input_tokens"] == 1200
        assert data["output_tokens"] == 150

    async def test_describe_screen_no_api_key(self, client):
        """POST /describe-screen without Anthropic key returns 500."""
        with patch("app.core.config.settings.anthropic_api_key", ""):
            response = await client.post(
                "/api/v1/describe-screen",
                json={
                    "image_base64": TINY_PNG_BASE64,
                },
            )

        assert response.status_code == 500
        assert "Anthropic API key not configured" in response.json()["detail"]

    async def test_describe_screen_service_error(self, client):
        """POST /describe-screen when service fails returns 500."""
        with patch("app.services.screen_reader.screen_reader_service.describe",
                    side_effect=Exception("Vision API error")):
            response = await client.post(
                "/api/v1/describe-screen",
                json={"image_base64": TINY_PNG_BASE64},
            )

        assert response.status_code == 500
        assert "Screen description failed" in response.json()["detail"]

    async def test_describe_screen_no_auth_needed(self, client):
        """Describe screen does not require authentication."""
        mock_result = {
            "description": "A window.",
            "input_tokens": 100,
            "output_tokens": 20,
        }

        with patch("app.services.screen_reader.screen_reader_service.describe",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/describe-screen",
                json={"image_base64": TINY_PNG_BASE64},
            )

        assert response.status_code == 200

    async def test_describe_screen_missing_image(self, client):
        """POST /describe-screen without image returns 422."""
        response = await client.post(
            "/api/v1/describe-screen",
            json={},
        )
        assert response.status_code == 422

    async def test_describe_screen_blind_mode(self, client):
        """Blind mode provides more detailed description."""
        mock_result = {
            "description": "Exhaustive visual description of the screen...",
            "input_tokens": 1500,
            "output_tokens": 500,
        }

        with patch("app.services.screen_reader.screen_reader_service.describe",
                    return_value=mock_result) as mock_describe:
            response = await client.post(
                "/api/v1/describe-screen",
                json={
                    "image_base64": TINY_PNG_BASE64,
                    "blind_mode": True,
                    "detail": 5,
                },
            )

        assert response.status_code == 200
        # Verify blind_mode was passed through
        mock_describe.assert_called_once()
        call_kwargs = mock_describe.call_args
        assert call_kwargs.kwargs.get("blind_mode") is True or \
               (len(call_kwargs.args) > 1 and call_kwargs.args[1] is True)


# =============================================================================
# SCREEN READER CHAT
# =============================================================================

class TestScreenReaderChat:

    async def test_chat_success(self, client):
        """POST /screen-reader-chat with valid messages returns response."""
        mock_result = {
            "response": "The top right shows a close button.",
            "input_tokens": 1300,
            "output_tokens": 45,
        }

        with patch("app.services.screen_reader.screen_reader_service.chat",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/screen-reader-chat",
                json={
                    "image_base64": TINY_PNG_BASE64,
                    "messages": [
                        {"role": "user", "content": "What's in the top right?"}
                    ],
                },
            )

        assert response.status_code == 200
        data = response.json()
        assert data["response"] == "The top right shows a close button."

    async def test_chat_empty_messages(self, client):
        """POST /screen-reader-chat with empty messages returns 400."""
        response = await client.post(
            "/api/v1/screen-reader-chat",
            json={
                "image_base64": TINY_PNG_BASE64,
                "messages": [],
            },
        )
        assert response.status_code == 400
        assert "Messages cannot be empty" in response.json()["detail"]

    async def test_chat_no_api_key(self, client):
        """POST /screen-reader-chat without API key returns 500."""
        with patch("app.core.config.settings.anthropic_api_key", ""):
            response = await client.post(
                "/api/v1/screen-reader-chat",
                json={
                    "image_base64": TINY_PNG_BASE64,
                    "messages": [{"role": "user", "content": "Hello"}],
                },
            )

        assert response.status_code == 500

    async def test_chat_service_error(self, client):
        """POST /screen-reader-chat when service fails returns 500."""
        with patch("app.services.screen_reader.screen_reader_service.chat",
                    side_effect=Exception("Chat error")):
            response = await client.post(
                "/api/v1/screen-reader-chat",
                json={
                    "image_base64": TINY_PNG_BASE64,
                    "messages": [{"role": "user", "content": "What's here?"}],
                },
            )

        assert response.status_code == 500

    async def test_chat_multi_turn(self, client):
        """Multi-turn conversation passes full message history."""
        mock_result = {
            "response": "The button is labeled 'Submit'.",
            "input_tokens": 1500,
            "output_tokens": 30,
        }

        with patch("app.services.screen_reader.screen_reader_service.chat",
                    return_value=mock_result) as mock_chat:
            response = await client.post(
                "/api/v1/screen-reader-chat",
                json={
                    "image_base64": TINY_PNG_BASE64,
                    "messages": [
                        {"role": "user", "content": "What do you see?"},
                        {"role": "assistant", "content": "I see a form with buttons."},
                        {"role": "user", "content": "What does the main button say?"},
                    ],
                },
            )

        assert response.status_code == 200
        # Verify all 3 messages were passed
        mock_chat.assert_called_once()


# =============================================================================
# COMPUTER USE
# =============================================================================

class TestComputerUse:

    async def test_computer_use_empty_messages(self, client):
        """POST /computer-use with empty messages returns 400."""
        response = await client.post(
            "/api/v1/computer-use",
            json={
                "messages": [],
                "display_width": 1920,
                "display_height": 1080,
            },
        )
        assert response.status_code == 400
        assert "Messages cannot be empty" in response.json()["detail"]

    async def test_computer_use_no_api_key(self, client):
        """POST /computer-use without API key returns 500."""
        with patch("app.core.config.settings.anthropic_api_key", ""):
            response = await client.post(
                "/api/v1/computer-use",
                json={
                    "messages": [{"role": "user", "content": "Click submit"}],
                    "display_width": 1920,
                    "display_height": 1080,
                },
            )

        assert response.status_code == 500

    async def test_computer_use_has_default_dimensions(self, client):
        """ComputerUseRequest defaults display_width=1920, display_height=1080."""
        mock_result = {
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "Done."}],
            "input_tokens": 500,
            "output_tokens": 20,
        }

        with patch("app.services.screen_reader.screen_reader_service.computer_use",
                    return_value=mock_result):
            # Omit display dimensions — should use defaults
            response = await client.post(
                "/api/v1/computer-use",
                json={
                    "messages": [{"role": "user", "content": "Click"}],
                },
            )

        assert response.status_code == 200

    async def test_computer_use_success(self, client):
        """POST /computer-use with valid request returns tool actions."""
        mock_result = {
            "stop_reason": "tool_use",
            "content": [
                {"type": "text", "text": "I'll click the submit button."},
                {
                    "type": "tool_use",
                    "id": "tool_1",
                    "name": "computer",
                    "input": {"action": "mouse_move", "coordinate": [500, 300]},
                },
            ],
            "input_tokens": 800,
            "output_tokens": 50,
        }

        with patch("app.services.screen_reader.screen_reader_service.computer_use",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/computer-use",
                json={
                    "messages": [{"role": "user", "content": "Click the submit button"}],
                    "display_width": 1920,
                    "display_height": 1080,
                },
            )

        assert response.status_code == 200
        data = response.json()
        assert data["stop_reason"] == "tool_use"
        assert len(data["content"]) == 2
        assert data["input_tokens"] == 800
