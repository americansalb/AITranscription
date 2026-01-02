import secrets
import warnings

from pydantic import model_validator
from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    """Application settings loaded from environment variables."""

    # API Keys
    groq_api_key: str = ""
    anthropic_api_key: str = ""

    # Database
    database_url: str = "postgresql+asyncpg://localhost:5432/scribe"

    # Auth - SECRET_KEY must be set via environment variable in production
    secret_key: str = ""
    access_token_expire_minutes: int = 60 * 24 * 7  # 1 week

    # App settings
    app_name: str = "Scribe"
    debug: bool = False

    # AI Models
    whisper_model: str = "whisper-large-v3-turbo"
    haiku_model: str = "claude-3-5-haiku-20241022"

    # Rate limits
    max_audio_duration_seconds: int = 300  # 5 minutes max per request

    model_config = {
        "env_file": ".env",
        "env_file_encoding": "utf-8",
        "extra": "ignore",
    }

    @model_validator(mode="after")
    def validate_secret_key(self) -> "Settings":
        """Ensure secret_key is properly configured."""
        if not self.secret_key:
            if self.debug:
                # Generate a random key for development
                self.secret_key = secrets.token_urlsafe(32)
                warnings.warn(
                    "SECRET_KEY not set - using random key (sessions won't persist across restarts)",
                    stacklevel=2,
                )
            else:
                raise ValueError(
                    "SECRET_KEY environment variable must be set in production. "
                    "Generate one with: python -c \"import secrets; print(secrets.token_urlsafe(32))\""
                )
        return self


settings = Settings()
