"""End-to-end integration test: signup → project → messages → discussion."""

import pytest
from httpx import AsyncClient

from tests.conftest import auth_headers, create_test_user


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
