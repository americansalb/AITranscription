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


def _extract_user_id(request: Request) -> str:
    """Extract user identity from JWT token, falling back to client IP.

    NEVER trust client-supplied headers like X-User-Id for rate limiting.
    """
    auth_header = request.headers.get("Authorization", "")
    if auth_header.startswith("Bearer "):
        token = auth_header[7:]
        try:
            from app.api.auth import decode_access_token
            user_id = decode_access_token(token)
            if user_id is not None:
                return f"user:{user_id}"
        except Exception:
            pass

    # Fall back to client IP for unauthenticated requests
    client_ip = request.client.host if request.client else "unknown"
    return f"ip:{client_ip}"


class RateLimitMiddleware(BaseHTTPMiddleware):
    """FastAPI middleware that enforces per-user rate limits."""

    async def dispatch(self, request: Request, call_next):
        # Extract user identity from JWT or IP (never from client headers)
        identity = _extract_user_id(request)
        path = request.url.path

        # Stricter limit for provider proxy
        if "/api/v1/providers/" in path:
            key = f"provider:{identity}"
            limit = PROVIDER_RPM
        else:
            key = f"api:{identity}"
            limit = DEFAULT_RPM

        bucket = _buckets[key]
        if not bucket.allow(limit):
            logger.warning("Rate limited: identity=%s path=%s", identity, path)
            raise HTTPException(
                status_code=429,
                detail=f"Rate limit exceeded. Max {limit} requests per minute.",
            )

        return await call_next(request)
