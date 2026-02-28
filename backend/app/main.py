import os
from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.api import router
from app.api.admin import router as admin_router
from app.api.auth import router as auth_router
from app.api.dictionary import router as dictionary_router
from app.api.learning import router as learning_router
from app.api.gamification import router as gamification_router
from app.api.audience import router as audience_router
from app.api.roles import router as roles_router
from app.core.config import settings
from app.core.database import engine
from app.models.base import Base

# CORS: restrict to Tauri app origins by default; override via CORS_ORIGINS env var
_DEFAULT_CORS_ORIGINS = [
    "http://localhost",
    "http://localhost:5173",
    "http://127.0.0.1:5173",
    "http://localhost:19836",
    "tauri://localhost",
    "https://tauri.localhost",
]
_cors_env = os.environ.get("CORS_ORIGINS", "")
CORS_ORIGINS = [o.strip() for o in _cors_env.split(",") if o.strip()] if _cors_env else _DEFAULT_CORS_ORIGINS


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan - import models and preload resources.

    All database migrations and seeding now happen during build time via:
    - alembic upgrade head (creates/updates schema)
    - python -m scripts.seed_admin --force (seeds dev accounts)

    This ensures the app can start quickly and bind to port immediately.
    """
    # Import models to register them with Base (required for relationships to work)
    from app.models import user, dictionary, learning, gamification  # noqa: F401

    # Note: Embedding model loads lazily on first use to reduce startup memory
    # from app.services.correction_retriever import preload_embedding_model
    # preload_embedding_model()

    yield


app = FastAPI(
    lifespan=lifespan,
    title=settings.app_name,
    description="AI-powered transcription API with Groq Whisper and Claude Haiku",
    version="0.1.0",
    debug=False,  # Never expose tracebacks in responses, even if DEBUG=true in .env
    docs_url="/docs" if settings.debug else None,   # Only expose docs in debug mode
    redoc_url="/redoc" if settings.debug else None,
)

# CORS configuration - restricted to Tauri app origins (override via CORS_ORIGINS env var)
app.add_middleware(
    CORSMiddleware,
    allow_origins=CORS_ORIGINS,
    allow_credentials=True,
    allow_methods=["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"],
    allow_headers=["Authorization", "Content-Type", "Accept"],
)

# Include API routes
app.include_router(router, prefix="/api/v1")
app.include_router(admin_router, prefix="/api/v1")
app.include_router(auth_router, prefix="/api/v1")
app.include_router(dictionary_router, prefix="/api/v1")
app.include_router(learning_router, prefix="/api/v1")
app.include_router(gamification_router, prefix="/api/v1")
app.include_router(audience_router, prefix="/api/v1")
app.include_router(roles_router, prefix="/api/v1")


# Mount Vaak Lite sub-app at /vaaklite
from app.vaaklite.app import vaaklite_app  # noqa: E402
app.mount("/vaaklite", vaaklite_app)


@app.get("/")
async def root():
    """Root endpoint with API info."""
    return {
        "name": settings.app_name,
        "version": "0.1.0",
        "docs": "/docs",
        "health": "/api/v1/health",
        "vaaklite": "/vaaklite",
    }
