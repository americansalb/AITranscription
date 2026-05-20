"""Shared billing/metering service — single source of truth for usage limits,
BYOK key lookup, and usage recording.

Extracted from providers.py and agent_runtime.py to eliminate billing logic
duplication. Both the REST /completion endpoint and the server-side agent
runtime call these functions.
"""

import logging
from datetime import datetime, timedelta, timezone

from sqlalchemy import func, select, update
from sqlalchemy.ext.asyncio import AsyncSession

from app.config import settings
from app.models import SubscriptionTier, UsageRecord, WebUser

logger = logging.getLogger(__name__)


class BillingLimitExceeded(Exception):
    """Raised when a completion would exceed billing limits (monthly tokens or session budget)."""

    pass


def get_monthly_limit(tier: SubscriptionTier) -> int:
    """Return the monthly token limit for a subscription tier."""
    if tier == SubscriptionTier.FREE:
        return settings.free_tier_monthly_tokens
    elif tier == SubscriptionTier.PRO:
        return settings.pro_tier_monthly_tokens
    elif tier == SubscriptionTier.BYOK:
        return 999_999_999  # Effectively unlimited for BYOK
    return settings.free_tier_monthly_tokens


async def maybe_reset_monthly_usage(db: AsyncSession, user: WebUser) -> None:
    """Lazy monthly usage reset — resets counters if usage_reset_at is in a previous month."""
    now = datetime.now(timezone.utc)
    if user.usage_reset_at is None or (
        user.usage_reset_at.year != now.year or user.usage_reset_at.month != now.month
    ):
        await db.execute(
            update(WebUser)
            .where(WebUser.id == user.id)
            .values(monthly_tokens_used=0, monthly_cost_usd=0.0, usage_reset_at=now)
        )
        await db.commit()
        # Refresh in-memory object so the limit check sees reset values
        await db.refresh(user)


async def get_session_cost(db: AsyncSession, user_id: int, project_id: int) -> float:
    """Get the total marked-up cost for a project in the last 24 hours.

    Used to enforce the per-session budget ceiling (max_cost_per_session).
    """
    cutoff = datetime.now(timezone.utc) - timedelta(hours=24)
    result = await db.execute(
        select(func.coalesce(func.sum(UsageRecord.marked_up_cost_usd), 0.0))
        .where(
            UsageRecord.user_id == user_id,
            UsageRecord.project_id == project_id,
            UsageRecord.created_at >= cutoff,
        )
    )
    return float(result.scalar())


async def check_billing_limits(db: AsyncSession, user: WebUser, project_id: int) -> None:
    """Check monthly token limit and per-session budget.

    Raises BillingLimitExceeded if the user is over either limit.
    Call maybe_reset_monthly_usage() before this.
    """
    monthly_limit = get_monthly_limit(user.tier)
    if user.monthly_tokens_used >= monthly_limit:
        raise BillingLimitExceeded(
            f"Monthly token limit ({monthly_limit:,}) reached. Upgrade your plan."
        )

    session_cost = await get_session_cost(db, user.id, project_id)
    if session_cost >= settings.max_cost_per_session:
        raise BillingLimitExceeded(
            f"Project session budget (${settings.max_cost_per_session:.2f}/day) exceeded. "
            f"Current: ${session_cost:.2f}. Try again tomorrow or increase budget."
        )


def infer_provider_from_model(model: str) -> str | None:
    """Infer the LLM provider from a model name string."""
    if "claude" in model:
        return "anthropic"
    elif "gpt" in model or model.startswith("o"):
        return "openai"
    elif "gemini" in model:
        return "google"
    return None


async def get_byok_key(
    user: WebUser, provider: str, db: AsyncSession | None = None
) -> str | None:
    """Look up and optionally auto-encrypt a BYOK API key for the given provider.

    Returns the plaintext API key, or None if not configured.
    When a DB session is provided, auto-encrypts legacy plaintext keys on first read.
    """
    from app.services.key_encryption import decrypt_key, encrypt_key, is_encrypted

    stored = None
    attr_name = None
    if provider == "anthropic":
        stored = user.byok_anthropic_key
        attr_name = "byok_anthropic_key"
    elif provider == "openai":
        stored = user.byok_openai_key
        attr_name = "byok_openai_key"
    elif provider == "google":
        stored = user.byok_google_key
        attr_name = "byok_google_key"

    if not stored:
        return None

    plaintext = decrypt_key(stored)

    # Auto-encrypt legacy plaintext keys on first read
    if db is not None and attr_name and not is_encrypted(stored):
        encrypted = encrypt_key(plaintext)
        if encrypted != stored:
            setattr(user, attr_name, encrypted)
            await db.commit()
            logger.info("Auto-encrypted legacy %s BYOK key for user %d", provider, user.id)

    return plaintext


async def record_usage(
    db: AsyncSession,
    user_id: int,
    project_id: int,
    proxy_result,
) -> int:
    """Create a UsageRecord and atomically update user counters.

    Returns total tokens consumed.
    """
    total_tokens = proxy_result.input_tokens + proxy_result.output_tokens
    record = UsageRecord(
        user_id=user_id,
        project_id=project_id,
        model=proxy_result.model,
        provider=proxy_result.provider,
        input_tokens=proxy_result.input_tokens,
        output_tokens=proxy_result.output_tokens,
        raw_cost_usd=proxy_result.raw_cost_usd,
        marked_up_cost_usd=proxy_result.marked_up_cost_usd,
    )
    db.add(record)

    # Atomic update of user's running totals (prevents TOCTOU race with concurrent requests)
    await db.execute(
        update(WebUser)
        .where(WebUser.id == user_id)
        .values(
            monthly_tokens_used=WebUser.monthly_tokens_used + total_tokens,
            monthly_cost_usd=WebUser.monthly_cost_usd + proxy_result.marked_up_cost_usd,
        )
    )
    await db.commit()

    return total_tokens
