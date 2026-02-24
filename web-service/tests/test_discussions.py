"""Tests for the discussion system: Delphi, Oxford, Continuous."""

import pytest
from httpx import AsyncClient

from tests.conftest import auth_headers, create_test_user


async def _setup(client: AsyncClient, email_suffix: str = "") -> tuple[str, int]:
    """Helper: create user + project, return (token, project_id)."""
    email = f"disc{email_suffix}-{id(client)}@test.com"
    data = await create_test_user(client, email)
    token = data["access_token"]
    resp = await client.post("/api/v1/projects/", json={"name": "Disc Test"}, headers=auth_headers(token))
    return token, resp.json()["id"]


@pytest.mark.asyncio
async def test_start_delphi_discussion(client: AsyncClient):
    token, pid = await _setup(client)
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi",
        "topic": "Should we use microservices?",
        "participants": ["architect:0", "developer:0"],
    }, headers=headers)
    assert resp.status_code == 201
    disc = resp.json()
    assert disc["mode"] == "delphi"
    assert disc["topic"] == "Should we use microservices?"
    assert disc["is_active"] is True
    assert disc["phase"] == "preparing"
    assert disc["current_round"] == 0


@pytest.mark.asyncio
async def test_start_continuous_discussion(client: AsyncClient):
    token, pid = await _setup(client, "-cont")
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "continuous",
        "topic": "Code review",
        "auto_close_timeout_seconds": 30,
    }, headers=headers)
    assert resp.status_code == 201
    disc = resp.json()
    assert disc["mode"] == "continuous"
    assert disc["phase"] == "reviewing"


@pytest.mark.asyncio
async def test_no_duplicate_active_discussion(client: AsyncClient):
    token, pid = await _setup(client, "-dup")
    headers = auth_headers(token)

    await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "First",
    }, headers=headers)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "oxford", "topic": "Second",
    }, headers=headers)
    assert resp.status_code == 409


@pytest.mark.asyncio
async def test_get_active_discussion(client: AsyncClient):
    token, pid = await _setup(client, "-active")
    headers = auth_headers(token)

    await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Active test",
    }, headers=headers)

    resp = await client.get(f"/api/v1/projects/{pid}/discussions/active", headers=headers)
    assert resp.status_code == 200
    assert resp.json()["topic"] == "Active test"


@pytest.mark.asyncio
async def test_get_active_discussion_none(client: AsyncClient):
    token, pid = await _setup(client, "-noact")
    headers = auth_headers(token)

    resp = await client.get(f"/api/v1/projects/{pid}/discussions/active", headers=headers)
    assert resp.status_code == 200
    assert resp.json() is None


@pytest.mark.asyncio
async def test_delphi_full_round_cycle(client: AsyncClient):
    """Full Delphi cycle: start → open round → submit → close round → review."""
    token, pid = await _setup(client, "-cycle")
    headers = auth_headers(token)

    # Start
    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Architecture review",
    }, headers=headers)
    disc_id = resp.json()["id"]

    # Open round 1
    resp = await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/open-round", headers=headers)
    assert resp.status_code == 200
    assert resp.json()["round"] == 1
    assert resp.json()["phase"] == "submitting"

    # Submit
    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/submit?role_slug=architect",
        json={"body": "I think we should use a monolith first."},
        headers=headers,
    )
    assert resp.status_code == 200
    assert resp.json()["status"] == "submitted"

    # Close round
    resp = await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/close-round", headers=headers)
    assert resp.status_code == 200
    result = resp.json()
    assert result["phase"] == "reviewing"
    assert result["aggregate"] is not None

    # End
    resp = await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/end", headers=headers)
    assert resp.status_code == 200
    assert resp.json()["status"] == "ended"


@pytest.mark.asyncio
async def test_oxford_teams(client: AsyncClient):
    token, pid = await _setup(client, "-oxford")
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "oxford", "topic": "Monolith vs microservices",
    }, headers=headers)
    disc_id = resp.json()["id"]

    # Set teams
    resp = await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/teams", json={
        "teams": {"for": ["architect:0"], "against": ["developer:0"]},
    }, headers=headers)
    assert resp.status_code == 200
    assert "for" in resp.json()["teams"]


@pytest.mark.asyncio
async def test_submit_duplicate_rejected(client: AsyncClient):
    """Same role can't submit twice to the same round."""
    token, pid = await _setup(client, "-dupsubmit")
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Test",
    }, headers=headers)
    disc_id = resp.json()["id"]

    await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/open-round", headers=headers)

    # First submit
    await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/submit?role_slug=human",
        json={"body": "First attempt"},
        headers=headers,
    )

    # Duplicate
    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/submit?role_slug=human",
        json={"body": "Duplicate"},
        headers=headers,
    )
    assert resp.status_code == 409


@pytest.mark.asyncio
async def test_end_discussion(client: AsyncClient):
    token, pid = await _setup(client, "-end")
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Ending test",
    }, headers=headers)
    disc_id = resp.json()["id"]

    resp = await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/end", headers=headers)
    assert resp.status_code == 200

    # Can't operate on ended discussion
    resp = await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/open-round", headers=headers)
    assert resp.status_code == 409


@pytest.mark.asyncio
async def test_get_discussion_by_id(client: AsyncClient):
    """GET /{discussion_id} returns discussion including after it ends."""
    token, pid = await _setup(client, "-byid")
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Retrievable discussion",
    }, headers=headers)
    disc_id = resp.json()["id"]

    # Get active discussion by ID
    resp = await client.get(f"/api/v1/projects/{pid}/discussions/{disc_id}", headers=headers)
    assert resp.status_code == 200
    assert resp.json()["topic"] == "Retrievable discussion"
    assert resp.json()["is_active"] is True

    # End it
    await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/end", headers=headers)

    # Can still retrieve ended discussion by ID
    resp = await client.get(f"/api/v1/projects/{pid}/discussions/{disc_id}", headers=headers)
    assert resp.status_code == 200
    assert resp.json()["is_active"] is False


@pytest.mark.asyncio
async def test_get_nonexistent_discussion(client: AsyncClient):
    """GET for non-existent discussion returns 404."""
    token, pid = await _setup(client, "-nodisc")
    headers = auth_headers(token)

    resp = await client.get(f"/api/v1/projects/{pid}/discussions/99999", headers=headers)
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_set_timeout_continuous(client: AsyncClient):
    """Set auto-close timeout on continuous discussion."""
    token, pid = await _setup(client, "-timeout")
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "continuous", "topic": "Timeout test",
    }, headers=headers)
    disc_id = resp.json()["id"]

    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/set-timeout",
        json={"timeout_seconds": 120},
        headers=headers,
    )
    assert resp.status_code == 200
    assert resp.json()["auto_close_timeout_seconds"] == 120


@pytest.mark.asyncio
async def test_set_timeout_non_continuous_rejected(client: AsyncClient):
    """Setting timeout on non-continuous discussion returns 400."""
    token, pid = await _setup(client, "-timeout-bad")
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Wrong mode for timeout",
    }, headers=headers)
    disc_id = resp.json()["id"]

    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/set-timeout",
        json={"timeout_seconds": 60},
        headers=headers,
    )
    assert resp.status_code == 400
    assert "continuous" in resp.json()["detail"].lower()


@pytest.mark.asyncio
async def test_set_teams_non_oxford_rejected(client: AsyncClient):
    """Setting teams on non-Oxford discussion returns 400."""
    token, pid = await _setup(client, "-teams-bad")
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Wrong mode for teams",
    }, headers=headers)
    disc_id = resp.json()["id"]

    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/teams",
        json={"teams": {"for": ["a:0"], "against": ["b:0"]}},
        headers=headers,
    )
    assert resp.status_code == 400
    assert "Oxford" in resp.json()["detail"]


@pytest.mark.asyncio
async def test_track_submission(client: AsyncClient):
    """Track a board message as a discussion submission."""
    token, pid = await _setup(client, "-track")
    headers = auth_headers(token)

    # Create a board message first
    msg_resp = await client.post(f"/api/v1/messages/{pid}", json={
        "to": "all",
        "type": "status",
        "subject": "My submission",
        "body": "I think we should use microservices.",
    }, headers=headers)
    assert msg_resp.status_code == 200
    message_id = msg_resp.json()["id"]

    # Start a delphi discussion + open round
    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Architecture",
        "participants": ["architect:0", "developer:0"],
    }, headers=headers)
    disc_id = resp.json()["id"]

    await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/open-round", headers=headers)

    # Track the message as a submission
    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/track-submission",
        json={"from_role": "architect:0", "message_id": message_id},
        headers=headers,
    )
    assert resp.status_code == 200
    assert resp.json()["status"] == "submitted"
    assert resp.json()["from_role"] == "architect:0"


@pytest.mark.asyncio
async def test_track_submission_duplicate_rejected(client: AsyncClient):
    """Same role can't track-submit twice in the same round."""
    token, pid = await _setup(client, "-trackdup")
    headers = auth_headers(token)

    # Create two messages
    msg1 = await client.post(f"/api/v1/messages/{pid}", json={
        "to": "all", "type": "status",
        "subject": "Sub 1", "body": "First",
    }, headers=headers)
    assert msg1.status_code == 200
    msg2 = await client.post(f"/api/v1/messages/{pid}", json={
        "to": "all", "type": "status",
        "subject": "Sub 2", "body": "Second",
    }, headers=headers)
    assert msg2.status_code == 200

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Dup test",
    }, headers=headers)
    disc_id = resp.json()["id"]

    await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/open-round", headers=headers)

    # First track
    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/track-submission",
        json={"from_role": "dev:0", "message_id": msg1.json()["id"]},
        headers=headers,
    )
    assert resp.status_code == 200

    # Duplicate track
    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/track-submission",
        json={"from_role": "dev:0", "message_id": msg2.json()["id"]},
        headers=headers,
    )
    assert resp.status_code == 409


@pytest.mark.asyncio
async def test_track_submission_nonexistent_message(client: AsyncClient):
    """Tracking a non-existent message returns 404."""
    token, pid = await _setup(client, "-trackmiss")
    headers = auth_headers(token)

    resp = await client.post(f"/api/v1/projects/{pid}/discussions", json={
        "mode": "delphi", "topic": "Missing msg",
    }, headers=headers)
    disc_id = resp.json()["id"]

    await client.post(f"/api/v1/projects/{pid}/discussions/{disc_id}/open-round", headers=headers)

    resp = await client.post(
        f"/api/v1/projects/{pid}/discussions/{disc_id}/track-submission",
        json={"from_role": "dev:0", "message_id": 99999},
        headers=headers,
    )
    assert resp.status_code == 404
