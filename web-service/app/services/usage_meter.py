"""Usage metering — tracks token consumption and enforces plan limits.

Every LLM API call is metered here. This is the billing data source.
"""

import logging
from dataclasses import dataclass
from datetime import datetime, timezone

logger = logging.getLogger(__name__)


@dataclass
class UsageRecord:
    """A single usage record for billing."""

    user_id: int
    project_id: str
    role_slug: str
    provider: str
    model: str
    input_tokens: int
    output_tokens: int
    raw_cost_usd: float
    marked_up_cost_usd: float
    timestamp: datetime


@dataclass
class UserUsageSummary:
    """Aggregated usage for a user in the current billing period."""

    total_input_tokens: int = 0
    total_output_tokens: int = 0
    total_cost_usd: float = 0.0
    monthly_limit_tokens: int = 0
    period_start: datetime | None = None
    period_end: datetime | None = None


async def record_usage(
    user_id: int,
    project_id: str,
    role_slug: str,
    provider: str,
    model: str,
    input_tokens: int,
    output_tokens: int,
    raw_cost_usd: float,
    marked_up_cost_usd: float,
) -> UsageRecord:
    """Record a usage event. Called after every successful LLM completion."""
    record = UsageRecord(
        user_id=user_id,
        project_id=project_id,
        role_slug=role_slug,
        provider=provider,
        model=model,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        raw_cost_usd=raw_cost_usd,
        marked_up_cost_usd=marked_up_cost_usd,
        timestamp=datetime.now(timezone.utc),
    )
    # TODO: insert into database
    logger.info(
        "Usage recorded: user=%d project=%s role=%s tokens=%d cost=$%.4f",
        user_id, project_id, role_slug, input_tokens + output_tokens, marked_up_cost_usd,
    )
    return record


async def get_user_usage(user_id: int) -> UserUsageSummary:
    """Get aggregated usage for the current billing period."""
    # TODO: query DB for current month's usage
    return UserUsageSummary()


async def check_limits(user_id: int, estimated_tokens: int) -> bool:
    """Check if a user has enough quota for an estimated request.

    Returns True if the request can proceed, False if it would exceed limits.
    """
    usage = await get_user_usage(user_id)
    total_used = usage.total_input_tokens + usage.total_output_tokens
    remaining = usage.monthly_limit_tokens - total_used

    if estimated_tokens > remaining:
        logger.warning(
            "User %d over limit: used=%d limit=%d estimated=%d",
            user_id, total_used, usage.monthly_limit_tokens, estimated_tokens,
        )
        return False
    return True
