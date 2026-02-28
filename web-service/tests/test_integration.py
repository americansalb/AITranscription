"""End-to-end integration tests: auth, projects, messages, discussions, billing, usage, roles."""

import pytest
from httpx import AsyncClient
from sqlalchemy import select

from tests.conftest import auth_headers, create_test_user


@pytest.mark.asyncio
async def test_health_endpoint(client: AsyncClient):
    """GET /health returns 200 with status and version."""
    resp = await client.get("/health")
    assert resp.status_code == 200
    data = resp.json()
    assert data["status"] == "ok"
    assert "version" in data


@pytest.mark.asyncio
async def test_full_flow_signup_to_discussion(client: AsyncClient):
    """Complete user journey: signup → create project → send messages → run discussion."""

    # 1. Sign up
    signup = await create_test_user(client, "e2e@example.com", "securepass123")
    token = signup["access_token"]
    headers = auth_headers(token)

    # 2. Verify identity
    me = await client.get("/api/v1/auth/me", headers=headers)
    assert me.status_code == 200
    assert me.json()["email"] == "e2e@example.com"
    assert me.json()["tier"] == "free"

    # 3. Create project
    resp = await client.post("/api/v1/projects/", json={"name": "E2E Project"}, headers=headers)
    assert resp.status_code == 201
    project = resp.json()
    pid = project["id"]
    assert len(project["roles"]) == 4  # manager, architect, developer, tester

    # 4. Assign developer to OpenAI
    resp = await client.put(
        f"/api/v1/projects/{pid}/roles/developer/provider",
        json={"provider": "openai", "model": "gpt-4o"},
        headers=headers,
    )
    assert resp.status_code == 200

    # 5. Send messages to the board
    resp = await client.post(f"/api/v1/messages/{pid}", json={
        "to": "all",
        "type": "broadcast",
        "subject": "Project kickoff",
        "body": "Welcome to the project everyone!",
    }, headers=headers)
    assert resp.status_code == 200
    first_msg_id = resp.json()["id"]

    resp = await client.post(f"/api/v1/messages/{pid}", json={
        "to": "developer",
        "type": "directive",
        "subject": "Implement auth",
        "body": "Please implement JWT authentication.",
    }, headers=headers)
    assert resp.status_code == 200

    # 6. Read messages back
    resp = await client.get(f"/api/v1/messages/{pid}", headers=headers)
    assert resp.status_code == 200
    msgs = resp.json()
    assert msgs["total"] >= 2

    # 7. Read with since_id
    resp = await client.get(f"/api/v1/messages/{pid}?since_id={first_msg_id}", headers=headers)
    assert resp.json()["total"] >= 1

    # 8. Start a Delphi discussion
    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi",
        "topic": "Should we add WebSocket support?",
        "participants": ["architect:0", "developer:0"],
        "max_rounds": 3,
    }, headers=headers)
    assert resp.status_code == 201
    disc = resp.json()
    disc_id = disc["id"]
    assert disc["phase"] == "preparing"

    # 9. Open round 1
    resp = await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/open-round", headers=headers)
    assert resp.status_code == 200

    # 10. Submit responses
    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/submit?role_slug=architect",
        json={"body": "Yes, WebSockets are essential for real-time updates."},
        headers=headers,
    )
    assert resp.status_code == 200

    # 11. Close round and get aggregate
    resp = await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/close-round", headers=headers)
    assert resp.status_code == 200
    agg = resp.json()
    assert agg["aggregate"] is not None
    assert agg["phase"] == "reviewing"

    # 12. Check active discussion reflects the state
    resp = await client.get(f"/api/v1/projects/{pid}/discussions/active", headers=headers)
    assert resp.status_code == 200
    active = resp.json()
    assert active["current_round"] == 1
    assert len(active["rounds"]) == 1

    # 13. End the discussion
    resp = await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/end", headers=headers)
    assert resp.status_code == 200

    # 14. No more active discussion
    resp = await client.get(f"/api/v1/projects/{pid}/discussions/active", headers=headers)
    assert resp.status_code == 200
    assert resp.json() is None

    # 15. Can start a new discussion after ending the first
    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "continuous",
        "topic": "Code quality review",
        "auto_close_timeout_seconds": 60,
    }, headers=headers)
    assert resp.status_code == 201
    assert resp.json()["mode"] == "continuous"


@pytest.mark.asyncio
async def test_project_isolation(client: AsyncClient):
    """Users can't access each other's projects or messages."""
    user1 = await create_test_user(client, "iso1@test.com")
    user2 = await create_test_user(client, "iso2@test.com")

    h1 = auth_headers(user1["access_token"])
    h2 = auth_headers(user2["access_token"])

    # User 1 creates project
    resp = await client.post("/api/v1/projects/", json={"name": "User1 Project"}, headers=h1)
    pid1 = resp.json()["id"]

    # User 2 creates project
    resp = await client.post("/api/v1/projects/", json={"name": "User2 Project"}, headers=h2)
    pid2 = resp.json()["id"]

    # User 1 can't see user 2's project
    assert (await client.get(f"/api/v1/projects/{pid2}", headers=h1)).status_code == 404

    # User 2 can't send messages to user 1's project
    resp = await client.post(f"/api/v1/messages/{pid1}", json={"to": "all", "body": "Intruder!"}, headers=h2)
    assert resp.status_code == 404

    # User 2 can't start discussions in user 1's project
    resp = await client.post(f"/api/v1/projects/{pid1}/discussions", json={
        "mode": "delphi", "topic": "Hacking",
    }, headers=h2)
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_billing_status_free_tier(client: AsyncClient):
    """Free user gets correct billing status with zero usage."""
    data = await create_test_user(client, "billing@test.com")
    headers = auth_headers(data["access_token"])

    resp = await client.get("/api/v1/billing/status", headers=headers)
    assert resp.status_code == 200
    status = resp.json()
    assert status["plan"] == "free"
    assert status["usage"]["tokens_used"] == 0
    assert status["usage"]["tokens_limit"] > 0
    assert status["usage"]["cost_usd"] == 0.0


@pytest.mark.asyncio
async def test_billing_checkout_no_stripe(client: AsyncClient):
    """Checkout returns 503 when Stripe is not configured."""
    data = await create_test_user(client, "checkout@test.com")
    headers = auth_headers(data["access_token"])

    resp = await client.post("/api/v1/billing/checkout", json={"plan": "pro"}, headers=headers)
    assert resp.status_code == 503


@pytest.mark.asyncio
async def test_billing_checkout_invalid_plan(client: AsyncClient, monkeypatch):
    """Checkout rejects invalid plan names."""
    data = await create_test_user(client, "badplan@test.com")
    headers = auth_headers(data["access_token"])

    # Monkeypatch stripe key so we get past the "not configured" check
    from app.config import settings
    monkeypatch.setattr(settings, "stripe_secret_key", "sk_test_fake")

    resp = await client.post("/api/v1/billing/checkout", json={"plan": "platinum"}, headers=headers)
    assert resp.status_code == 400
    assert "Invalid plan" in resp.json()["detail"]


@pytest.mark.asyncio
async def test_billing_portal_no_subscription(client: AsyncClient):
    """Portal requires an active subscription."""
    data = await create_test_user(client, "portal@test.com")
    headers = auth_headers(data["access_token"])

    resp = await client.post("/api/v1/billing/portal", headers=headers)
    assert resp.status_code == 400


@pytest.mark.asyncio
async def test_usage_endpoint(client: AsyncClient):
    """Usage endpoint returns correct structure for free user."""
    data = await create_test_user(client, "usage@test.com")
    headers = auth_headers(data["access_token"])

    # Create a project first (usage can be project-scoped)
    proj = await client.post("/api/v1/projects/", json={"name": "Usage Test"}, headers=headers)
    pid = proj.json()["id"]

    # Global usage
    resp = await client.get("/api/v1/providers/usage", headers=headers)
    assert resp.status_code == 200
    usage = resp.json()
    assert usage["total_tokens"] == 0
    assert usage["remaining_tokens"] > 0
    assert usage["provider_breakdown"] == {}

    # Project-scoped usage
    resp = await client.get(f"/api/v1/providers/usage/{pid}", headers=headers)
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_models_endpoint(client: AsyncClient):
    """Models endpoint returns available models."""
    data = await create_test_user(client, "models@test.com")
    headers = auth_headers(data["access_token"])

    resp = await client.get("/api/v1/providers/models", headers=headers)
    assert resp.status_code == 200
    result = resp.json()
    assert "models" in result
    assert isinstance(result["models"], list)


@pytest.mark.asyncio
async def test_role_lifecycle(client: AsyncClient):
    """Full role lifecycle: create → get briefing → update briefing → delete."""
    data = await create_test_user(client, "rolelife@test.com")
    headers = auth_headers(data["access_token"])

    # Create project
    resp = await client.post("/api/v1/projects/", json={"name": "Role Test"}, headers=headers)
    pid = resp.json()["id"]

    # Create custom role
    resp = await client.post(f"/api/v1/projects/{pid}/roles", json={
        "slug": "security-auditor",
        "title": "Security Auditor",
        "description": "Reviews code for vulnerabilities",
        "tags": ["security", "review"],
        "permissions": ["read_code"],
        "maxInstances": 2,
    }, headers=headers)
    assert resp.status_code == 201
    role = resp.json()
    assert role["slug"] == "security-auditor"
    assert role["maxInstances"] == 2

    # Get briefing (should be the description)
    resp = await client.get(f"/api/v1/projects/{pid}/roles/security-auditor/briefing", headers=headers)
    assert resp.status_code == 200
    assert "vulnerabilities" in resp.json()["briefing"]

    # Update briefing
    new_briefing = "You are the Security Auditor. Focus on OWASP Top 10."
    resp = await client.put(
        f"/api/v1/projects/{pid}/roles/security-auditor/briefing",
        json={"briefing": new_briefing},
        headers=headers,
    )
    assert resp.status_code == 200

    # Verify briefing updated
    resp = await client.get(f"/api/v1/projects/{pid}/roles/security-auditor/briefing", headers=headers)
    assert resp.json()["briefing"] == new_briefing

    # Verify role appears in project
    resp = await client.get(f"/api/v1/projects/{pid}", headers=headers)
    assert "security-auditor" in resp.json()["roles"]

    # Delete role
    resp = await client.delete(f"/api/v1/projects/{pid}/roles/security-auditor", headers=headers)
    assert resp.status_code == 204

    # Verify deleted
    resp = await client.get(f"/api/v1/projects/{pid}", headers=headers)
    assert "security-auditor" not in resp.json()["roles"]


@pytest.mark.asyncio
async def test_byok_upgrade_and_api_keys(client: AsyncClient, db):
    """BYOK user can set API keys after upgrade."""
    from app.models import SubscriptionTier, WebUser

    data = await create_test_user(client, "byok-e2e@test.com")
    headers = auth_headers(data["access_token"])

    # Free user can't set keys
    resp = await client.put("/api/v1/auth/api-keys", json={"anthropic": "sk-test"}, headers=headers)
    assert resp.status_code == 403

    # Simulate Stripe upgrade to BYOK
    result = await db.execute(select(WebUser).where(WebUser.email == "byok-e2e@test.com"))
    user = result.scalar_one()
    user.tier = SubscriptionTier.BYOK
    await db.commit()

    # Now can set keys
    resp = await client.put("/api/v1/auth/api-keys", json={
        "anthropic": "sk-ant-key",
        "openai": "sk-openai-key",
    }, headers=headers)
    assert resp.status_code == 200
    assert resp.json()["tier"] == "byok"

    # Verify billing status reflects BYOK
    resp = await client.get("/api/v1/billing/status", headers=headers)
    assert resp.json()["plan"] == "byok"
    assert resp.json()["usage"]["tokens_limit"] == 999_999_999


@pytest.mark.asyncio
async def test_agent_actions(client: AsyncClient):
    """Buzz and interrupt agent endpoints work."""
    data = await create_test_user(client, "agent-e2e@test.com")
    headers = auth_headers(data["access_token"])

    resp = await client.post("/api/v1/projects/", json={"name": "Agent Test"}, headers=headers)
    pid = resp.json()["id"]

    # Buzz developer
    resp = await client.post(
        f"/api/v1/projects/{pid}/roles/developer/buzz",
        json={"instance": 0},
        headers=headers,
    )
    assert resp.status_code == 200
    assert resp.json()["status"] == "buzzed"

    # Interrupt architect
    resp = await client.post(
        f"/api/v1/projects/{pid}/roles/architect/interrupt",
        json={"reason": "Emergency: production is down", "instance": 0},
        headers=headers,
    )
    assert resp.status_code == 200
    assert resp.json()["status"] == "interrupted"

    # Verify buzz and interrupt appear as messages
    resp = await client.get(f"/api/v1/messages/{pid}", headers=headers)
    msgs = resp.json()["messages"]
    types = [m["type"] for m in msgs]
    assert "buzz" in types
    assert "interrupt" in types


@pytest.mark.asyncio
async def test_password_change_invalidates_old_password(client: AsyncClient):
    """End-to-end: signup → change password → old password fails → new works."""
    data = await create_test_user(client, "pwchange@test.com", "original123")
    headers = auth_headers(data["access_token"])

    # Change password
    resp = await client.post("/api/v1/auth/change-password", json={
        "current_password": "original123",
        "new_password": "updated456",
    }, headers=headers)
    assert resp.status_code == 200

    # Old password fails
    resp = await client.post("/api/v1/auth/login", json={
        "email": "pwchange@test.com",
        "password": "original123",
    })
    assert resp.status_code == 401

    # New password works
    resp = await client.post("/api/v1/auth/login", json={
        "email": "pwchange@test.com",
        "password": "updated456",
    })
    assert resp.status_code == 200
    assert "access_token" in resp.json()


@pytest.mark.asyncio
async def test_agent_lifecycle_and_billing_validation(client: AsyncClient, db, monkeypatch):
    """End-to-end: signup → project → assign provider → start agent → list → stop → billing checks.

    The agent loop is mocked to a no-op (it uses async_session directly, not the test DB).
    This tests the REST API lifecycle and that billing validation catches over-limit users.
    """
    import asyncio
    import app.services.agent_runtime as runtime_mod

    # Mock the agent loop so it doesn't actually run (avoids needing real LLM + real DB)
    async def _noop_loop(state, briefing, user_id):
        while state.is_running:
            await asyncio.sleep(0.1)

    monkeypatch.setattr(runtime_mod, "_agent_loop", _noop_loop)

    # 1. Sign up
    data = await create_test_user(client, "agent-lifecycle@test.com")
    headers = auth_headers(data["access_token"])

    # 2. Create project
    resp = await client.post("/api/v1/projects/", json={"name": "Agent Lifecycle"}, headers=headers)
    assert resp.status_code == 201
    pid = resp.json()["id"]
    assert len(resp.json()["roles"]) == 4  # default roles

    # 3. Assign provider to developer
    resp = await client.put(
        f"/api/v1/projects/{pid}/roles/developer/provider",
        json={"provider": "anthropic", "model": "claude-sonnet-4-6"},
        headers=headers,
    )
    assert resp.status_code == 200
    assert resp.json()["model"] == "claude-sonnet-4-6"

    # 4. Start agent
    resp = await client.post(f"/api/v1/projects/{pid}/roles/developer/start", headers=headers)
    assert resp.status_code == 200
    assert resp.json()["status"] == "started"

    # 5. Starting again should fail (409)
    resp = await client.post(f"/api/v1/projects/{pid}/roles/developer/start", headers=headers)
    assert resp.status_code == 409

    # 6. List agents
    resp = await client.get(f"/api/v1/projects/{pid}/agents", headers=headers)
    assert resp.status_code == 200
    agents = resp.json()
    assert len(agents) == 1
    assert agents[0]["role"] == "developer"
    assert agents[0]["is_running"] is True

    # 7. Stop agent
    resp = await client.post(f"/api/v1/projects/{pid}/roles/developer/stop", headers=headers)
    assert resp.status_code == 200
    assert resp.json()["status"] == "stopped"

    # 8. Stopping again should fail (409)
    resp = await client.post(f"/api/v1/projects/{pid}/roles/developer/stop", headers=headers)
    assert resp.status_code == 409

    # 9. No agents running
    resp = await client.get(f"/api/v1/projects/{pid}/agents", headers=headers)
    assert resp.json() == []

    # 10. Billing validation: free-tier user at limit gets 429 on completion
    # Monkeypatch _maybe_reset_monthly_usage to no-op (avoids MissingGreenlet
    # from mid-request commit in async SQLAlchemy with test DB)
    import app.api.providers as providers_mod
    async def _noop_reset(db, user):
        pass
    monkeypatch.setattr(providers_mod, "_maybe_reset_monthly_usage", _noop_reset)

    from app.models import WebUser
    from sqlalchemy import update as sql_update
    await db.execute(
        sql_update(WebUser).where(WebUser.email == "agent-lifecycle@test.com")
        .values(monthly_tokens_used=50_000)
    )
    await db.commit()

    resp = await client.post("/api/v1/providers/completion", json={
        "project_id": pid,
        "role_slug": "developer",
        "messages": [{"role": "user", "content": "test"}],
    }, headers=headers)
    assert resp.status_code == 429
    assert "Monthly token limit" in resp.json()["detail"]

    # 11. Usage endpoint reflects the tokens
    resp = await client.get("/api/v1/providers/usage", headers=headers)
    assert resp.status_code == 200
    assert resp.json()["total_tokens"] == 50_000
    assert resp.json()["remaining_tokens"] == 0
