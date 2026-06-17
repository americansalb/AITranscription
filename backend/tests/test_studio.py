"""Tests for the bulk Transcription Studio (services + API).

Services are tested against the real (SQLite) engine the rest of the suite uses, with
Groq and Anthropic mocked. ffmpeg is forced off so the direct-upload path is exercised
without needing the binary.
"""

import os

# Settings validate at import time, so make sure required env exists before app imports.
os.environ.setdefault("SECRET_KEY", "test-secret-key-aaaaaaaaaaaaaaaaaaaaaaaa")
os.environ.setdefault("GROQ_API_KEY", "test-groq")
os.environ.setdefault("ANTHROPIC_API_KEY", "test-anthropic")
os.environ.setdefault("DATABASE_URL", "sqlite+aiosqlite:///./test.db")

from unittest.mock import AsyncMock, patch  # noqa: E402

import pytest  # noqa: E402
from httpx import ASGITransport, AsyncClient  # noqa: E402
from sqlalchemy import delete, select  # noqa: E402

from app.core.database import async_session_maker, get_db  # noqa: E402
from app.models.media_library import (  # noqa: E402
    STATUS_COMPLETED,
    STATUS_QUEUED,
    MediaItem,
    TranscriptSegment,
)
from app.services import studio_transcription, transcript_qa  # noqa: E402


# --------------------------------------------------------------------------- #
# Fixtures
# --------------------------------------------------------------------------- #

@pytest.fixture
async def studio_db():
    """Create studio tables and clear them around each test."""
    await studio_transcription.ensure_tables()
    async with async_session_maker() as s:
        await s.execute(delete(TranscriptSegment))
        await s.execute(delete(MediaItem))
        await s.commit()
    yield
    async with async_session_maker() as s:
        await s.execute(delete(TranscriptSegment))
        await s.execute(delete(MediaItem))
        await s.commit()


def _fake_whisper(text, segments=None, duration=None, language="en"):
    return {
        "text": text,
        "language": language,
        "duration": duration,
        "segments": segments,
    }


class _Block:
    def __init__(self, text):
        self.type = "text"
        self.text = text


class _AnthropicResponse:
    def __init__(self, text):
        self.content = [_Block(text)]


# --------------------------------------------------------------------------- #
# Pure helpers
# --------------------------------------------------------------------------- #

def test_normalize_whisper_segments_handles_dicts_and_objects():
    class Seg:
        def __init__(self, start, end, text):
            self.start, self.end, self.text = start, end, text

    norm = studio_transcription._normalize_whisper_segments(
        [{"start": 0, "end": 1.5, "text": "Hello"}, Seg(1.5, 3.0, "world"), {"text": "   "}]
    )
    assert len(norm) == 2
    assert norm[0]["text"] == "Hello"
    assert norm[1]["start"] == 1.5


def test_group_segments_applies_offset_and_groups():
    norm = [
        {"start": 0.0, "end": 2.0, "text": "a" * 400},
        {"start": 2.0, "end": 4.0, "text": "b" * 400},
        {"start": 4.0, "end": 6.0, "text": "c" * 100},
    ]
    rows = studio_transcription._group_segments(norm, offset=600.0, target_chars=700)
    # First two (800 chars) flush as one group; third flushes as the second.
    assert len(rows) == 2
    assert rows[0][1] == 600.0  # start offset applied
    assert rows[0][2] == 604.0  # end of 2nd segment + offset


def test_fallback_segments_splits_text():
    text = ("Sentence one. " * 60).strip()  # ~840 chars
    rows = studio_transcription._fallback_segments(text, target_chars=300)
    assert len(rows) >= 2
    assert all(r[1] is None and r[2] is None for r in rows)


def test_extract_keywords_drops_stopwords():
    kws = transcript_qa.extract_keywords("What about the PRICING and revenue?")
    assert "pricing" in kws
    assert "revenue" in kws
    assert "the" not in kws and "about" not in kws


# --------------------------------------------------------------------------- #
# process_item end-to-end (direct path, ffmpeg off, Groq mocked)
# --------------------------------------------------------------------------- #

async def test_process_item_transcribes_and_persists(studio_db, tmp_path):
    audio = tmp_path / "clip.mp3"
    audio.write_bytes(b"fake audio bytes")

    async with async_session_maker() as s:
        item = MediaItem(filename="clip.mp3", status=STATUS_QUEUED, source_path=str(audio))
        s.add(item)
        await s.commit()
        await s.refresh(item)
        media_id = item.id

    fake = _fake_whisper(
        "The quarterly pricing went up. Revenue grew ten percent.",
        segments=[
            {"start": 0.0, "end": 3.0, "text": "The quarterly pricing went up."},
            {"start": 3.0, "end": 6.0, "text": "Revenue grew ten percent."},
        ],
        duration=6.0,
    )

    with patch.object(studio_transcription.media_prep, "ffmpeg_available", return_value=False), \
         patch.object(
             studio_transcription.transcription_service,
             "transcribe",
             new=AsyncMock(return_value=fake),
         ):
        await studio_transcription.process_item(media_id)

    async with async_session_maker() as s:
        item = await s.get(MediaItem, media_id)
        assert item.status == STATUS_COMPLETED
        assert "pricing" in item.transcript
        assert item.word_count > 0
        assert item.duration_seconds == 6.0
        assert item.source_path is None
        segs = (await s.execute(
            select(TranscriptSegment).where(TranscriptSegment.media_id == media_id)
        )).scalars().all()
        assert len(segs) >= 1

    # Upload file should have been cleaned up.
    assert not audio.exists()


async def test_process_item_marks_failed_when_file_missing(studio_db):
    async with async_session_maker() as s:
        item = MediaItem(filename="gone.mp3", status=STATUS_QUEUED, source_path="/no/such/file.mp3")
        s.add(item)
        await s.commit()
        await s.refresh(item)
        media_id = item.id

    await studio_transcription.process_item(media_id)

    async with async_session_maker() as s:
        item = await s.get(MediaItem, media_id)
        assert item.status == "failed"
        assert item.error


# --------------------------------------------------------------------------- #
# Search & Q&A
# --------------------------------------------------------------------------- #

async def _seed_completed(filename="meeting.mp3"):
    async with async_session_maker() as s:
        item = MediaItem(
            filename=filename,
            status=STATUS_COMPLETED,
            transcript="We discussed pricing and the new revenue model.",
            word_count=8,
        )
        s.add(item)
        await s.commit()
        await s.refresh(item)
        s.add_all([
            TranscriptSegment(media_id=item.id, idx=0, text="We discussed pricing in detail.",
                              start_seconds=0.0, end_seconds=5.0),
            TranscriptSegment(media_id=item.id, idx=1, text="The new revenue model looks strong.",
                              start_seconds=5.0, end_seconds=10.0),
        ])
        await s.commit()
        return item.id


async def test_search_segments_finds_keyword(studio_db):
    await _seed_completed()
    async with async_session_maker() as s:
        results = await transcript_qa.search_segments(s, "pricing")
    assert results
    assert any("pricing" in r["text"].lower() for r in results)
    assert results[0]["timestamp"] == "00:00"


async def test_answer_question_uses_haiku_with_sources(studio_db):
    await _seed_completed()
    with patch("anthropic.AsyncAnthropic") as MockClient:
        instance = MockClient.return_value
        instance.messages.create = AsyncMock(
            return_value=_AnthropicResponse("Pricing was discussed [1].")
        )
        async with async_session_maker() as s:
            result = await transcript_qa.answer_question(s, "What about pricing?")

    assert "Pricing" in result["answer"]
    assert result["sources"]
    assert result["model"]  # qa_model echoed back
    # Confirm we actually called Haiku with a system prompt + user message.
    _, kwargs = instance.messages.create.call_args
    assert kwargs["system"]
    assert kwargs["messages"][0]["role"] == "user"


# --------------------------------------------------------------------------- #
# API endpoints
# --------------------------------------------------------------------------- #

async def test_config_endpoint_open_by_default():
    from app.main import app

    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as ac:
        r = await ac.get("/api/v1/studio/config")
    assert r.status_code == 200
    body = r.json()
    assert body["access_required"] is False
    assert "qa_model" in body


async def test_upload_creates_queued_jobs(studio_db):
    from app.main import app

    async def _override_db():
        async with async_session_maker() as session:
            yield session

    app.dependency_overrides[get_db] = _override_db
    try:
        with patch.object(studio_transcription, "enqueue", new=AsyncMock()) as mock_enqueue:
            transport = ASGITransport(app=app)
            async with AsyncClient(transport=transport, base_url="http://test") as ac:
                files = [
                    ("files", ("a.mp3", b"fake-bytes-aaa", "audio/mpeg")),
                    ("files", ("b.xyz", b"bad", "application/octet-stream")),
                ]
                r = await ac.post("/api/v1/studio/jobs", files=files)
                assert r.status_code == 200
                data = r.json()
                assert len(data["created"]) == 1
                assert data["created"][0]["status"] == STATUS_QUEUED
                assert len(data["errors"]) == 1  # b.xyz rejected

                listing = await ac.get("/api/v1/studio/jobs")
                assert listing.status_code == 200
                assert any(j["filename"] == "a.mp3" for j in listing.json()["jobs"])

            assert mock_enqueue.await_count == 1
    finally:
        app.dependency_overrides.pop(get_db, None)
