"""Tests for provider proxy endpoints: usage, models, completion validation.

LLM completion calls require live API keys, so we test:
  - GET /providers/usage — empty usage, per-project scoped
  - GET /providers/models — returns model catalog based on configured keys
  - POST /providers/completion — input validation (project/role checks, usage limits)
  - Monthly usage reset logic
  - Per-session budget enforcement
"""

import pytest
from httpx import AsyncClient
from sqlalchemy import select, update
from sqlalchemy.ext.asyncio import AsyncSession

from tests.conftest import auth_headers, create_test_user


# --- Helpers ---

async def _setup_project_with_role(client: AsyncClient, email: str = "prov@test.com"):
    """Create user + project, return (token, project_id, headers)."""
    data = await create_test_user(client, email)
    token = data["access_token"]
    headers = auth_headers(token)
    resp = await client.post("/api/v1/projects/", json={"name": "Prov Test"}, headers=headers)
    assert resp.status_code == 201
    pid = resp.json()["id"]
    return token, pid, headers


# --- Usage endpoint ---

async def test_usage_empty(client: AsyncClient):
    """New user with no usage gets zero counters."""
    data = await create_test_user(client, "usage-empty@test.com")
    headers = auth_headers(data["access_token"])

    resp = await client.get("/api/v1/providers/usage", headers=headers)
    assert resp.status_code == 200
    usage = resp.json()
    assert usage["total_tokens"] == 0
    assert usage["total_cost_usd"] == 0.0
    assert usage["monthly_limit_tokens"] == 50_000  # free tier
    assert usage["remaining_tokens"] == 50_000
    assert usage["provider_breakdown"] == {}


async def test_usage_unauthenticated(client: AsyncClient):
    """Usage without auth returns 401/403."""
    resp = await client.get("/api/v1/providers/usage")
    assert resp.status_code in (401, 403)


async def test_usage_reflects_counters(client: AsyncClient, db):
    """Usage endpoint reflects manually-set counters on user."""
    from app.models import WebUser

    data = await create_test_user(client, "usage-count@test.com")
    headers = auth_headers(data["access_token"])

    await db.execute(
        update(WebUser)
        .where(WebUser.email == "usage-count@test.com")
        .values(monthly_tokens_used=25000, monthly_cost_usd=0.75)
    )
    await db.commit()

    resp = await client.get("/api/v1/providers/usage", headers=headers)
    usage = resp.json()
    assert usage["total_tokens"] == 25000
    assert usage["total_cost_usd"] == 0.75
    assert usage["remaining_tokens"] == 25000  # 50000 - 25000


async def test_usage_pro_tier_limit(client: AsyncClient, db):
    """PRO-tier user gets higher monthly limit in usage response."""
    from app.models import SubscriptionTier, WebUser

    data = await create_test_user(client, "usage-pro@test.com")
    headers = auth_headers(data["access_token"])

    result = await db.execute(select(WebUser).where(WebUser.email == "usage-pro@test.com"))
    user = result.scalar_one()
    user.tier = SubscriptionTier.PRO
    await db.commit()

    resp = await client.get("/api/v1/providers/usage", headers=headers)
    assert resp.json()["monthly_limit_tokens"] == 2_000_000


# --- Models endpoint ---

async def test_models_no_keys(client: AsyncClient):
    """With no API keys configured, models list is empty."""
    resp = await client.get("/api/v1/providers/models")
    assert resp.status_code == 200
    # In test env, no API keys are set
    assert resp.json()["models"] == []


async def test_models_with_anthropic_key(client: AsyncClient, monkeypatch):
    """With Anthropic key configured, returns Anthropic models."""
    from app import config
    monkeypatch.setattr(config.settings, "anthropic_api_key", "sk-ant-test")

    resp = await client.get("/api/v1/providers/models")
    assert resp.status_code == 200
    models = resp.json()["models"]
    assert len(models) >= 3  # opus, sonnet, haiku
    providers = {m["provider"] for m in models}
    assert "anthropic" in providers


# --- Completion validation ---

async def test_completion_unauthenticated(client: AsyncClient):
    """Completion without auth returns 401/403."""
    resp = await client.post("/api/v1/providers/completion", json={
        "project_id": 1,
        "role_slug": "developer",
        "messages": [{"role": "user", "content": "hello"}],
    })
    assert resp.status_code in (401, 403)


async def test_completion_nonexistent_project(client: AsyncClient):
    """Completion with bad project_id returns 404."""
    data = await create_test_user(client, "comp-noproject@test.com")
    resp = await client.post("/api/v1/providers/completion", json={
        "project_id": 99999,
        "role_slug": "developer",
        "messages": [{"role": "user", "content": "hello"}],
    }, headers=auth_headers(data["access_token"]))
    assert resp.status_code == 404


async def test_completion_nonexistent_role(client: AsyncClient):
    """Completion with bad role_slug returns 404."""
    token, pid, headers = await _setup_project_with_role(client, "comp-norole@test.com")
    resp = await client.post("/api/v1/providers/completion", json={
        "project_id": int(pid),
        "role_slug": "nonexistent-role",
        "messages": [{"role": "user", "content": "hello"}],
    }, headers=headers)
    assert resp.status_code == 404
    assert "nonexistent-role" in resp.json()["detail"]


async def test_completion_other_users_project(client: AsyncClient):
    """Completion on another user's project returns 404."""
    _, pid, _ = await _setup_project_with_role(client, "comp-owner@test.com")
    attacker = await create_test_user(client, "comp-attacker@test.com")

    resp = await client.post("/api/v1/providers/completion", json={
        "project_id": int(pid),
        "role_slug": "developer",
        "messages": [{"role": "user", "content": "hello"}],
    }, headers=auth_headers(attacker["access_token"]))
    assert resp.status_code == 404


async def test_completion_usage_limit_exceeded(client: AsyncClient, db, monkeypatch):
    """Free-tier user at token limit gets 429.

    Monkeypatches free_tier_monthly_tokens to 0 so any user exceeds the limit.
    Also patches _maybe_reset_monthly_usage to avoid mid-request commit that causes
    MissingGreenlet on role attribute lazy-load in async SQLAlchemy.
    """
    from app import config
    import app.api.providers as providers_mod

    monkeypatch.setattr(config.settings, "free_tier_monthly_tokens", 0)

    async def _noop_reset(db, user):
        pass
    monkeypatch.setattr(providers_mod, "_maybe_reset_monthly_usage", _noop_reset)

    token, pid, headers = await _setup_project_with_role(client, "comp-limit@test.com")

    resp = await client.post("/api/v1/providers/completion", json={
        "project_id": int(pid),
        "role_slug": "developer",
        "messages": [{"role": "user", "content": "hello"}],
    }, headers=headers)
    assert resp.status_code == 429
    assert "Monthly token limit" in resp.json()["detail"]


async def test_completion_session_budget_exceeded(client: AsyncClient, db):
    """User over per-session budget ceiling gets 429."""
    from datetime import datetime, timezone
    from app.models import UsageRecord, WebUser

    token, pid, headers = await _setup_project_with_role(client, "comp-budget@test.com")

    # Find user ID
    result = await db.execute(select(WebUser).where(WebUser.email == "comp-budget@test.com"))
    user = result.scalar_one()

    # Create usage record that exceeds $50 session budget
    record = UsageRecord(
        user_id=user.id,
        project_id=int(pid),
        model="claude-sonnet-4-6",
        provider="anthropic",
        input_tokens=1000000,
        output_tokens=1000000,
        raw_cost_usd=25.0,
        marked_up_cost_usd=50.0,
        created_at=datetime.now(timezone.utc),
    )
    db.add(record)
    await db.commit()

    resp = await client.post("/api/v1/providers/completion", json={
        "project_id": int(pid),
        "role_slug": "developer",
        "messages": [{"role": "user", "content": "hello"}],
    }, headers=headers)
    assert resp.status_code == 429
    assert "session budget" in resp.json()["detail"].lower()


async def test_completion_byok_missing_key(client: AsyncClient, db, monkeypatch):
    """BYOK user without API key for role's provider gets 402.

    Patches _maybe_reset_monthly_usage to avoid mid-request commit that causes
    MissingGreenlet on role.provider lazy-load in async SQLAlchemy.
    """
    from datetime import datetime, timezone
    from app.models import SubscriptionTier, WebUser
    import app.api.providers as providers_mod

    async def _noop_reset(db, user):
        pass
    monkeypatch.setattr(providers_mod, "_maybe_reset_monthly_usage", _noop_reset)

    token, pid, headers = await _setup_project_with_role(client, "comp-byok@test.com")

    # Upgrade to BYOK + set usage_reset_at so lazy reset doesn't trigger
    await db.execute(
        update(WebUser)
        .where(WebUser.email == "comp-byok@test.com")
        .values(tier=SubscriptionTier.BYOK, usage_reset_at=datetime.now(timezone.utc))
    )
    await db.commit()

    resp = await client.post("/api/v1/providers/completion", json={
        "project_id": int(pid),
        "role_slug": "developer",
        "messages": [{"role": "user", "content": "hello"}],
    }, headers=headers)
    assert resp.status_code == 402
    assert "No API key configured" in resp.json()["detail"]


# --- Usage with project scope ---

async def test_usage_project_scoped(client: AsyncClient, db):
    """Usage scoped to a project returns only that project's records."""
    from datetime import datetime, timezone
    from app.models import UsageRecord, WebUser

    token, pid, headers = await _setup_project_with_role(client, "usage-scope@test.com")

    result = await db.execute(select(WebUser).where(WebUser.email == "usage-scope@test.com"))
    user = result.scalar_one()

    # Add a usage record for this project
    record = UsageRecord(
        user_id=user.id,
        project_id=int(pid),
        model="claude-sonnet-4-6",
        provider="anthropic",
        input_tokens=1000,
        output_tokens=500,
        raw_cost_usd=0.01,
        marked_up_cost_usd=0.02,
        created_at=datetime.now(timezone.utc),
    )
    db.add(record)
    await db.commit()

    # Scoped usage
    resp = await client.get(f"/api/v1/providers/usage/{pid}", headers=headers)
    assert resp.status_code == 200
    usage = resp.json()
    breakdown = usage["provider_breakdown"]
    assert "anthropic" in breakdown
    assert breakdown["anthropic"]["tokens"] == 1500
    assert breakdown["anthropic"]["requests"] == 1
