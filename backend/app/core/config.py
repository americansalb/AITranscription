from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    """Application settings loaded from environment variables."""

    # API Keys
    groq_api_key: str = ""
    anthropic_api_key: str = ""
    elevenlabs_api_key: str = ""
    elevenlabs_voice_id: str = "21m00Tcm4TlvDq8ikWAM"  # Default: Rachel

    # Database
    database_url: str = "postgresql+asyncpg://localhost:5432/scribe"

    # Auth
    secret_key: str = "dev-secret-key-change-in-production"
    access_token_expire_minutes: int = 60 * 24 * 7  # 1 week

    # App settings
    app_name: str = "Scribe"
    debug: bool = False

    # AI Models
    whisper_model: str = "whisper-large-v3-turbo"
    haiku_model: str = "claude-3-5-haiku-20241022"

    # Rate limits
    max_audio_duration_seconds: int = 300  # 5 minutes max per request

    # ML Features (experimental)
    enable_ml_corrections: bool = False  # Feature flag for embedding-based learning

    model_config = {
        "env_file": ".env",
        "env_file_encoding": "utf-8",
        "extra": "ignore",
    }


settings = Settings()
