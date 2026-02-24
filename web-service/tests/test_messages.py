"""Tests for the message board REST endpoints."""

import pytest
from httpx import AsyncClient

from tests.conftest import auth_headers, create_test_user


async def _setup_project(client: AsyncClient) -> tuple[str, int]:
    """Helper: create user + project, return (token, project_id)."""
    data = await create_test_user(client, f"msg-{id(client)}@test.com")
    token = data["access_token"]
    resp = await client.post("/api/v1/projects/", json={"name": "Msg Test"}, headers=auth_headers(token))
    return token, resp.json()["id"]


@pytest.mark.asyncio
async def test_send_and_get_messages(client: AsyncClient):
    token, pid = await _setup_project(client)
    headers = auth_headers(token)

    # Send a message
    resp = await client.post(f"/api/v1/messages/{pid}", json={
        "to": "all",
        "type": "broadcast",
        "subject": "Hello",
        "body": "World",
    }, headers=headers)
    assert resp.status_code == 200
    msg = resp.json()
    assert msg["subject"] == "Hello"
    assert msg["body"] == "World"
    assert msg["to"] == "all"
    assert msg["type"] == "broadcast"

    # Fetch messages
    resp = await client.get(f"/api/v1/messages/{pid}", headers=headers)
    assert resp.status_code == 200
    data = resp.json()
    assert data["total"] >= 1
    assert any(m["subject"] == "Hello" for m in data["messages"])


@pytest.mark.asyncio
async def test_messages_since_id(client: AsyncClient):
    token, pid = await _setup_project(client)
    headers = auth_headers(token)

    # Send two messages
    r1 = await client.post(f"/api/v1/messages/{pid}", json={"to": "all", "body": "First"}, headers=headers)
    first_id = r1.json()["id"]
    await client.post(f"/api/v1/messages/{pid}", json={"to": "all", "body": "Second"}, headers=headers)

    # Fetch since first ID — should only get second
    resp = await client.get(f"/api/v1/messages/{pid}?since_id={first_id}", headers=headers)
    data = resp.json()
    assert data["total"] == 1
    assert data["messages"][0]["body"] == "Second"


@pytest.mark.asyncio
async def test_messages_wrong_project(client: AsyncClient):
    data = await create_test_user(client)
    resp = await client.get("/api/v1/messages/99999", headers=auth_headers(data["access_token"]))
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_send_message_unauthenticated(client: AsyncClient):
    resp = await client.post("/api/v1/messages/1", json={"to": "all", "body": "nope"})
    assert resp.status_code in (401, 403)


@pytest.mark.asyncio
async def test_message_limit(client: AsyncClient):
    token, pid = await _setup_project(client)
    headers = auth_headers(token)

    for i in range(5):
        await client.post(f"/api/v1/messages/{pid}", json={"to": "all", "body": f"msg-{i}"}, headers=headers)

    resp = await client.get(f"/api/v1/messages/{pid}?limit=2", headers=headers)
    assert resp.json()["total"] == 2


@pytest.mark.asyncio
async def test_delete_message(client: AsyncClient):
    token, pid = await _setup_project(client)
    headers = auth_headers(token)

    # Send a message
    resp = await client.post(f"/api/v1/messages/{pid}", json={
        "to": "all",
        "body": "To be deleted",
    }, headers=headers)
    msg_id = resp.json()["id"]

    # Delete it
    resp = await client.delete(f"/api/v1/messages/{pid}/{msg_id}", headers=headers)
    assert resp.status_code == 204

    # Verify it's gone
    resp = await client.get(f"/api/v1/messages/{pid}", headers=headers)
    assert not any(m["id"] == msg_id for m in resp.json()["messages"])


@pytest.mark.asyncio
async def test_delete_nonexistent_message(client: AsyncClient):
    token, pid = await _setup_project(client)
    headers = auth_headers(token)

    resp = await client.delete(f"/api/v1/messages/{pid}/99999", headers=headers)
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_delete_message_wrong_project(client: AsyncClient):
    data = await create_test_user(client, f"del-wrong-{id(client)}@test.com")
    token = data["access_token"]
    headers = auth_headers(token)

    # Create project
    resp = await client.post("/api/v1/projects/", json={"name": "Del Test"}, headers=headers)
    pid = resp.json()["id"]

    # Send message
    resp = await client.post(f"/api/v1/messages/{pid}", json={"to": "all", "body": "test"}, headers=headers)
    msg_id = resp.json()["id"]

    # Try to delete from non-existent project
    resp = await client.delete(f"/api/v1/messages/99999/{msg_id}", headers=headers)
    assert resp.status_code == 404
