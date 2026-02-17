"""Correction retriever service for embedding-based learning."""
import gc
import logging
import psutil
from contextlib import contextmanager
from typing import Optional

import numpy as np
from sentence_transformers import SentenceTransformer
from sqlalchemy import select, text, update
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.learning import CorrectionEmbedding

logger = logging.getLogger(__name__)

# Embedding model configuration
EMBEDDING_MODEL = "all-MiniLM-L6-v2"
EMBEDDING_DIM = 384

# Global model reference for lifecycle management
_embedding_model: Optional[SentenceTransformer] = None


def get_available_memory_mb() -> float:
    """Get available system memory in MB."""
    try:
        return psutil.virtual_memory().available / (1024 * 1024)
    except Exception:
        return 0.0


@contextmanager
def get_embedding_model():
    """Context manager for loading/unloading embedding model.

    Usage:
        with get_embedding_model() as model:
            embedding = model.encode(text)

    This ensures the model is loaded on-demand and unloaded after use
    to minimize memory footprint.
    """
    global _embedding_model

    # Check available memory before loading
    available_mb = get_available_memory_mb()
    logger.info(f"Available memory: {available_mb:.0f} MB")

    if available_mb < 100 and available_mb > 0:
        logger.warning(f"Low memory ({available_mb:.0f} MB), skipping model load")
        raise MemoryError(f"Insufficient memory to load embedding model ({available_mb:.0f} MB available)")

    # Load model if not already loaded
    if _embedding_model is None:
        logger.info(f"Loading embedding model: {EMBEDDING_MODEL}")
        _embedding_model = SentenceTransformer(EMBEDDING_MODEL)
        logger.info(f"Model loaded. Available memory: {get_available_memory_mb():.0f} MB")

    try:
        yield _embedding_model
    finally:
        # Unload model after use to free memory
        if _embedding_model is not None:
            logger.info("Unloading embedding model to free memory")
            _embedding_model = None
            gc.collect()  # Force garbage collection
            logger.info(f"Model unloaded. Available memory: {get_available_memory_mb():.0f} MB")


def compute_embedding(text: str) -> list[float]:
    """Compute embedding for a text string."""
    with get_embedding_model() as model:
        embedding = model.encode(text, convert_to_numpy=True)
        return embedding.tolist()


def compute_correction_embedding(original: str, corrected: str) -> list[float]:
    """Compute embedding for a correction pair.

    We embed the combined context to capture the relationship between
    the original and corrected text.
    """
    with get_embedding_model() as model:
        # Combine original and corrected to capture the correction pattern
        combined = f"Original: {original}\nCorrected: {corrected}"
        embedding = model.encode(combined, convert_to_numpy=True)
        return embedding.tolist()


def classify_correction_type(original: str, corrected: str) -> str:
    """Classify the type of correction made."""
    original_lower = original.lower()
    corrected_lower = corrected.lower()

    # Filler word removal
    filler_words = {"um", "uh", "er", "ah", "hmm", "like", "you know"}
    original_words = set(original_lower.split())
    corrected_words = set(corrected_lower.split())
    removed_words = original_words - corrected_words
    if removed_words & filler_words:
        return "filler"

    # Punctuation only change
    original_alphanum = "".join(c for c in original if c.isalnum() or c.isspace())
    corrected_alphanum = "".join(c for c in corrected if c.isalnum() or c.isspace())
    if original_alphanum.lower() == corrected_alphanum.lower():
        return "punctuation"

    # Spelling correction (similar length, different characters)
    if abs(len(original) - len(corrected)) <= 3:
        # Check if words are similar (Levenshtein-like heuristic)
        if len(original_words) == len(corrected_words):
            return "spelling"

    # Vocabulary change
    if len(removed_words) > 0 or len(corrected_words - original_words) > 0:
        return "vocabulary"

    # Default to grammar
    return "grammar"


class CorrectionRetriever:
    """Service for storing and retrieving correction patterns."""

    def __init__(self, db: AsyncSession, user_id: int):
        self.db = db
        self.user_id = user_id

    async def store_correction(
        self,
        original: str,
        corrected: str,
        audio_sample_id: Optional[int] = None,
    ) -> CorrectionEmbedding:
        """Store a correction pair with its embedding.

        If a similar correction already exists (>95% similarity),
        increment its count instead of creating a duplicate.
        """
        # Skip if texts are identical
        if original.strip() == corrected.strip():
            return None

        # Check for existing similar correction
        existing = await self.find_similar(original, threshold=0.95)
        if existing:
            # Increment count on existing correction
            await self.db.execute(
                update(CorrectionEmbedding)
                .where(CorrectionEmbedding.id == existing[0]["id"])
                .values(correction_count=CorrectionEmbedding.correction_count + 1)
            )
            await self.db.commit()
            return await self.db.get(CorrectionEmbedding, existing[0]["id"])

        # Compute embedding and classify
        embedding = compute_correction_embedding(original, corrected)
        correction_type = classify_correction_type(original, corrected)

        # Create new correction
        correction = CorrectionEmbedding(
            user_id=self.user_id,
            original_text=original,
            corrected_text=corrected,
            embedding=embedding,
            correction_type=correction_type,
            audio_sample_id=audio_sample_id,
        )
        self.db.add(correction)
        await self.db.commit()
        await self.db.refresh(correction)

        logger.info(
            f"Stored correction for user {self.user_id}: "
            f"'{original[:50]}...' -> '{corrected[:50]}...' ({correction_type})"
        )
        return correction

    async def find_similar(
        self,
        text: str,
        threshold: float = 0.7,
        limit: int = 5,
    ) -> list[dict]:
        """Find corrections similar to the given text."""
        # Cap limit to prevent unbounded queries
        limit = min(limit, 50)
        # Truncate excessively long text before embedding (embedding models have token limits)
        if len(text) > 2000:
            text = text[:2000]
        query_embedding = compute_embedding(text)

        # Use pgvector's cosine distance operator (<=>)
        # Cosine distance = 1 - cosine_similarity
        result = await self.db.execute(
            text("""
                SELECT
                    id,
                    original_text,
                    corrected_text,
                    correction_type,
                    correction_count,
                    1 - (embedding <=> :embedding) as similarity
                FROM correction_embeddings
                WHERE user_id = :user_id
                  AND embedding IS NOT NULL
                  AND 1 - (embedding <=> :embedding) > :threshold
                ORDER BY embedding <=> :embedding
                LIMIT :limit
            """),
            {
                "embedding": str(query_embedding),
                "user_id": self.user_id,
                "threshold": threshold,
                "limit": limit,
            },
        )

        return [
            {
                "id": row.id,
                "original_text": row.original_text,
                "corrected_text": row.corrected_text,
                "correction_type": row.correction_type,
                "correction_count": row.correction_count,
                "similarity": row.similarity,
            }
            for row in result
        ]

    async def retrieve_relevant_corrections(
        self,
        transcript: str,
        top_k: int = 5,
        threshold: float = 0.6,
    ) -> list[dict]:
        """Retrieve corrections relevant to a transcript.

        This is the main method called during polishing to find
        similar past corrections that can guide the LLM.
        """
        # For longer transcripts, split into sentences and search each
        sentences = self._split_sentences(transcript)

        all_corrections = []
        seen_ids = set()

        for sentence in sentences[:5]:  # Limit to first 5 sentences
            similar = await self.find_similar(
                sentence,
                threshold=threshold,
                limit=top_k,
            )
            for correction in similar:
                if correction["id"] not in seen_ids:
                    all_corrections.append(correction)
                    seen_ids.add(correction["id"])

        # Sort by similarity and limit
        all_corrections.sort(key=lambda x: x["similarity"], reverse=True)
        return all_corrections[:top_k]

    async def get_correction_stats(self) -> dict:
        """Get statistics about learned corrections for this user."""
        result = await self.db.execute(
            text("""
                SELECT
                    COUNT(*) as total_corrections,
                    COUNT(DISTINCT correction_type) as unique_types,
                    SUM(correction_count) as total_applications,
                    AVG(correction_count) as avg_frequency
                FROM correction_embeddings
                WHERE user_id = :user_id
            """),
            {"user_id": self.user_id},
        )
        row = result.fetchone()

        # Get breakdown by type
        type_result = await self.db.execute(
            text("""
                SELECT correction_type, COUNT(*) as count
                FROM correction_embeddings
                WHERE user_id = :user_id
                GROUP BY correction_type
            """),
            {"user_id": self.user_id},
        )
        type_breakdown = {row.correction_type: row.count for row in type_result}

        return {
            "total_corrections": row.total_corrections or 0,
            "unique_types": row.unique_types or 0,
            "total_applications": row.total_applications or 0,
            "avg_frequency": float(row.avg_frequency or 0),
            "by_type": type_breakdown,
        }

    async def delete_correction(self, correction_id: int) -> bool:
        """Delete a learned correction (for user cleanup)."""
        result = await self.db.execute(
            select(CorrectionEmbedding).where(
                CorrectionEmbedding.id == correction_id,
                CorrectionEmbedding.user_id == self.user_id,
            )
        )
        correction = result.scalar_one_or_none()

        if correction:
            await self.db.delete(correction)
            await self.db.commit()
            return True
        return False

    async def get_recent_corrections(self, limit: int = 20) -> list[dict]:
        """Get recent corrections for display."""
        result = await self.db.execute(
            select(CorrectionEmbedding)
            .where(CorrectionEmbedding.user_id == self.user_id)
            .order_by(CorrectionEmbedding.created_at.desc())
            .limit(limit)
        )

        return [
            {
                "id": c.id,
                "original_text": c.original_text,
                "corrected_text": c.corrected_text,
                "correction_type": c.correction_type,
                "correction_count": c.correction_count,
                "created_at": c.created_at.isoformat() if c.created_at else None,
            }
            for c in result.scalars()
        ]

    def _split_sentences(self, text: str) -> list[str]:
        """Split text into sentences for individual embedding."""
        # Simple sentence splitting
        import re
        sentences = re.split(r'(?<=[.!?])\s+', text)
        return [s.strip() for s in sentences if s.strip()]


# Note: Preloading is no longer used with context manager approach
# The model is loaded on-demand and unloaded after use to minimize memory footprint
def preload_embedding_model():
    """Preload function (deprecated with context manager approach).

    With the new context manager approach, the model is loaded on-demand
    and unloaded after use, so preloading is no longer necessary.
    """
    logger.info("Model preloading disabled - using on-demand loading with context manager")
