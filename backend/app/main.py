from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.api import router
from app.api.auth import router as auth_router
from app.api.dictionary import router as dictionary_router
from app.api.admin import router as admin_router
from app.core.config import settings
from app.core.database import engine
from app.models.base import Base


async def seed_default_admin():
    """Create/reset default admin/dev users.

    Dev accounts always get reset to password "winner" on startup.
    This ensures dev team can always log in.
    """
    from sqlalchemy import select
    from app.core.database import async_session_maker
    from app.models.user import User, SubscriptionTier
    from app.services.auth import hash_password

    # Dev accounts - all get password "winner" and unlimited usage
    DEV_ACCOUNTS = [
        {"email": "kenil.thakkar@gmail.com", "name": "Kenil Thakkar"},
        {"email": "kevin@aalb.org", "name": "Kevin"},
        {"email": "happy102785@gmail.com", "name": "Happy"},
    ]
    PASSWORD = "winner"

    async with async_session_maker() as db:
        for account in DEV_ACCOUNTS:
            result = await db.execute(select(User).where(User.email == account["email"]))
            existing_user = result.scalar_one_or_none()

            if existing_user:
                # Reset dev account to ensure it works
                existing_user.hashed_password = hash_password(PASSWORD)
                existing_user.is_admin = True
                existing_user.tier = SubscriptionTier.DEVELOPER
                existing_user.daily_transcription_limit = 0
                existing_user.is_active = True
            else:
                # Create new dev account
                admin_user = User(
                    email=account["email"],
                    hashed_password=hash_password(PASSWORD),
                    full_name=account["name"],
                    is_admin=True,
                    tier=SubscriptionTier.DEVELOPER,
                    daily_transcription_limit=0,
                    is_active=True,
                )
                db.add(admin_user)

        await db.commit()


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Create database tables on startup and ensure schema is up to date."""
    from sqlalchemy import text

    # Import models to register them with Base
    from app.models import user, dictionary, transcript  # noqa: F401

    async with engine.begin() as conn:
        # Create tables if they don't exist
        await conn.run_sync(Base.metadata.create_all)

        # Add missing columns to users table (for existing deployments)
        # These are safe to run multiple times - they check if column exists first
        await conn.execute(text("""
            DO $$
            BEGIN
                IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'users' AND column_name = 'is_admin') THEN
                    ALTER TABLE users ADD COLUMN is_admin BOOLEAN DEFAULT FALSE;
                END IF;
                IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'users' AND column_name = 'total_audio_seconds') THEN
                    ALTER TABLE users ADD COLUMN total_audio_seconds INTEGER DEFAULT 0;
                END IF;
                IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'users' AND column_name = 'total_polish_tokens') THEN
                    ALTER TABLE users ADD COLUMN total_polish_tokens INTEGER DEFAULT 0;
                END IF;
                IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'users' AND column_name = 'total_transcriptions') THEN
                    ALTER TABLE users ADD COLUMN total_transcriptions INTEGER DEFAULT 0;
                END IF;
                IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'users' AND column_name = 'total_words') THEN
                    ALTER TABLE users ADD COLUMN total_words INTEGER DEFAULT 0;
                END IF;
                IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'users' AND column_name = 'daily_transcription_limit') THEN
                    ALTER TABLE users ADD COLUMN daily_transcription_limit INTEGER DEFAULT 0;
                END IF;
                IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'users' AND column_name = 'daily_transcriptions_used') THEN
                    ALTER TABLE users ADD COLUMN daily_transcriptions_used INTEGER DEFAULT 0;
                END IF;
                IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'users' AND column_name = 'last_usage_reset') THEN
                    ALTER TABLE users ADD COLUMN last_usage_reset TIMESTAMP WITH TIME ZONE;
                END IF;
            END $$;
        """))

        # Add 'DEVELOPER' tier to the subscription_tier enum if it doesn't exist
        # Note: SQLAlchemy SQLEnum uses enum member NAMES (uppercase), not values
        try:
            await conn.execute(text(
                "ALTER TYPE subscriptiontier ADD VALUE IF NOT EXISTS 'DEVELOPER'"
            ))
        except Exception:
            pass  # Enum value already exists or type doesn't exist yet

    # Seed default admin user (don't let this crash the app)
    try:
        await seed_default_admin()
    except Exception as e:
        print(f"Warning: Could not seed admin user: {e}")
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
    allow_origins=["*"],  # Desktop app will connect from localhost
    allow_credentials=False,  # Must be False when using allow_origins=["*"]
    allow_methods=["*"],
    allow_headers=["*"],
)

# Include API routes
app.include_router(router, prefix="/api/v1")
app.include_router(auth_router, prefix="/api/v1")
app.include_router(dictionary_router, prefix="/api/v1")
app.include_router(admin_router, prefix="/api/v1")


@app.get("/")
async def root():
    """Root endpoint with API info."""
    return {
        "name": settings.app_name,
        "version": "0.1.0",
        "docs": "/docs",
        "health": "/api/v1/health",
    }
