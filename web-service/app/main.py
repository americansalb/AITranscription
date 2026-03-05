"""Vaak Web Service — FastAPI entry point.

Separate from the desktop backend. Shares models/schemas via the shared/ package.
"""

import logging
import os
from contextlib import asynccontextmanager

from fastapi import FastAPI, Request, Response
from fastapi.middleware.cors import CORSMiddleware
from starlette.middleware.base import BaseHTTPMiddleware

from app.config import settings
from app.api import auth, projects, messages, billing, providers, discussions
from app.database import init_db
from app.middleware.rate_limiter import RateLimitMiddleware


class SecurityHeadersMiddleware(BaseHTTPMiddleware):
    """Add security headers to all responses."""

    async def dispatch(self, request: Request, call_next):
        response: Response = await call_next(request)
        response.headers["X-Content-Type-Options"] = "nosniff"
        response.headers["X-Frame-Options"] = "DENY"
        response.headers["X-XSS-Protection"] = "1; mode=block"
        response.headers["Referrer-Policy"] = "strict-origin-when-cross-origin"
        response.headers["Permissions-Policy"] = "camera=(), microphone=(), geolocation=()"
        # CSP: allow self + inline styles (needed for some UI frameworks)
        response.headers["Content-Security-Policy"] = (
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; "
            "img-src 'self' data:; font-src 'self'; connect-src 'self' wss: ws:; "
            "frame-ancestors 'none'"
        )
        # HSTS: only in production (not behind reverse proxy in dev)
        if not settings.secret_key.startswith("change-me"):
            response.headers["Strict-Transport-Security"] = (
                "max-age=31536000; includeSubDomains"
            )
        return response

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Startup/shutdown lifecycle."""
    logger.info("Starting %s v%s", settings.app_name, settings.version)
    await init_db()
    logger.info("Database initialized")

    # Security warnings
    if not settings.fernet_key:
        logger.warning(
            "VAAK_WEB_FERNET_KEY not set — BYOK API keys will be stored UNENCRYPTED. "
            "Generate one with: python -c \"from cryptography.fernet import Fernet; print(Fernet.generate_key().decode())\""
        )
    if settings.secret_key == "change-me-in-production" or len(settings.secret_key) < 32:
        if os.environ.get("VAAK_WEB_TESTING"):
            logger.warning("VAAK_WEB_SECRET_KEY is insecure — allowed because VAAK_WEB_TESTING=1")
        else:
            raise RuntimeError(
                "VAAK_WEB_SECRET_KEY is insecure (default value or shorter than 32 chars). "
                "Set a strong secret: python -c \"import secrets; print(secrets.token_urlsafe(48))\""
            )

    # Agent runtime limitation: in-memory agent registry is per-process.
    # Running with --workers > 1 causes duplicate agents and billing desync.
    web_concurrency = os.environ.get("WEB_CONCURRENCY", "")
    if web_concurrency and int(web_concurrency) > 1:
        logger.warning(
            "WEB_CONCURRENCY=%s detected. Agent runtime requires --workers 1 "
            "(in-memory agent registry is per-process). Multiple workers will cause "
            "duplicate agents and double-billing. Use --workers 1 for now.",
            web_concurrency,
        )

    yield
    logger.info("Shutting down")


app = FastAPI(
    title=settings.app_name,
    version=settings.version,
    description="Multi-provider AI collaboration platform",
    lifespan=lifespan,
)

# Middleware stack is LIFO — add in reverse order of desired execution:
# Request → RateLimit → CORS → SecurityHeaders → handler
app.add_middleware(SecurityHeadersMiddleware)
app.add_middleware(RateLimitMiddleware)
app.add_middleware(
    CORSMiddleware,
    allow_origins=settings.cors_origins,
    allow_credentials=True,
    allow_methods=["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"],
    allow_headers=["Content-Type", "Authorization", "Accept"],
)

# Routes
app.include_router(auth.router, prefix="/api/v1/auth", tags=["auth"])
app.include_router(projects.router, prefix="/api/v1/projects", tags=["projects"])
app.include_router(messages.router, prefix="/api/v1/messages", tags=["messages"])
app.include_router(billing.router, prefix="/api/v1/billing", tags=["billing"])
app.include_router(providers.router, prefix="/api/v1/providers", tags=["providers"])
app.include_router(discussions.router, prefix="/api/v1/projects", tags=["discussions"])


@app.get("/health")
async def health_check():
    return {
        "status": "ok",
        "version": settings.version,
    }
