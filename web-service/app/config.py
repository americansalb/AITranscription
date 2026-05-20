"""Web service configuration — loaded from environment variables.

IMPORTANT: The agent runtime uses in-memory state (_active_agents dict).
Run with --workers 1 to avoid state desync between processes.
For horizontal scaling, a DB-backed agent state table or Redis is needed.
"""

from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    """Web service settings. All values can be overridden via environment variables."""

    # Service
    app_name: str = "Vaak Web"
    version: str = "0.1.0"
    debug: bool = False

    # Database (PostgreSQL for web, not SQLite)
    database_url: str = "postgresql+asyncpg://vaak:vaak@localhost:5432/vaak_web"

    # Auth
    secret_key: str  # REQUIRED — no default, fails if not set
    access_token_expire_minutes: int = 60 * 24  # 24 hours
    google_client_id: str = ""
    google_client_secret: str = ""
    github_client_id: str = ""
    github_client_secret: str = ""

    # BYOK key encryption (Fernet symmetric — generate with: python -c "from cryptography.fernet import Fernet; print(Fernet.generate_key().decode())")
    fernet_key: str = ""  # Empty = no encryption (dev mode). Set in production!

    # Platform API keys (OUR keys — used for default tier users)
    anthropic_api_key: str = ""
    openai_api_key: str = ""
    google_ai_api_key: str = ""

    # Billing (Stripe)
    stripe_secret_key: str = ""
    stripe_webhook_secret: str = ""
    stripe_price_pro: str = ""       # Stripe Price ID for Pro tier
    stripe_price_byok: str = ""      # Stripe Price ID for BYOK tier

    # Usage limits (tokens per month)
    free_tier_monthly_tokens: int = 50_000
    pro_tier_monthly_tokens: int = 2_000_000

    # Cost markup (multiplier on raw API cost)
    markup_multiplier: float = 2.0

    # Safety: per-message cost ceiling (USD) — reject if estimated cost exceeds this
    max_cost_per_message: float = 5.0

    # Safety: per-project session budget (USD)
    max_cost_per_session: float = 50.0

    # Agent runtime
    agent_poll_interval_seconds: float = 2.0
    agent_max_context_tokens: int = 100_000
    agent_max_response_tokens: int = 4096
    agent_completion_timeout_seconds: int = 120

    # CORS
    cors_origins: list[str] = ["http://localhost:5173", "http://localhost:3000"]

    model_config = {"env_file": ".env", "env_prefix": "VAAK_WEB_"}


settings = Settings()
