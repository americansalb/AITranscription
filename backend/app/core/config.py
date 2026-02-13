from pathlib import Path

from pydantic_settings import BaseSettings

# Resolve .env relative to this file (backend/.env) so it works regardless of CWD
_THIS_DIR = Path(__file__).resolve().parent  # app/core/
_ENV_FILE = _THIS_DIR.parent.parent / ".env"  # backend/.env


class Settings(BaseSettings):
    """Application settings loaded from environment variables."""

    # API Keys
    groq_api_key: str = ""
    anthropic_api_key: str = ""
    openai_api_key: str = ""
    elevenlabs_api_key: str = ""
    elevenlabs_voice_id: str = "TlLCuK5N2ARR6OHBwD53"  # Default: AALB

    # Database
    database_url: str = "postgresql+asyncpg://localhost:5432/vaak"

    # Auth
    secret_key: str = "dev-secret-key-change-in-production"
    access_token_expire_minutes: int = 60 * 24 * 7  # 1 week

    # App settings
    app_name: str = "Vaak"
    debug: bool = False
    port: int = 19836  # Fixed high port unlikely to conflict

    # AI Models
    whisper_model: str = "whisper-large-v3-turbo"
    haiku_model: str = "claude-3-5-haiku-20241022"
    vision_model: str = "claude-3-5-haiku-20241022"

    # Rate limits
    max_audio_duration_seconds: int = 300  # 5 minutes max per request

    # ML Features (experimental)
    enable_ml_corrections: bool = False  # Feature flag for embedding-based learning

    model_config = {
        "env_file": str(_ENV_FILE),
        "env_file_encoding": "utf-8",
        "extra": "ignore",
    }


settings = Settings()
