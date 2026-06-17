"""Search and question-answering over transcripts using Claude Haiku.

Retrieval is deliberately simple and dependency-free so it works on both SQLite (tests)
and Postgres (production) and needs no embedding model: candidate transcript segments are
pre-filtered in the database with case-insensitive ``ILIKE`` on the question's keywords,
then scored in Python by how many distinct keywords they match. The top segments become
cited context for a single Claude Haiku call, which answers strictly from that context.

For a modest internal library this is fast and accurate enough; swapping in pgvector /
full-text ranking later only means changing ``_candidate_segments``.
"""

from __future__ import annotations

import logging
import re

from sqlalchemy import or_, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.config import settings
from app.models.media_library import STATUS_COMPLETED, MediaItem, TranscriptSegment

logger = logging.getLogger(__name__)

# Common words that add noise to keyword matching.
_STOPWORDS = {
    "the", "and", "for", "are", "but", "not", "you", "your", "with", "this", "that",
    "from", "what", "when", "where", "which", "who", "whom", "how", "why", "was", "were",
    "has", "have", "had", "does", "did", "can", "will", "would", "could", "should",
    "about", "into", "over", "than", "then", "they", "them", "their", "there", "here",
    "a", "an", "of", "to", "in", "on", "is", "it", "as", "at", "be", "or", "do", "did",
}


def extract_keywords(text: str) -> list[str]:
    """Lowercased, de-duplicated content words (length > 2, not stopwords)."""
    words = re.findall(r"[A-Za-z0-9']+", (text or "").lower())
    seen: list[str] = []
    for word in words:
        if len(word) > 2 and word not in _STOPWORDS and word not in seen:
            seen.append(word)
    return seen


def _format_timestamp(seconds: float | None) -> str:
    if seconds is None:
        return "--:--"
    seconds = int(seconds)
    if seconds >= 3600:
        return f"{seconds // 3600:d}:{(seconds % 3600) // 60:02d}:{seconds % 60:02d}"
    return f"{seconds // 60:02d}:{seconds % 60:02d}"


async def _candidate_segments(
    db: AsyncSession,
    keywords: list[str],
    media_ids: list[int] | None,
    pool: int,
) -> list[tuple[TranscriptSegment, str]]:
    """Fetch (segment, media_filename) rows matching any keyword (or a recent sample)."""
    stmt = select(TranscriptSegment, MediaItem.filename).join(
        MediaItem, TranscriptSegment.media_id == MediaItem.id
    )
    if media_ids:
        stmt = stmt.where(TranscriptSegment.media_id.in_(media_ids))
    else:
        stmt = stmt.where(MediaItem.status == STATUS_COMPLETED)

    if keywords:
        stmt = stmt.where(
            or_(*[TranscriptSegment.text.ilike(f"%{kw}%") for kw in keywords])
        )

    stmt = stmt.limit(pool)
    result = await db.execute(stmt)
    return [(row[0], row[1]) for row in result.all()]


def _score(text: str, keywords: list[str]) -> int:
    lowered = text.lower()
    return sum(1 for kw in keywords if kw in lowered)


async def search_segments(
    db: AsyncSession,
    query: str,
    media_ids: list[int] | None = None,
    limit: int = 20,
) -> list[dict]:
    """Return transcript snippets ranked by keyword overlap with ``query``."""
    keywords = extract_keywords(query)
    if not keywords:
        return []

    candidates = await _candidate_segments(db, keywords, media_ids, pool=400)
    scored = []
    for segment, filename in candidates:
        score = _score(segment.text, keywords)
        if score:
            scored.append((score, segment, filename))
    scored.sort(key=lambda x: (-x[0], x[1].start_seconds or 0.0))

    return [
        {
            "media_id": segment.media_id,
            "filename": filename,
            "start_seconds": segment.start_seconds,
            "timestamp": _format_timestamp(segment.start_seconds),
            "text": segment.text,
            "score": score,
        }
        for score, segment, filename in scored[:limit]
    ]


async def answer_question(
    db: AsyncSession,
    question: str,
    media_ids: list[int] | None = None,
) -> dict:
    """Answer ``question`` from transcript context using Claude Haiku, with citations."""
    if not settings.anthropic_api_key:
        raise ValueError("ANTHROPIC_API_KEY is not configured")
    if not (question or "").strip():
        raise ValueError("Question cannot be empty")

    keywords = extract_keywords(question)
    max_segments = settings.studio_qa_max_segments

    ranked = await search_segments(db, question, media_ids=media_ids, limit=max_segments)
    if not ranked:
        # No keyword hits — fall back to a chronological sample so Claude still has
        # something to work with (e.g. broad questions like "summarise this").
        candidates = await _candidate_segments(db, [], media_ids, pool=max_segments)
        ranked = [
            {
                "media_id": seg.media_id,
                "filename": filename,
                "start_seconds": seg.start_seconds,
                "timestamp": _format_timestamp(seg.start_seconds),
                "text": seg.text,
                "score": 0,
            }
            for seg, filename in candidates[:max_segments]
        ]

    if not ranked:
        return {
            "answer": "There are no transcripts available yet to answer from.",
            "sources": [],
            "model": settings.qa_model,
        }

    context_lines = []
    for i, src in enumerate(ranked, start=1):
        context_lines.append(
            f"[{i}] ({src['filename']} @ {src['timestamp']}): {src['text']}"
        )
    context = "\n\n".join(context_lines)

    system_prompt = (
        "You answer questions about a collection of audio/video transcripts. "
        "Use ONLY the numbered transcript excerpts provided as CONTEXT. "
        "Cite the excerpts you rely on inline using their bracketed numbers, e.g. [1], [2]. "
        "If the context does not contain the answer, say so plainly instead of guessing. "
        "Be concise and quote the transcript where helpful."
    )
    user_prompt = f"CONTEXT:\n{context}\n\nQUESTION: {question}"

    from anthropic import AsyncAnthropic

    client = AsyncAnthropic(api_key=settings.anthropic_api_key)
    response = await client.messages.create(
        model=settings.qa_model,
        max_tokens=1024,
        system=system_prompt,
        messages=[{"role": "user", "content": user_prompt}],
    )
    answer = "".join(
        block.text for block in response.content if getattr(block, "type", None) == "text"
    ).strip()

    return {
        "answer": answer,
        "sources": ranked,
        "model": settings.qa_model,
    }
