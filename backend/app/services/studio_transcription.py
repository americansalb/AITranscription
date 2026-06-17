"""Bulk transcription orchestration for the Transcription Studio.

Uploaded files are staged on disk and a ``MediaItem`` row (status ``queued``) is created.
A single in-process async worker drains a queue of media ids one at a time — sequential
processing respects Groq's rate limits and keeps memory low. For each item it:

  1. splits the media into Groq-sized audio chunks (ffmpeg), or sends a small file
     directly when ffmpeg is unavailable,
  2. transcribes each chunk with Groq Whisper,
  3. stitches the text together and stores timestamped ``TranscriptSegment`` rows
     (offsetting each chunk's timestamps by its start), and
  4. marks the item ``completed`` (or ``failed`` with the error) and deletes the upload.

Because Render's disk is ephemeral, a restart can interrupt an in-flight job. On startup
``recover_pending`` re-queues any non-terminal job whose staged file still exists and
fails the rest with a clear message rather than leaving them stuck.
"""

from __future__ import annotations

import asyncio
import logging
import os
import re
import shutil
import uuid
from datetime import datetime, timezone

from sqlalchemy import delete, select

from app.core.config import settings
from app.core.database import async_session_maker, engine
from app.models.base import Base
from app.models.media_library import (
    STATUS_COMPLETED,
    STATUS_FAILED,
    STATUS_PROCESSING,
    STATUS_QUEUED,
    MediaItem,
    TranscriptSegment,
)
from app.services import media_prep
from app.services.transcription import transcription_service

logger = logging.getLogger(__name__)

_MB = 1024 * 1024
_SEGMENT_TARGET_CHARS = 700  # roughly a paragraph — a good search/citation unit

_queue: asyncio.Queue[int] | None = None
_worker_task: asyncio.Task | None = None


def _now() -> datetime:
    return datetime.now(timezone.utc)


# --------------------------------------------------------------------------- #
# Storage helpers
# --------------------------------------------------------------------------- #

def upload_dir() -> str:
    """Resolve (and create) the directory used to stage uploads while processing."""
    import tempfile

    path = settings.studio_upload_dir or os.path.join(
        tempfile.gettempdir(), "vaak_studio_uploads"
    )
    os.makedirs(path, exist_ok=True)
    return path


def save_upload(data: bytes, filename: str) -> str:
    """Persist uploaded bytes to a uniquely-named file and return its path."""
    safe = re.sub(r"[^A-Za-z0-9._-]", "_", os.path.basename(filename or "upload"))
    path = os.path.join(upload_dir(), f"{uuid.uuid4().hex}_{safe}")
    with open(path, "wb") as fh:
        fh.write(data)
    return path


def _cleanup(path: str | None) -> None:
    if not path:
        return
    try:
        if os.path.isfile(path):
            os.remove(path)
    except OSError:
        pass
    chunk_dir = f"{path}_chunks"
    shutil.rmtree(chunk_dir, ignore_errors=True)


async def ensure_tables() -> None:
    """Create just the studio tables if missing — idempotent, DB-agnostic.

    Avoids depending on the full alembic run for local/dev use, and never touches the
    pgvector-typed learning tables (which can't be created on SQLite).
    """
    async with engine.begin() as conn:
        await conn.run_sync(
            lambda sync_conn: Base.metadata.create_all(
                sync_conn,
                tables=[MediaItem.__table__, TranscriptSegment.__table__],
                checkfirst=True,
            )
        )


# --------------------------------------------------------------------------- #
# Worker lifecycle
# --------------------------------------------------------------------------- #

async def start_worker() -> None:
    """Initialise tables, start the background worker, and recover interrupted jobs."""
    global _queue, _worker_task
    if _worker_task is not None and not _worker_task.done():
        return
    await ensure_tables()
    _queue = asyncio.Queue()
    _worker_task = asyncio.create_task(_worker_loop(), name="studio-transcription-worker")
    await recover_pending()


async def stop_worker() -> None:
    global _worker_task
    if _worker_task is not None:
        _worker_task.cancel()
        try:
            await _worker_task
        except (asyncio.CancelledError, Exception):
            pass
        _worker_task = None


async def enqueue(media_id: int) -> None:
    """Add a media id to the processing queue (no-op if the worker isn't running)."""
    if _queue is None:
        logger.warning("Studio worker not running; media %s left queued", media_id)
        return
    await _queue.put(media_id)


async def recover_pending() -> None:
    """Re-queue interrupted jobs whose files survived; fail the ones whose didn't."""
    async with async_session_maker() as db:
        result = await db.execute(
            select(MediaItem).where(
                MediaItem.status.in_([STATUS_QUEUED, STATUS_PROCESSING])
            )
        )
        items = result.scalars().all()
        to_requeue: list[int] = []
        for item in items:
            if item.source_path and os.path.exists(item.source_path):
                item.status = STATUS_QUEUED
                to_requeue.append(item.id)
            else:
                item.status = STATUS_FAILED
                item.error = "Interrupted before processing (the server restarted). Please re-upload."
                item.completed_at = _now()
        await db.commit()

    for media_id in to_requeue:
        await enqueue(media_id)
    if to_requeue:
        logger.info("Re-queued %d interrupted transcription job(s)", len(to_requeue))


async def _worker_loop() -> None:
    assert _queue is not None
    logger.info("Transcription studio worker started")
    while True:
        media_id = await _queue.get()
        try:
            await process_item(media_id)
        except Exception:  # pragma: no cover - defensive; process_item handles its own
            logger.exception("Unhandled error processing media %s", media_id)
        finally:
            _queue.task_done()


# --------------------------------------------------------------------------- #
# Core processing
# --------------------------------------------------------------------------- #

async def process_item(media_id: int) -> None:
    """Transcribe one media item end-to-end and persist the result."""
    async with async_session_maker() as db:
        item = await db.get(MediaItem, media_id)
        if item is None or item.status == STATUS_COMPLETED:
            return
        item.status = STATUS_PROCESSING
        item.error = None
        await db.commit()
        source_path = item.source_path
        filename = item.filename

    try:
        if not source_path or not os.path.exists(source_path):
            raise FileNotFoundError(
                "Uploaded file is no longer available (the server may have restarted mid-job)."
            )
        full_text, language, duration, segments = await _transcribe_file(
            source_path, filename
        )
    except Exception as exc:
        logger.exception("Transcription failed for media %s", media_id)
        async with async_session_maker() as db:
            item = await db.get(MediaItem, media_id)
            if item is not None:
                item.status = STATUS_FAILED
                item.error = f"{type(exc).__name__}: {exc}"[:1000]
                item.completed_at = _now()
                item.source_path = None
                await db.commit()
        _cleanup(source_path)
        return

    async with async_session_maker() as db:
        item = await db.get(MediaItem, media_id)
        if item is None:
            _cleanup(source_path)
            return
        # Clear any prior segments (e.g. on a retry) before inserting fresh ones.
        await db.execute(
            delete(TranscriptSegment).where(TranscriptSegment.media_id == media_id)
        )
        for idx, (text, start, end) in enumerate(segments):
            db.add(
                TranscriptSegment(
                    media_id=media_id,
                    idx=idx,
                    text=text,
                    start_seconds=start,
                    end_seconds=end,
                )
            )
        item.transcript = full_text
        item.language = language
        item.duration_seconds = duration
        item.word_count = len(full_text.split())
        item.status = STATUS_COMPLETED
        item.error = None
        item.completed_at = _now()
        item.source_path = None
        await db.commit()

    _cleanup(source_path)
    logger.info("Transcribed media %s (%d words)", media_id, len((full_text or "").split()))


async def _transcribe_file(path: str, filename: str):
    """Return (full_text, language, duration_seconds, [(text, start, end), ...])."""
    texts: list[str] = []
    seg_rows: list[tuple[str, float | None, float | None]] = []
    language: str | None = None
    total_duration = 0.0
    saw_duration = False

    def _read(p: str) -> bytes:
        with open(p, "rb") as fh:
            return fh.read()

    async def _handle_result(result: dict, offset: float):
        nonlocal language, total_duration, saw_duration
        text = (result.get("text") or "").strip()
        if text:
            texts.append(text)
        language = language or result.get("language")
        norm = _normalize_whisper_segments(result.get("segments"))
        if norm:
            seg_rows.extend(_group_segments(norm, offset))
        dur = result.get("duration")
        if dur is not None:
            try:
                total_duration += float(dur)
                saw_duration = True
            except (TypeError, ValueError):
                pass

    if media_prep.ffmpeg_available():
        chunk_dir = f"{path}_chunks"
        chunks = await asyncio.to_thread(
            media_prep.split_to_chunks, path, chunk_dir, settings.studio_chunk_seconds
        )
        for chunk in chunks:
            data = await asyncio.to_thread(_read, chunk.path)
            result = await transcription_service.transcribe(
                audio_data=data, filename=os.path.basename(chunk.path)
            )
            await _handle_result(result, chunk.start_seconds)
        shutil.rmtree(chunk_dir, ignore_errors=True)
    else:
        size = os.path.getsize(path)
        limit = settings.groq_direct_limit_mb * _MB
        if size > limit:
            raise ValueError(
                f"File is {size / _MB:.1f} MB. Without ffmpeg, files must be "
                f"under {settings.groq_direct_limit_mb} MB."
            )
        data = await asyncio.to_thread(_read, path)
        result = await transcription_service.transcribe(audio_data=data, filename=filename)
        await _handle_result(result, 0.0)

    full_text = "\n\n".join(texts).strip()
    if not seg_rows and full_text:
        seg_rows = _fallback_segments(full_text)

    duration: float | None = None
    if saw_duration:
        duration = round(total_duration, 2)
    elif seg_rows and seg_rows[-1][2] is not None:
        duration = seg_rows[-1][2]

    return full_text, language, duration, seg_rows


# --------------------------------------------------------------------------- #
# Segment helpers
# --------------------------------------------------------------------------- #

def _seg_value(seg, name: str):
    if isinstance(seg, dict):
        return seg.get(name)
    return getattr(seg, name, None)


def _normalize_whisper_segments(segments) -> list[dict]:
    """Coerce Groq/Whisper verbose_json segments into plain {start,end,text} dicts."""
    if not segments:
        return []
    out: list[dict] = []
    for seg in segments:
        text = (_seg_value(seg, "text") or "")
        text = text.strip() if isinstance(text, str) else ""
        if not text:
            continue
        out.append(
            {
                "start": _coerce_float(_seg_value(seg, "start")),
                "end": _coerce_float(_seg_value(seg, "end")),
                "text": text,
            }
        )
    return out


def _coerce_float(value) -> float | None:
    try:
        return float(value) if value is not None else None
    except (TypeError, ValueError):
        return None


def _group_segments(
    norm_segments: list[dict], offset: float, target_chars: int = _SEGMENT_TARGET_CHARS
):
    """Merge fine-grained Whisper segments into ~paragraph units with absolute times."""
    rows: list[tuple[str, float | None, float | None]] = []
    buf: list[str] = []
    buf_start: float | None = None
    buf_end: float | None = None

    def flush():
        nonlocal buf, buf_start, buf_end
        if buf:
            rows.append((" ".join(buf).strip(), buf_start, buf_end))
            buf, buf_start, buf_end = [], None, None

    for seg in norm_segments:
        if buf_start is None:
            buf_start = (seg["start"] + offset) if seg["start"] is not None else None
        if seg["end"] is not None:
            buf_end = seg["end"] + offset
        buf.append(seg["text"])
        if sum(len(t) for t in buf) >= target_chars:
            flush()
    flush()
    return rows


def _fallback_segments(text: str, target_chars: int = _SEGMENT_TARGET_CHARS):
    """Split plain text into ~paragraph units when no timestamps are available."""
    sentences = re.split(r"(?<=[.!?])\s+", text)
    rows: list[tuple[str, float | None, float | None]] = []
    buf: list[str] = []
    for sentence in sentences:
        buf.append(sentence)
        if sum(len(s) for s in buf) >= target_chars:
            rows.append((" ".join(buf).strip(), None, None))
            buf = []
    if buf:
        rows.append((" ".join(buf).strip(), None, None))
    return [r for r in rows if r[0]]
