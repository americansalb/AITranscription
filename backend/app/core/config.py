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
    secret_key: str = ""  # Must be set via SECRET_KEY env var — no insecure default
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

    # API call timeouts (seconds) — provisional values, calibrate with production data
    groq_timeout_base: int = 30        # Base timeout for Groq transcription
    groq_timeout_per_min_audio: int = 30  # Additional seconds per minute of audio
    anthropic_timeout_base: int = 30   # Base timeout for Anthropic polish
    anthropic_timeout_per_1k_chars: int = 15  # Additional seconds per 1000 chars
    timeout_ceiling: int = 300         # Maximum timeout for any single API call (5 min)

    # ML Features (experimental)
    enable_ml_corrections: bool = False  # Feature flag for embedding-based learning

    model_config = {
        "env_file": str(_ENV_FILE),
        "env_file_encoding": "utf-8",
        "extra": "ignore",
    }


settings = Settings()

# Validate secret key at import time — app won't start without a proper secret
if not settings.secret_key or settings.secret_key == "dev-secret-key-change-in-production":
    raise RuntimeError(
        "FATAL: SECRET_KEY environment variable is not set or is still the "
        "insecure default. Set a strong, random SECRET_KEY in your .env file "
        "or environment before starting the application."
    )
