"""Tests for transcription and polishing API endpoints.

Covers:
  - POST /transcribe (audio upload, format validation, size limit)
  - POST /polish (text polishing, empty text, auth optional)
  - POST /transcribe-and-polish (combined endpoint)
  - POST /tts (text-to-speech, empty text)
  - POST /transcribe-base64 (base64 audio, invalid base64)
  - GET /health (public endpoint)
  - Authorization behavior (optional auth for most endpoints)
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

    async def test_transcribe_success(self, client):
        """POST /transcribe with valid audio returns transcription."""
        mock_result = {
            "text": "Hello, this is a test recording.",
            "duration": 3.5,
            "language": "en",
            "segments": [],
        }

        with patch("app.services.transcription_service.transcribe",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.wav", b"fake audio data", "audio/wav")},
            )

        assert response.status_code == 200
        data = response.json()
        assert data["raw_text"] == "Hello, this is a test recording."
        assert data["duration"] == 3.5
        assert data["language"] == "en"

    async def test_transcribe_with_language(self, client):
        """POST /transcribe with language parameter passes it through."""
        mock_result = {"text": "Hola mundo", "duration": 2.0, "language": "es"}

        with patch("app.services.transcription_service.transcribe",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.mp3", b"fake mp3 data", "audio/mpeg")},
                data={"language": "es"},
            )

        assert response.status_code == 200
        assert response.json()["language"] == "es"

    async def test_transcribe_invalid_format(self, client):
        """POST /transcribe with unsupported format returns 400."""
        response = await client.post(
            "/api/v1/transcribe",
            files={"audio": ("test.txt", b"not audio", "text/plain")},
        )
        assert response.status_code == 400
        assert "Unsupported audio format" in response.json()["detail"]

    async def test_transcribe_no_auth_required(self, client):
        """Transcribe endpoint works without authentication."""
        mock_result = {"text": "Test", "duration": 1.0, "language": "en"}

        with patch("app.services.transcription_service.transcribe",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.wav", b"audio data", "audio/wav")},
            )

        assert response.status_code == 200

    async def test_transcribe_service_error(self, client):
        """POST /transcribe when service fails returns 500."""
        with patch("app.services.transcription_service.transcribe",
                    side_effect=Exception("Groq API error")):
            response = await client.post(
                "/api/v1/transcribe",
                files={"audio": ("test.wav", b"audio data", "audio/wav")},
            )

        assert response.status_code == 500
        assert "Transcription failed" in response.json()["detail"]

    async def test_transcribe_accepts_valid_extensions(self, client):
        """Various valid audio extensions should be accepted."""
        mock_result = {"text": "Test", "duration": 1.0, "language": "en"}

        for ext, mime in [
            ("wav", "audio/wav"),
            ("mp3", "audio/mpeg"),
            ("m4a", "audio/mp4"),
            ("webm", "audio/webm"),
            ("ogg", "audio/ogg"),
            ("flac", "audio/flac"),
        ]:
            with patch("app.services.transcription_service.transcribe",
                        return_value=mock_result):
                response = await client.post(
                    "/api/v1/transcribe",
                    files={"audio": (f"test.{ext}", b"audio data", mime)},
                )
            assert response.status_code == 200, f"Failed for extension .{ext}"


# =============================================================================
# POLISH ENDPOINT
# =============================================================================

class TestPolishEndpoint:

    async def test_polish_success(self, client):
        """POST /polish with valid text returns polished result."""
        mock_result = {
            "text": "I think we should discuss the project.",
            "usage": {"input_tokens": 450, "output_tokens": 280},
            "corrections_used": 0,
        }

        with patch("app.services.polish_service.polish",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/polish",
                json={
                    "text": "uh I think we should um discuss the project",
                    "context": "email",
                    "formality": "neutral",
                },
            )

        assert response.status_code == 200
        data = response.json()
        assert data["text"] == "I think we should discuss the project."
        assert data["input_tokens"] == 450
        assert data["output_tokens"] == 280

    async def test_polish_empty_text(self, client):
        """POST /polish with empty text returns 400."""
        response = await client.post(
            "/api/v1/polish",
            json={"text": ""},
        )
        assert response.status_code == 400
        assert "empty" in response.json()["detail"].lower()

    async def test_polish_whitespace_only(self, client):
        """POST /polish with whitespace-only text returns 400."""
        response = await client.post(
            "/api/v1/polish",
            json={"text": "   "},
        )
        assert response.status_code == 400

    async def test_polish_no_auth_works(self, client):
        """Polish works without authentication (optional auth)."""
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

        assert response.status_code == 200

    async def test_polish_with_custom_words(self, client):
        """POST /polish passes custom_words to the service."""
        mock_result = {
            "text": "Using Vaak for transcription.",
            "usage": {"input_tokens": 20, "output_tokens": 10},
            "corrections_used": 0,
        }

        with patch("app.services.polish_service.polish",
                    return_value=mock_result) as mock_polish:
            response = await client.post(
                "/api/v1/polish",
                json={
                    "text": "using vaak for transcription",
                    "custom_words": ["Vaak"],
                },
            )

        assert response.status_code == 200
        # Verify custom_words was passed through
        mock_polish.assert_called_once()
        call_kwargs = mock_polish.call_args
        assert "Vaak" in call_kwargs.kwargs.get("custom_words", []) or \
               "Vaak" in (call_kwargs[1].get("custom_words", []) if len(call_kwargs) > 1 else [])

    async def test_polish_service_error(self, client):
        """POST /polish when service fails returns 500."""
        with patch("app.services.polish_service.polish",
                    side_effect=Exception("Anthropic API error")):
            response = await client.post(
                "/api/v1/polish",
                json={"text": "test input"},
            )

        assert response.status_code == 500
        assert "Polish failed" in response.json()["detail"]

    async def test_polish_missing_text_field(self, client):
        """POST /polish without text field returns 422."""
        response = await client.post(
            "/api/v1/polish",
            json={"context": "email"},
        )
        assert response.status_code == 422

    async def test_polish_with_all_formality_levels(self, client):
        """Polish accepts all formality levels."""
        mock_result = {
            "text": "Output",
            "usage": {"input_tokens": 5, "output_tokens": 3},
            "corrections_used": 0,
        }

        for formality in ["casual", "neutral", "formal"]:
            with patch("app.services.polish_service.polish",
                        return_value=mock_result):
                response = await client.post(
                    "/api/v1/polish",
                    json={"text": "test", "formality": formality},
                )
            assert response.status_code == 200, f"Failed for formality={formality}"


# =============================================================================
# TTS ENDPOINT
# =============================================================================

class TestTTSEndpoint:

    async def test_tts_success(self, client):
        """POST /tts with valid text returns audio bytes."""
        fake_audio = b"\xff\xfb\x90\x00" * 100  # Fake MP3 bytes

        with patch("app.services.elevenlabs_tts.synthesize",
                    return_value=fake_audio):
            response = await client.post(
                "/api/v1/tts",
                data={"text": "Hello world"},
            )

        assert response.status_code == 200
        assert response.headers.get("content-type") == "audio/mpeg"

    async def test_tts_empty_text(self, client):
        """POST /tts with empty text returns 422 (Form validation)."""
        response = await client.post(
            "/api/v1/tts",
            data={"text": ""},
        )
        # FastAPI Form validation rejects empty string before reaching endpoint logic
        assert response.status_code == 422

    async def test_tts_with_session_id(self, client):
        """POST /tts with session_id for Claude Code integration."""
        fake_audio = b"\xff\xfb\x90\x00" * 100

        with patch("app.services.elevenlabs_tts.synthesize",
                    return_value=fake_audio):
            response = await client.post(
                "/api/v1/tts",
                data={"text": "Hello", "session_id": "hostname-12345"},
            )

        assert response.status_code == 200

    async def test_tts_service_error(self, client):
        """POST /tts when ElevenLabs fails returns 500."""
        with patch("app.services.elevenlabs_tts.synthesize",
                    return_value=None):
            response = await client.post(
                "/api/v1/tts",
                data={"text": "Hello world"},
            )

        assert response.status_code == 500


# =============================================================================
# TRANSCRIBE BASE64 ENDPOINT
# =============================================================================

class TestTranscribeBase64Endpoint:

    async def test_base64_success(self, client):
        """POST /transcribe-base64 with valid base64 audio returns transcription."""
        audio_bytes = b"RIFF" + b"\x00" * 100  # Fake WAV-like bytes
        encoded = base64.b64encode(audio_bytes).decode("utf-8")

        mock_result = {"text": "Hello from base64", "duration": 2.0, "language": "en"}

        with patch("app.services.transcription_service.transcribe",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe-base64",
                json={"audio_base64": encoded},
            )

        assert response.status_code == 200
        data = response.json()
        assert data["raw_text"] == "Hello from base64"

    async def test_base64_invalid_encoding(self, client):
        """POST /transcribe-base64 with invalid base64 returns 400."""
        response = await client.post(
            "/api/v1/transcribe-base64",
            json={"audio_base64": "!!!not-valid-base64!!!"},
        )
        assert response.status_code == 400

    async def test_base64_with_language(self, client):
        """POST /transcribe-base64 passes language parameter."""
        audio_bytes = b"RIFF" + b"\x00" * 50
        encoded = base64.b64encode(audio_bytes).decode("utf-8")

        mock_result = {"text": "Hola", "duration": 1.0, "language": "es"}

        with patch("app.services.transcription_service.transcribe",
                    return_value=mock_result):
            response = await client.post(
                "/api/v1/transcribe-base64",
                json={"audio_base64": encoded, "language": "es"},
            )

        assert response.status_code == 200
        assert response.json()["language"] == "es"

    async def test_base64_missing_audio(self, client):
        """POST /transcribe-base64 without audio_base64 returns 422."""
        response = await client.post(
            "/api/v1/transcribe-base64",
            json={},
        )
        assert response.status_code == 422


# =============================================================================
# TRANSCRIBE AND POLISH COMBINED ENDPOINT
# =============================================================================

class TestTranscribeAndPolishEndpoint:

    async def test_combined_success_unauthenticated(self, client):
        """POST /transcribe-and-polish works without auth."""
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

        with patch("app.services.transcription_service.transcribe",
                    return_value=mock_transcribe), \
             patch("app.services.polish_service.polish",
                   return_value=mock_polish):
            response = await client.post(
                "/api/v1/transcribe-and-polish",
                files={"audio": ("test.wav", b"fake audio", "audio/wav")},
            )

        assert response.status_code == 200
        data = response.json()
        assert data["raw_text"] == "uh hello this is um a test"
        assert data["polished_text"] == "Hello, this is a test."
        assert data["duration"] == 3.0
        assert data["saved"] is False  # Not authenticated

    async def test_combined_invalid_audio(self, client):
        """POST /transcribe-and-polish with bad format returns 400."""
        response = await client.post(
            "/api/v1/transcribe-and-polish",
            files={"audio": ("test.pdf", b"not audio", "application/pdf")},
        )
        assert response.status_code == 400

    async def test_combined_transcribe_failure(self, client):
        """POST /transcribe-and-polish when transcription fails returns 500."""
        with patch("app.services.transcription_service.transcribe",
                    side_effect=Exception("Groq error")):
            response = await client.post(
                "/api/v1/transcribe-and-polish",
                files={"audio": ("test.wav", b"audio", "audio/wav")},
            )

        assert response.status_code == 500
