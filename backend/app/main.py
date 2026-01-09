from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.api import router
from app.api.auth import router as auth_router
from app.api.dictionary import router as dictionary_router
from app.api.learning import router as learning_router
from app.core.config import settings
from app.core.database import engine
from app.models.base import Base


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan - import models and preload resources.

    All database migrations and seeding now happen during build time via:
    - alembic upgrade head (creates/updates schema)
    - python -m scripts.seed_admin --force (seeds dev accounts)

    This ensures the app can start quickly and bind to port immediately.
    """
    # Import models to register them with Base (required for relationships to work)
    from app.models import user, dictionary, learning  # noqa: F401

    # Note: Embedding model loads lazily on first use to reduce startup memory
    # from app.services.correction_retriever import preload_embedding_model
    # preload_embedding_model()

    yield


app = FastAPI(
    lifespan=lifespan,
    title=settings.app_name,
    description="AI-powered transcription API with Groq Whisper and Claude Haiku",
    version="0.1.0",
    docs_url="/docs",
    redoc_url="/redoc",
)

# CORS configuration - allow all origins for Tauri desktop app
app.add_middleware(
    CORSMiddleware,
    allow_origin_regex=r".*",  # Allows any origin including tauri://localhost
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
    expose_headers=["*"],
)

# Include API routes
app.include_router(router, prefix="/api/v1")
app.include_router(auth_router, prefix="/api/v1")
app.include_router(dictionary_router, prefix="/api/v1")
app.include_router(learning_router, prefix="/api/v1")


@app.get("/")
async def root():
    """Root endpoint with API info."""
    return {
        "name": settings.app_name,
        "version": "0.1.0",
        "docs": "/docs",
        "health": "/api/v1/health",
    }
