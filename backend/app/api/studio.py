"""Transcription Studio API — bulk upload, transcription status, search, and Q&A.

Mounted under /api/v1/studio. Like the existing /vaaklite and /translate utilities this
is unauthenticated by default; set VAAK ``studio_access_token`` (env STUDIO_ACCESS_TOKEN)
to require a shared secret via the ``X-Studio-Token`` header (or ``?token=``).
"""

from __future__ import annotations

import logging

from fastapi import APIRouter, Depends, File, Header, HTTPException, Query, UploadFile
from fastapi.responses import PlainTextResponse
from pydantic import BaseModel, Field
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.config import settings
from app.core.database import get_db
from app.models.media_library import (
    STATUS_QUEUED,
    MediaItem,
    TranscriptSegment,
)
from app.services import media_prep, studio_transcription, transcript_qa

logger = logging.getLogger(__name__)
router = APIRouter()

_MB = 1024 * 1024

# Audio + video extensions we accept. With ffmpeg present (the default) the audio is
# extracted and re-encoded, so video containers work too.
ALLOWED_EXTENSIONS = {
    # audio
    ".wav", ".mp3", ".m4a", ".aac", ".flac", ".ogg", ".oga", ".opus", ".webm",
    ".wma", ".aiff", ".aif",
    # video
    ".mp4", ".mov", ".mkv", ".avi", ".m4v", ".mpeg", ".mpg", ".3gp", ".wmv", ".flv",
}


# --------------------------------------------------------------------------- #
# Access gate
# --------------------------------------------------------------------------- #

async def require_studio_access(
    x_studio_token: str | None = Header(default=None),
    token: str | None = Query(default=None),
) -> None:
    required = settings.studio_access_token
    if required and (x_studio_token or token) != required:
        raise HTTPException(status_code=401, detail="Invalid or missing studio access token")


# --------------------------------------------------------------------------- #
# Schemas
# --------------------------------------------------------------------------- #

class AskRequest(BaseModel):
    question: str = Field(..., min_length=1, max_length=2000)
    media_ids: list[int] | None = None


def _ext(filename: str) -> str:
    name = filename or ""
    return ("." + name.rsplit(".", 1)[-1].lower()) if "." in name else ""


def _job_summary(item: MediaItem) -> dict:
    return {
        "id": item.id,
        "filename": item.filename,
        "status": item.status,
        "error": item.error,
        "language": item.language,
        "duration_seconds": item.duration_seconds,
        "word_count": item.word_count,
        "size_bytes": item.size_bytes,
        "has_transcript": bool(item.transcript),
        "created_at": item.created_at.isoformat() if item.created_at else None,
        "completed_at": item.completed_at.isoformat() if item.completed_at else None,
    }


# --------------------------------------------------------------------------- #
# Endpoints
# --------------------------------------------------------------------------- #

@router.get("/config")
async def studio_config() -> dict:
    """Capabilities for the UI (no auth so the client can discover requirements)."""
    return {
        "groq_configured": bool(settings.groq_api_key),
        "anthropic_configured": bool(settings.anthropic_api_key),
        "ffmpeg_available": media_prep.ffmpeg_available(),
        "qa_model": settings.qa_model,
        "whisper_model": settings.whisper_model,
        "access_required": bool(settings.studio_access_token),
        "max_upload_mb": settings.studio_max_upload_mb,
        "allowed_extensions": sorted(ALLOWED_EXTENSIONS),
    }


@router.post("/jobs", dependencies=[Depends(require_studio_access)])
async def create_jobs(
    files: list[UploadFile] = File(...),
    db: AsyncSession = Depends(get_db),
) -> dict:
    """Upload one or more media files; each becomes a queued transcription job."""
    if not files:
        raise HTTPException(status_code=400, detail="No files uploaded")

    max_bytes = settings.studio_max_upload_mb * _MB
    created: list[dict] = []
    errors: list[dict] = []

    for upload in files:
        filename = upload.filename or "upload"
        ext = _ext(filename)
        if ext not in ALLOWED_EXTENSIONS:
            errors.append({"filename": filename, "error": f"Unsupported format: {ext or 'unknown'}"})
            continue

        data = await upload.read()
        if not data:
            errors.append({"filename": filename, "error": "Empty file"})
            continue
        if len(data) > max_bytes:
            errors.append({
                "filename": filename,
                "error": f"File too large ({len(data) / _MB:.1f} MB; max {settings.studio_max_upload_mb} MB)",
            })
            continue

        path = studio_transcription.save_upload(data, filename)
        item = MediaItem(
            filename=filename,
            content_type=upload.content_type,
            size_bytes=len(data),
            status=STATUS_QUEUED,
            source_path=path,
        )
        db.add(item)
        await db.commit()
        await db.refresh(item)
        await studio_transcription.enqueue(item.id)
        created.append(_job_summary(item))

    if not created and errors:
        raise HTTPException(status_code=400, detail={"errors": errors})

    return {"created": created, "errors": errors}


@router.get("/jobs", dependencies=[Depends(require_studio_access)])
async def list_jobs(db: AsyncSession = Depends(get_db)) -> dict:
    result = await db.execute(select(MediaItem).order_by(MediaItem.created_at.desc()))
    items = result.scalars().all()
    return {"jobs": [_job_summary(item) for item in items]}


@router.get("/jobs/{media_id}", dependencies=[Depends(require_studio_access)])
async def get_job(media_id: int, db: AsyncSession = Depends(get_db)) -> dict:
    item = await db.get(MediaItem, media_id)
    if item is None:
        raise HTTPException(status_code=404, detail="Job not found")
    seg_count = await db.scalar(
        select(func.count(TranscriptSegment.id)).where(
            TranscriptSegment.media_id == media_id
        )
    )
    detail = _job_summary(item)
    detail["transcript"] = item.transcript
    detail["segment_count"] = int(seg_count or 0)
    return detail


@router.get(
    "/jobs/{media_id}/transcript",
    dependencies=[Depends(require_studio_access)],
    response_class=PlainTextResponse,
)
async def download_transcript(
    media_id: int,
    format: str = Query(default="txt", pattern="^(txt|srt)$"),
    db: AsyncSession = Depends(get_db),
) -> PlainTextResponse:
    item = await db.get(MediaItem, media_id)
    if item is None:
        raise HTTPException(status_code=404, detail="Job not found")
    if not item.transcript:
        raise HTTPException(status_code=409, detail="Transcript is not ready")

    base = (item.filename.rsplit(".", 1)[0] or "transcript")

    if format == "txt":
        body = item.transcript
        media_type = "text/plain"
        ext = "txt"
    else:
        result = await db.execute(
            select(TranscriptSegment)
            .where(TranscriptSegment.media_id == media_id)
            .order_by(TranscriptSegment.idx)
        )
        body = _to_srt(result.scalars().all())
        media_type = "application/x-subrip"
        ext = "srt"

    return PlainTextResponse(
        content=body,
        media_type=media_type,
        headers={"Content-Disposition": f'attachment; filename="{base}.{ext}"'},
    )


@router.delete("/jobs/{media_id}", dependencies=[Depends(require_studio_access)])
async def delete_job(media_id: int, db: AsyncSession = Depends(get_db)) -> dict:
    item = await db.get(MediaItem, media_id)
    if item is None:
        raise HTTPException(status_code=404, detail="Job not found")
    if item.source_path:
        studio_transcription._cleanup(item.source_path)
    await db.delete(item)  # segments cascade
    await db.commit()
    return {"deleted": media_id}


@router.get("/search", dependencies=[Depends(require_studio_access)])
async def search(
    q: str = Query(..., min_length=1),
    media_ids: list[int] | None = Query(default=None),
    limit: int = Query(default=20, ge=1, le=100),
    db: AsyncSession = Depends(get_db),
) -> dict:
    results = await transcript_qa.search_segments(db, q, media_ids=media_ids, limit=limit)
    return {"query": q, "results": results}


@router.post("/ask", dependencies=[Depends(require_studio_access)])
async def ask(req: AskRequest, db: AsyncSession = Depends(get_db)) -> dict:
    try:
        return await transcript_qa.answer_question(
            db, req.question, media_ids=req.media_ids
        )
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc))
    except Exception as exc:  # pragma: no cover - upstream/LLM failures
        logger.error("Q&A failed: %s: %s", type(exc).__name__, exc)
        raise HTTPException(status_code=502, detail="Question answering failed")


# --------------------------------------------------------------------------- #
# Helpers
# --------------------------------------------------------------------------- #

def _srt_timestamp(seconds: float | None) -> str:
    total_ms = int(round((seconds or 0.0) * 1000))
    hours, rem = divmod(total_ms, 3_600_000)
    minutes, rem = divmod(rem, 60_000)
    secs, ms = divmod(rem, 1000)
    return f"{hours:02d}:{minutes:02d}:{secs:02d},{ms:03d}"


def _to_srt(segments: list[TranscriptSegment]) -> str:
    blocks = []
    for i, seg in enumerate(segments, start=1):
        start = seg.start_seconds if seg.start_seconds is not None else 0.0
        end = seg.end_seconds if seg.end_seconds is not None else start + 3.0
        blocks.append(
            f"{i}\n{_srt_timestamp(start)} --> {_srt_timestamp(end)}\n{seg.text}\n"
        )
    return "\n".join(blocks)
