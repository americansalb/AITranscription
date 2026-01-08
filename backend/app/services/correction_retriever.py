"""Correction retriever service for embedding-based learning."""
import logging
from functools import lru_cache
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


@lru_cache(maxsize=1)
def get_embedding_model() -> SentenceTransformer:
    """Load the embedding model (cached singleton)."""
    logger.info(f"Loading embedding model: {EMBEDDING_MODEL}")
    return SentenceTransformer(EMBEDDING_MODEL)


def compute_embedding(text: str) -> list[float]:
    """Compute embedding for a text string."""
    model = get_embedding_model()
    embedding = model.encode(text, convert_to_numpy=True)
    return embedding.tolist()


def compute_correction_embedding(original: str, corrected: str) -> list[float]:
    """Compute embedding for a correction pair.

    We embed the combined context to capture the relationship between
    the original and corrected text.
    """
    model = get_embedding_model()
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


# Singleton for embedding model preloading
def preload_embedding_model():
    """Preload the embedding model at startup."""
    try:
        get_embedding_model()
        logger.info("Embedding model preloaded successfully")
    except Exception as e:
        logger.error(f"Failed to preload embedding model: {e}")
