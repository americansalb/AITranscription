"""Tests for transcription and polishing API endpoints.

Covers:
  - POST /transcribe (audio upload, format validation, size limit)
  - POST /polish (text polishing, empty text, auth required)
  - POST /transcribe-and-polish (combined endpoint)
  - POST /tts (text-to-speech, empty text)
  - POST /transcribe-base64 (base64 audio, invalid base64)
  - GET /health (public endpoint)
  - Authorization behavior (auth required for all endpoints except health)
"""
import base64
import pytest
from unittest.mock import AsyncMock, MagicMock, patch
from io import BytesIO


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


# =============================================================================
# HEALTH CHECK
# =============================================================================

class TestHealthEndpoint:

    async def test_health_returns_200(self, client):
        """GET /health returns 200 with status info."""
        response = await client.get("/api/v1/health")
        assert response.status_code == 200
        data = response.json()
        assert data["status"] == "healthy"
        assert "groq_configured" in data
        assert "anthropic_configured" in data

    async def test_health_no_auth_needed(self, client):
        """Health check does not require authentication."""
        response = await client.get("/api/v1/health")
        assert response.status_code == 200


# =============================================================================
# TRANSCRIBE ENDPOINT
# =============================================================================

class TestTranscribeEndpoint:

    async def test_transcribe_success(self, client, auth_headers):
        """POST /transcribe with valid audio returns transcription."""
        user = make_user()
        mock_result = {
            "text": "Hello, this is a test recording.",
            "duration": 3.5,
            "language": "en",
            "segments": [],
        }

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.wav", b"fake audio data", "audio/wav")},
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["raw_text"] == "Hello, this is a test recording."
        assert data["duration"] == 3.5
        assert data["language"] == "en"

    async def test_transcribe_with_language(self, client, auth_headers):
        """POST /transcribe with language parameter passes it through."""
        user = make_user()
        mock_result = {"text": "Hola mundo", "duration": 2.0, "language": "es"}

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.mp3", b"fake mp3 data", "audio/mpeg")},
                data={"language": "es"},
                headers=auth_headers,
            )

        assert response.status_code == 200
        assert response.json()["language"] == "es"

    async def test_transcribe_invalid_format(self, client, auth_headers):
        """POST /transcribe with unsupported format returns 400."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.txt", b"not audio", "text/plain")},
                headers=auth_headers,
            )
        assert response.status_code == 400
        assert "Unsupported audio format" in response.json()["detail"]

    async def test_transcribe_auth_required(self, client):
        """Transcribe endpoint returns 401 when no auth token provided."""
        mock_result = {"text": "Test", "duration": 1.0, "language": "en"}

        with patch("app.services.transcription_service.transcribe",
                   return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.wav", b"audio data", "audio/wav")},
            )

        assert response.status_code == 401

    async def test_transcribe_service_error(self, client, auth_headers):
        """POST /transcribe when service fails returns 502."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   side_effect=Exception("Groq API error")):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.wav", b"audio data", "audio/wav")},
                headers=auth_headers,
            )

        assert response.status_code == 502
        assert "Transcription" in response.json()["detail"]

    async def test_transcribe_value_error(self, client, auth_headers):
        """POST /transcribe when service raises ValueError returns 400."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   side_effect=ValueError("bad input")):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.wav", b"audio data", "audio/wav")},
                headers=auth_headers,
            )

        assert response.status_code == 400
        assert "Transcription failed" in response.json()["detail"]

    async def test_transcribe_accepts_valid_extensions(self, client, auth_headers):
        """Various valid audio extensions should be accepted."""
        user = make_user()
        mock_result = {"text": "Test", "duration": 1.0, "language": "en"}

        for ext, mime in [
            ("wav", "audio/wav"),
            ("mp3", "audio/mpeg"),
            ("m4a", "audio/mp4"),
            ("webm", "audio/webm"),
            ("ogg", "audio/ogg"),
            ("flac", "audio/flac"),
        ]:
            with patch("app.api.auth.get_user_by_id", return_value=user), \
                 patch("app.services.transcription_service.transcribe",
                       return_value=mock_result):
                response = await client.post(
                    "/api/v1/transcribe",
                    files={"audio": (f"test.{ext}", b"audio data", mime)},
                    headers=auth_headers,
                )
            assert response.status_code == 200, f"Failed for extension .{ext}"


# =============================================================================
# POLISH ENDPOINT
# =============================================================================

class TestPolishEndpoint:

    async def test_polish_success(self, client, auth_headers):
        """POST /polish with valid text returns polished result."""
        user = make_user()
        mock_result = {
            "text": "I think we should discuss the project.",
            "usage": {"input_tokens": 450, "output_tokens": 280},
            "corrections_used": 0,
        }

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.polish_service.polish",
                   return_value=mock_result):
            response = await client.post(
                "/api/v1/polish",
                json={
                    "text": "uh I think we should um discuss the project",
                    "context": "email",
                    "formality": "neutral",
                },
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["text"] == "I think we should discuss the project."
        assert data["input_tokens"] == 450
        assert data["output_tokens"] == 280

    async def test_polish_empty_text(self, client, auth_headers):
        """POST /polish with empty text returns 400."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/polish",
                json={"text": ""},
                headers=auth_headers,
            )
        assert response.status_code == 400
        assert "empty" in response.json()["detail"].lower()

    async def test_polish_whitespace_only(self, client, auth_headers):
        """POST /polish with whitespace-only text returns 400."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/polish",
                json={"text": "   "},
                headers=auth_headers,
            )
        assert response.status_code == 400

    async def test_polish_auth_required(self, client):
        """Polish returns 401 when no auth token provided."""
        mock_result = {
            "text": "Test output.",
            "usage": {"input_tokens": 10, "output_tokens": 5},
            "corrections_used": 0,
        }

        with patch("app.services.polish_service.polish",
                   return_value=mock_result):
            response = await client.post(
                "/api/v1/polish",
                json={"text": "test input"},
            )

        assert response.status_code == 401

    async def test_polish_with_custom_words(self, client, auth_headers):
        """POST /polish passes custom_words to the service."""
        user = make_user()
        mock_result = {
            "text": "Using Vaak for transcription.",
            "usage": {"input_tokens": 20, "output_tokens": 10},
            "corrections_used": 0,
        }

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.polish_service.polish",
                   return_value=mock_result) as mock_polish:
            response = await client.post(
                "/api/v1/polish",
                json={
                    "text": "using vaak for transcription",
                    "custom_words": ["Vaak"],
                },
                headers=auth_headers,
            )

        assert response.status_code == 200
        # Verify custom_words was passed through
        mock_polish.assert_called_once()
        call_kwargs = mock_polish.call_args
        assert "Vaak" in call_kwargs.kwargs.get("custom_words", []) or \
               "Vaak" in (call_kwargs[1].get("custom_words", []) if len(call_kwargs) > 1 else [])

    async def test_polish_service_error(self, client, auth_headers):
        """POST /polish when service fails returns 200 with raw text (graceful fallback)."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.polish_service.polish",
                   side_effect=Exception("Anthropic API error")):
            response = await client.post(
                "/api/v1/polish",
                json={"text": "test input"},
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        # Graceful fallback: returns the raw input text
        assert data["text"] == "test input"
        assert data["input_tokens"] == 0
        assert data["output_tokens"] == 0

    async def test_polish_missing_text_field(self, client, auth_headers):
        """POST /polish without text field returns 422."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/polish",
                json={"context": "email"},
                headers=auth_headers,
            )
        assert response.status_code == 422

    async def test_polish_with_all_formality_levels(self, client, auth_headers):
        """Polish accepts all formality levels."""
        user = make_user()
        mock_result = {
            "text": "Output",
            "usage": {"input_tokens": 5, "output_tokens": 3},
            "corrections_used": 0,
        }

        for formality in ["casual", "neutral", "formal"]:
            with patch("app.api.auth.get_user_by_id", return_value=user), \
                 patch("app.services.polish_service.polish",
                       return_value=mock_result):
                response = await client.post(
                    "/api/v1/polish",
                    json={"text": "test", "formality": formality},
                    headers=auth_headers,
                )
            assert response.status_code == 200, f"Failed for formality={formality}"


# =============================================================================
# TTS ENDPOINT
# =============================================================================

class TestTTSEndpoint:

    async def test_tts_success(self, client, auth_headers):
        """POST /tts with valid text returns audio bytes."""
        user = make_user()
        fake_audio = b"\xff\xfb\x90\x00" * 100  # Fake MP3 bytes

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.elevenlabs_tts.synthesize",
                   return_value=fake_audio):
            response = await client.post(
                "/api/v1/tts",
                data={"text": "Hello world"},
                headers=auth_headers,
            )

        assert response.status_code == 200
        assert response.headers.get("content-type") == "audio/mpeg"

    async def test_tts_empty_text(self, client, auth_headers):
        """POST /tts with empty text returns 422 (Form validation)."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/tts",
                data={"text": ""},
                headers=auth_headers,
            )
        # FastAPI Form validation rejects empty string before reaching endpoint logic
        assert response.status_code == 422

    async def test_tts_with_session_id(self, client, auth_headers):
        """POST /tts with session_id for Claude Code integration."""
        user = make_user()
        fake_audio = b"\xff\xfb\x90\x00" * 100

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.elevenlabs_tts.synthesize",
                   return_value=fake_audio):
            response = await client.post(
                "/api/v1/tts",
                data={"text": "Hello", "session_id": "hostname-12345"},
                headers=auth_headers,
            )

        assert response.status_code == 200

    async def test_tts_service_error(self, client, auth_headers):
        """POST /tts when ElevenLabs returns empty audio returns 502."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.elevenlabs_tts.synthesize",
                   return_value=None):
            response = await client.post(
                "/api/v1/tts",
                data={"text": "Hello world"},
                headers=auth_headers,
            )

        assert response.status_code == 502


# =============================================================================
# TRANSCRIBE BASE64 ENDPOINT
# =============================================================================

class TestTranscribeBase64Endpoint:

    async def test_base64_success(self, client, auth_headers):
        """POST /transcribe-base64 with valid base64 audio returns transcription."""
        user = make_user()
        audio_bytes = b"RIFF" + b"\x00" * 100  # Fake WAV-like bytes
        encoded = base64.b64encode(audio_bytes).decode("utf-8")

        mock_result = {"text": "Hello from base64", "duration": 2.0, "language": "en"}

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe-base64",
                json={"audio_base64": encoded},
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["raw_text"] == "Hello from base64"

    async def test_base64_invalid_encoding(self, client, auth_headers):
        """POST /transcribe-base64 with invalid base64 returns 400."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/transcribe-base64",
                json={"audio_base64": "!!!not-valid-base64!!!"},
                headers=auth_headers,
            )
        assert response.status_code == 400

    async def test_base64_with_language(self, client, auth_headers):
        """POST /transcribe-base64 passes language parameter."""
        user = make_user()
        audio_bytes = b"RIFF" + b"\x00" * 50
        encoded = base64.b64encode(audio_bytes).decode("utf-8")

        mock_result = {"text": "Hola", "duration": 1.0, "language": "es"}

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe-base64",
                json={"audio_base64": encoded, "language": "es"},
                headers=auth_headers,
            )

        assert response.status_code == 200
        assert response.json()["language"] == "es"

    async def test_base64_missing_audio(self, client, auth_headers):
        """POST /transcribe-base64 without audio_base64 returns 422."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/transcribe-base64",
                json={},
                headers=auth_headers,
            )
        assert response.status_code == 422


# =============================================================================
# TRANSCRIBE AND POLISH COMBINED ENDPOINT
# =============================================================================

class TestTranscribeAndPolishEndpoint:

    async def test_combined_success_authenticated(self, client, auth_headers):
        """POST /transcribe-and-polish works with auth and sets saved=True."""
        user = make_user()
        mock_transcribe = {
            "text": "uh hello this is um a test",
            "duration": 3.0,
            "language": "en",
        }
        mock_polish = {
            "text": "Hello, this is a test.",
            "usage": {"input_tokens": 50, "output_tokens": 30},
            "corrections_used": 0,
        }

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   return_value=mock_transcribe), \
             patch("app.services.polish_service.polish",
                   return_value=mock_polish), \
             patch("app.services.gamification.GamificationService.award_transcription_xp",
                   return_value=None), \
             patch("app.services.gamification.AchievementService.check_achievements",
                   return_value=[]):
            response = await client.post(
                "/api/v1/transcribe-and-polish",
                files={"audio": ("test.wav", b"fake audio", "audio/wav")},
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["raw_text"] == "uh hello this is um a test"
        assert data["polished_text"] == "Hello, this is a test."
        assert data["duration"] == 3.0
        assert data["saved"] is True  # Authenticated user

    async def test_combined_invalid_audio(self, client, auth_headers):
        """POST /transcribe-and-polish with bad format returns 400."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/transcribe-and-polish",
                files={"audio": ("test.pdf", b"not audio", "application/pdf")},
                headers=auth_headers,
            )
        assert response.status_code == 400

    async def test_combined_transcribe_failure(self, client, auth_headers):
        """POST /transcribe-and-polish when transcription fails returns 502."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   side_effect=Exception("Groq error")):
            response = await client.post(
                "/api/v1/transcribe-and-polish",
                files={"audio": ("test.wav", b"audio", "audio/wav")},
                headers=auth_headers,
            )

        assert response.status_code == 502

    async def test_combined_polish_failure_returns_raw_text(self, client, auth_headers):
        """POST /transcribe-and-polish gracefully falls back to raw text when polish fails.

        Regression test for the "polish failed" bug: when the Anthropic API key
        is missing or the polish service errors, the endpoint should return the
        raw transcription as both raw_text and polished_text instead of HTTP 500.
        """
        user = make_user()
        mock_transcribe = {
            "text": "uh hello this is um a test",
            "duration": 3.0,
            "language": "en",
        }

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   return_value=mock_transcribe), \
             patch("app.services.polish_service.polish",
                   side_effect=ValueError("ANTHROPIC_API_KEY is not configured")):
            response = await client.post(
                "/api/v1/transcribe-and-polish",
                files={"audio": ("test.wav", b"fake audio", "audio/wav")},
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["raw_text"] == "uh hello this is um a test"
        # Fallback: polished_text == raw_text when polish fails
        assert data["polished_text"] == data["raw_text"]
        assert data["duration"] == 3.0
        assert data["language"] == "en"
        assert data["usage"]["input_tokens"] == 0
        assert data["usage"]["output_tokens"] == 0

    async def test_combined_polish_timeout_returns_raw_text(self, client, auth_headers):
        """Polish timeout should gracefully fall back to raw text, not 500."""
        user = make_user()
        mock_transcribe = {
            "text": "a longer test of the timeout behavior",
            "duration": 5.0,
            "language": "en",
        }

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   return_value=mock_transcribe), \
             patch("app.services.polish_service.polish",
                   side_effect=TimeoutError("Text polishing timed out after 60s")):
            response = await client.post(
                "/api/v1/transcribe-and-polish",
                files={"audio": ("test.wav", b"fake audio", "audio/wav")},
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["polished_text"] == data["raw_text"]
        assert data["usage"]["input_tokens"] == 0

    async def test_combined_polish_api_error_returns_raw_text(self, client, auth_headers):
        """Any polish API error should fall back gracefully, not crash."""
        user = make_user()
        mock_transcribe = {
            "text": "testing error handling",
            "duration": 2.0,
            "language": "en",
        }

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   return_value=mock_transcribe), \
             patch("app.services.polish_service.polish",
                   side_effect=RuntimeError("Connection refused")):
            response = await client.post(
                "/api/v1/transcribe-and-polish",
                files={"audio": ("test.wav", b"fake audio", "audio/wav")},
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["polished_text"] == "testing error handling"

    async def test_combined_empty_transcription_skips_polish(self, client, auth_headers):
        """Empty transcription result should skip polish entirely."""
        user = make_user()
        mock_transcribe = {
            "text": "",
            "duration": 1.0,
            "language": "en",
        }

        # Polish should NOT be called when transcription is empty
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.transcription_service.transcribe",
                   return_value=mock_transcribe), \
             patch("app.services.polish_service.polish") as mock_polish:
            response = await client.post(
                "/api/v1/transcribe-and-polish",
                files={"audio": ("test.wav", b"silence", "audio/wav")},
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["raw_text"] == ""
        assert data["polished_text"] == ""
        mock_polish.assert_not_called()
