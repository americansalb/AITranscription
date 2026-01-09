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
    """Application lifespan - run migrations and import models.

    Migrations run automatically at startup to ensure database schema is correct.
    """
    # Import models to register them with Base (required for relationships to work)
    from app.models import user, dictionary, learning  # noqa: F401

    # Run migrations automatically to ensure database schema is up to date
    import subprocess
    import sys
    try:
        subprocess.run(
            [sys.executable, "-m", "alembic", "upgrade", "head"],
            check=True,
            capture_output=True,
            text=True
        )
        print("✓ Database migrations completed successfully")
    except subprocess.CalledProcessError as e:
        print(f"⚠ Migration warning: {e.stderr}")
        # Don't crash if migrations fail - database might already be up to date

    yield


app = FastAPI(
    lifespan=lifespan,
    title=settings.app_name,
    description="AI-powered transcription API with Groq Whisper and Claude Haiku",
    version="0.1.0",
    docs_url="/docs",
    redoc_url="/redoc",
)

# CORS configuration for desktop app
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],  # Desktop app will connect from various origins
    allow_credentials=False,  # Must be False when using allow_origins=["*"]
    allow_methods=["*"],
    allow_headers=["*"],
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
