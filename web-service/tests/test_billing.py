"""Tests for billing endpoints: subscription status, checkout, portal, webhook.

Stripe SDK is not available in tests, so we test:
  - GET /billing/status — free user usage info (no Stripe call)
  - POST /billing/checkout — validation (no Stripe key = 503, invalid plan = 400)
  - POST /billing/portal — validation (no customer_id = 400)
  - POST /billing/webhook — validation (no webhook secret = 503, missing sig = 400)
  - Tier-based usage limits reflected in status
"""

import pytest
from httpx import AsyncClient

from tests.conftest import auth_headers, create_test_user


async def test_billing_status_free_tier(client: AsyncClient):
    """Free-tier user gets correct usage info with default limits."""
    data = await create_test_user(client, "billing-free@test.com")
    headers = auth_headers(data["access_token"])

    resp = await client.get("/api/v1/billing/status", headers=headers)
    assert resp.status_code == 200
    status = resp.json()
    assert status["plan"] == "free"
    assert status["usage"]["tokens_used"] == 0
    assert status["usage"]["tokens_limit"] == 50_000  # free_tier_monthly_tokens default
    assert status["usage"]["cost_usd"] == 0.0


async def test_billing_status_unauthenticated(client: AsyncClient):
    """Billing status without auth returns 401/403."""
    resp = await client.get("/api/v1/billing/status")
    assert resp.status_code in (401, 403)


async def test_billing_status_pro_tier(client: AsyncClient, db):
    """PRO-tier user gets higher token limit."""
    from app.models import SubscriptionTier, WebUser
    from sqlalchemy import select

    data = await create_test_user(client, "billing-pro@test.com")
    headers = auth_headers(data["access_token"])

    # Upgrade to PRO
    result = await db.execute(select(WebUser).where(WebUser.email == "billing-pro@test.com"))
    user = result.scalar_one()
    user.tier = SubscriptionTier.PRO
    await db.commit()

    resp = await client.get("/api/v1/billing/status", headers=headers)
    assert resp.status_code == 200
    status = resp.json()
    assert status["plan"] == "pro"
    assert status["active"] is True
    assert status["usage"]["tokens_limit"] == 2_000_000  # pro_tier_monthly_tokens


async def test_billing_status_byok_tier(client: AsyncClient, db):
    """BYOK-tier user gets effectively unlimited tokens."""
    from app.models import SubscriptionTier, WebUser
    from sqlalchemy import select

    data = await create_test_user(client, "billing-byok@test.com")
    headers = auth_headers(data["access_token"])

    result = await db.execute(select(WebUser).where(WebUser.email == "billing-byok@test.com"))
    user = result.scalar_one()
    user.tier = SubscriptionTier.BYOK
    await db.commit()

    resp = await client.get("/api/v1/billing/status", headers=headers)
    assert resp.status_code == 200
    status = resp.json()
    assert status["plan"] == "byok"
    assert status["active"] is True
    assert status["usage"]["tokens_limit"] == 999_999_999


async def test_checkout_no_stripe_key(client: AsyncClient):
    """Checkout without Stripe key configured returns 503."""
    data = await create_test_user(client, "checkout-nokey@test.com")
    resp = await client.post(
        "/api/v1/billing/checkout",
        json={"plan": "pro"},
        headers=auth_headers(data["access_token"]),
    )
    assert resp.status_code == 503
    assert "not configured" in resp.json()["detail"].lower()


async def test_checkout_invalid_plan(client: AsyncClient, monkeypatch):
    """Invalid plan name returns 400."""
    from app import config
    monkeypatch.setattr(config.settings, "stripe_secret_key", "sk_test_fake")

    data = await create_test_user(client, "checkout-bad@test.com")
    resp = await client.post(
        "/api/v1/billing/checkout",
        json={"plan": "enterprise"},
        headers=auth_headers(data["access_token"]),
    )
    assert resp.status_code == 400
    assert "Invalid plan" in resp.json()["detail"]


async def test_checkout_unauthenticated(client: AsyncClient):
    """Checkout without auth returns 401/403."""
    resp = await client.post("/api/v1/billing/checkout", json={"plan": "pro"})
    assert resp.status_code in (401, 403)


async def test_portal_no_customer_id(client: AsyncClient):
    """Portal without existing customer returns 400."""
    data = await create_test_user(client, "portal-noid@test.com")
    resp = await client.post(
        "/api/v1/billing/portal",
        headers=auth_headers(data["access_token"]),
    )
    assert resp.status_code == 400
    assert "No active subscription" in resp.json()["detail"]


async def test_portal_unauthenticated(client: AsyncClient):
    """Portal without auth returns 401/403."""
    resp = await client.post("/api/v1/billing/portal")
    assert resp.status_code in (401, 403)


async def test_webhook_no_secret(client: AsyncClient):
    """Webhook with no webhook secret configured returns 503."""
    resp = await client.post(
        "/api/v1/billing/webhook",
        content=b'{"type": "test"}',
        headers={"stripe-signature": "t=123,v1=abc"},
    )
    assert resp.status_code == 503


async def test_webhook_missing_signature(client: AsyncClient, monkeypatch):
    """Webhook without stripe-signature header returns 400."""
    from app import config
    monkeypatch.setattr(config.settings, "stripe_webhook_secret", "whsec_test")

    resp = await client.post(
        "/api/v1/billing/webhook",
        content=b'{"type": "test"}',
    )
    assert resp.status_code == 400
    assert "Missing signature" in resp.json()["detail"]


async def test_billing_status_reflects_usage(client: AsyncClient, db):
    """Status endpoint reflects manually-set usage counters."""
    from app.models import WebUser
    from sqlalchemy import select, update

    data = await create_test_user(client, "usage-reflect@test.com")
    headers = auth_headers(data["access_token"])

    # Simulate some usage
    await db.execute(
        update(WebUser)
        .where(WebUser.email == "usage-reflect@test.com")
        .values(monthly_tokens_used=12345, monthly_cost_usd=1.23)
    )
    await db.commit()

    resp = await client.get("/api/v1/billing/status", headers=headers)
    status = resp.json()
    assert status["usage"]["tokens_used"] == 12345
    assert status["usage"]["cost_usd"] == 1.23
