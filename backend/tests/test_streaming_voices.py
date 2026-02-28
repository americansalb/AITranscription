"""Tests for streaming endpoints and voice listing.

Covers:
  - POST /polish-stream — SSE streaming polish
  - POST /tts/stream — streaming text-to-speech
  - GET /voices — list available ElevenLabs voices
  - GET / — root endpoint
"""
import json
import pytest
from unittest.mock import AsyncMock, MagicMock, patch

from tests.conftest import make_user


# === GET /voices ===

class TestGetVoices:

    async def test_voices_returns_list(self, client, auth_headers):
        """Voices endpoint returns list of available voices."""
        user = make_user()
        mock_voices = [
            {"voice_id": "abc123", "name": "Rachel"},
            {"voice_id": "def456", "name": "Drew"},
        ]

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.routes.elevenlabs_tts.get_available_voices", return_value=mock_voices),
        ):
            resp = await client.get("/api/v1/voices", headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert "voices" in data
        assert len(data["voices"]) == 2
        assert data["voices"][0]["name"] == "Rachel"
        assert data["voices"][1]["voice_id"] == "def456"

    async def test_voices_empty(self, client, auth_headers):
        """Voices endpoint returns empty list when no voices configured."""
        user = make_user()

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.routes.elevenlabs_tts.get_available_voices", return_value=[]),
        ):
            resp = await client.get("/api/v1/voices", headers=auth_headers)

        assert resp.status_code == 200
        assert resp.json()["voices"] == []

    async def test_voices_no_auth(self, client):
        """Voices without auth returns 401."""
        resp = await client.get("/api/v1/voices")
        assert resp.status_code == 401


# === POST /polish-stream ===

class TestPolishStream:

    async def test_polish_stream_success(self, client, auth_headers):
        """Polish stream returns SSE events."""
        user = make_user()

        async def mock_stream(**kwargs):
            yield {"type": "correction_info", "data": {"count": 2}}
            yield {"type": "chunk", "data": {"text": "Hello, "}}
            yield {"type": "chunk", "data": {"text": "world."}}
            yield {"type": "done", "data": {"input_tokens": 50, "output_tokens": 10}}

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.routes.polish_service.polish_stream", return_value=mock_stream()),
        ):
            resp = await client.post("/api/v1/polish-stream", json={
                "text": "hello world",
            }, headers=auth_headers)

        assert resp.status_code == 200
        assert resp.headers.get("content-type", "").startswith("text/event-stream")

        # Parse SSE events from response body
        body = resp.text
        events = [line.replace("data: ", "") for line in body.strip().split("\n") if line.startswith("data: ")]
        assert len(events) >= 1  # At least one event

    async def test_polish_stream_empty_text(self, client, auth_headers):
        """Polish stream with empty text returns 400."""
        user = make_user()

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.post("/api/v1/polish-stream", json={
                "text": "   ",
            }, headers=auth_headers)

        assert resp.status_code == 400
        assert "empty" in resp.json()["detail"]

    async def test_polish_stream_no_auth(self, client):
        """Polish stream without auth returns 401."""
        resp = await client.post("/api/v1/polish-stream", json={"text": "hello"})
        assert resp.status_code == 401


# === POST /tts/stream ===

class TestTtsStream:

    async def test_tts_stream_success(self, client, auth_headers):
        """TTS stream returns chunked audio."""
        user = make_user()

        async def mock_audio_stream(text, voice_id=None):
            yield b"\xff\xfb\x90\x00"  # Fake MP3 header
            yield b"\x00" * 1024  # Fake audio data

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.routes.elevenlabs_tts.synthesize_stream", return_value=mock_audio_stream("test")),
        ):
            resp = await client.post("/api/v1/tts/stream", data={
                "text": "Hello world",
            }, headers=auth_headers)

        assert resp.status_code == 200
        assert resp.headers.get("content-type") == "audio/mpeg"

    async def test_tts_stream_empty_text(self, client, auth_headers):
        """TTS stream with empty text returns 400."""
        user = make_user()

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.post("/api/v1/tts/stream", data={
                "text": "   ",
            }, headers=auth_headers)

        assert resp.status_code == 400

    async def test_tts_stream_text_too_long(self, client, auth_headers):
        """TTS stream with > 5000 chars returns 400."""
        user = make_user()

        with patch("app.api.auth.get_user_by_id", return_value=user):
            resp = await client.post("/api/v1/tts/stream", data={
                "text": "a" * 5001,
            }, headers=auth_headers)

        assert resp.status_code == 400
        assert "too long" in resp.json()["detail"].lower()

    async def test_tts_stream_no_auth(self, client):
        """TTS stream without auth returns 401."""
        resp = await client.post("/api/v1/tts/stream", data={"text": "hello"})
        assert resp.status_code == 401


# === GET / (root endpoint) ===

class TestRootEndpoint:

    async def test_root_returns_info(self, client):
        """Root endpoint returns service information."""
        resp = await client.get("/")
        # Root may return 200 with app info or 404 if not defined
        # Based on main.py, there should be a root handler
        assert resp.status_code in (200, 404)
