"""LLM provider proxy — routes requests through LiteLLM with metering and billing."""

import logging

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.config import settings
from app.database import get_db
from app.models import Project, ProjectRole, SubscriptionTier, UsageRecord, WebUser
from app.services.provider_proxy import proxy_completion

logger = logging.getLogger(__name__)
router = APIRouter()


# --- Schemas ---

class CompletionRequest(BaseModel):
    project_id: int
    role_slug: str
    messages: list[dict] = Field(description="Chat messages in OpenAI format")
    tools: list[dict] | None = None
    system: str | None = None
    stream: bool = False


class CompletionResponse(BaseModel):
    content: str
    tool_calls: list[dict] = []
    usage: dict
    cost_usd: float
    provider: str
    model: str


class UsageSummary(BaseModel):
    total_tokens: int
    total_cost_usd: float
    monthly_limit_tokens: int
    remaining_tokens: int
    provider_breakdown: dict[str, dict] = {}


# --- Endpoints ---

@router.post("/completion", response_model=CompletionResponse)
async def route_completion(
    request: CompletionRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Route an LLM completion through the metered proxy.

    1. Look up the role's assigned provider and model
    2. Check user's usage against plan limits
    3. Route to LiteLLM
    4. Meter tokens and record cost
    5. Return response
    """
    # 1. Verify project ownership and get role config
    result = await db.execute(
        select(Project).where(Project.id == request.project_id, Project.owner_id == user.id)
    )
    project = result.scalar_one_or_none()
    if not project:
        raise HTTPException(status_code=404, detail="Project not found")

    result = await db.execute(
        select(ProjectRole).where(
            ProjectRole.project_id == project.id,
            ProjectRole.slug == request.role_slug,
        )
    )
    role = result.scalar_one_or_none()
    if not role:
        raise HTTPException(status_code=404, detail=f"Role '{request.role_slug}' not found")

    # 2. Check usage limits
    monthly_limit = _get_monthly_limit(user.tier)
    if user.monthly_tokens_used >= monthly_limit:
        raise HTTPException(
            status_code=429,
            detail=f"Monthly token limit ({monthly_limit:,}) reached. Upgrade your plan.",
        )

    # 3. Determine API key (BYOK vs platform)
    byok_key = None
    if user.tier == SubscriptionTier.BYOK:
        byok_key = _get_byok_key(user, role.provider)

    # 4. Call proxy
    try:
        proxy_result = await proxy_completion(
            user_id=user.id,
            model=role.model,
            messages=request.messages,
            tools=request.tools,
            system=request.system or role.briefing,
            stream=request.stream,
            byok_key=byok_key,
        )
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except RuntimeError as e:
        raise HTTPException(status_code=503, detail=str(e))

    # 5. Record usage
    total_tokens = proxy_result.input_tokens + proxy_result.output_tokens
    record = UsageRecord(
        user_id=user.id,
        project_id=project.id,
        model=proxy_result.model,
        provider=proxy_result.provider,
        input_tokens=proxy_result.input_tokens,
        output_tokens=proxy_result.output_tokens,
        raw_cost_usd=proxy_result.raw_cost_usd,
        marked_up_cost_usd=proxy_result.marked_up_cost_usd,
    )
    db.add(record)

    # Update user's running totals
    user.monthly_tokens_used += total_tokens
    user.monthly_cost_usd += proxy_result.marked_up_cost_usd
    await db.commit()

    return CompletionResponse(
        content=proxy_result.content,
        tool_calls=proxy_result.tool_calls,
        usage={
            "input_tokens": proxy_result.input_tokens,
            "output_tokens": proxy_result.output_tokens,
            "total_tokens": total_tokens,
        },
        cost_usd=proxy_result.marked_up_cost_usd,
        provider=proxy_result.provider,
        model=proxy_result.model,
    )


@router.get("/usage", response_model=UsageSummary)
@router.get("/usage/{project_id}", response_model=UsageSummary)
async def get_usage(
    project_id: int | None = None,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Get current user's usage summary, optionally scoped to a project."""
    monthly_limit = _get_monthly_limit(user.tier)

    # Provider breakdown from usage records
    usage_query = (
        select(
            UsageRecord.provider,
            func.sum(UsageRecord.input_tokens + UsageRecord.output_tokens).label("tokens"),
            func.sum(UsageRecord.marked_up_cost_usd).label("cost"),
            func.count(UsageRecord.id).label("requests"),
        )
        .where(UsageRecord.user_id == user.id)
        .group_by(UsageRecord.provider)
    )
    if project_id is not None:
        usage_query = usage_query.where(UsageRecord.project_id == project_id)

    result = await db.execute(usage_query)
    breakdown = {}
    for row in result.all():
        breakdown[row.provider] = {
            "tokens": int(row.tokens or 0),
            "cost_usd": float(row.cost or 0),
            "requests": int(row.requests or 0),
        }

    return UsageSummary(
        total_tokens=user.monthly_tokens_used,
        total_cost_usd=round(user.monthly_cost_usd, 4),
        monthly_limit_tokens=monthly_limit,
        remaining_tokens=max(0, monthly_limit - user.monthly_tokens_used),
        provider_breakdown=breakdown,
    )


@router.get("/models")
async def list_available_models():
    """List all available models across configured providers."""
    # Per-million-token pricing (input/output) — approximate as of Feb 2026
    MODEL_CATALOG = []
    if settings.anthropic_api_key:
        MODEL_CATALOG.extend([
            {"id": "claude-opus-4-6", "provider": "anthropic", "name": "Claude Opus 4.6", "input_cost": 15.0, "output_cost": 75.0},
            {"id": "claude-sonnet-4-6", "provider": "anthropic", "name": "Claude Sonnet 4.6", "input_cost": 3.0, "output_cost": 15.0},
            {"id": "claude-haiku-4-5-20251001", "provider": "anthropic", "name": "Claude Haiku 4.5", "input_cost": 0.80, "output_cost": 4.0},
        ])
    if settings.openai_api_key:
        MODEL_CATALOG.extend([
            {"id": "gpt-4o", "provider": "openai", "name": "GPT-4o", "input_cost": 2.50, "output_cost": 10.0},
            {"id": "gpt-4o-mini", "provider": "openai", "name": "GPT-4o Mini", "input_cost": 0.15, "output_cost": 0.60},
        ])
    if settings.google_ai_api_key:
        MODEL_CATALOG.extend([
            {"id": "gemini-2.0-flash", "provider": "google", "name": "Gemini 2.0 Flash", "input_cost": 0.10, "output_cost": 0.40},
            {"id": "gemini-2.0-pro", "provider": "google", "name": "Gemini 2.0 Pro", "input_cost": 1.25, "output_cost": 5.0},
        ])
    return {"models": MODEL_CATALOG}


# --- Helpers ---

def _get_monthly_limit(tier: SubscriptionTier) -> int:
    if tier == SubscriptionTier.FREE:
        return settings.free_tier_monthly_tokens
    elif tier == SubscriptionTier.PRO:
        return settings.pro_tier_monthly_tokens
    elif tier == SubscriptionTier.BYOK:
        return 999_999_999  # Effectively unlimited for BYOK
    return settings.free_tier_monthly_tokens


def _get_byok_key(user: WebUser, provider: str) -> str | None:
    if provider == "anthropic":
        return user.byok_anthropic_key
    elif provider == "openai":
        return user.byok_openai_key
    elif provider == "google":
        return user.byok_google_key
    return None
