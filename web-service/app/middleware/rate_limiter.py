"""Rate limiting middleware — per-user, per-provider throttling.

Uses in-memory counters for MVP. Switch to Redis for horizontal scaling.
"""

import time
import logging
from collections import defaultdict
from dataclasses import dataclass, field

from fastapi import Request, HTTPException
from starlette.middleware.base import BaseHTTPMiddleware

logger = logging.getLogger(__name__)

# Rate limit: requests per minute per user
DEFAULT_RPM = 60
PROVIDER_RPM = 30  # Stricter limit for LLM proxy calls


@dataclass
class RateBucket:
    """Sliding window rate limiter for a single key."""

    timestamps: list[float] = field(default_factory=list)

    def allow(self, limit: int, window_seconds: float = 60.0) -> bool:
        now = time.monotonic()
        cutoff = now - window_seconds
        self.timestamps = [t for t in self.timestamps if t > cutoff]
        if len(self.timestamps) >= limit:
            return False
        self.timestamps.append(now)
        return True


# In-memory buckets: key -> RateBucket
_buckets: dict[str, RateBucket] = defaultdict(RateBucket)


class RateLimitMiddleware(BaseHTTPMiddleware):
    """FastAPI middleware that enforces per-user rate limits."""

    async def dispatch(self, request: Request, call_next):
        # Extract user ID from JWT (simplified — real implementation checks auth)
        user_id = request.headers.get("X-User-Id", "anonymous")
        path = request.url.path

        # Stricter limit for provider proxy
        if "/api/v1/providers/" in path:
            key = f"provider:{user_id}"
            limit = PROVIDER_RPM
        else:
            key = f"api:{user_id}"
            limit = DEFAULT_RPM

        bucket = _buckets[key]
        if not bucket.allow(limit):
            logger.warning("Rate limited: user=%s path=%s", user_id, path)
            raise HTTPException(
                status_code=429,
                detail=f"Rate limit exceeded. Max {limit} requests per minute.",
            )

        return await call_next(request)
