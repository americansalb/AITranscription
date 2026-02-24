"""Tests for project CRUD and role management."""

import pytest
from httpx import AsyncClient

from tests.conftest import auth_headers, create_test_user


async def _create_project(client: AsyncClient, token: str, name: str = "Test Project") -> dict:
    resp = await client.post(
        "/api/v1/projects/",
        json={"name": name},
        headers=auth_headers(token),
    )
    assert resp.status_code == 201, f"Create project failed: {resp.text}"
    return resp.json()


@pytest.mark.asyncio
async def test_create_project(client: AsyncClient):
    data = await create_test_user(client)
    project = await _create_project(client, data["access_token"])
    assert project["name"] == "Test Project"
    assert project["is_active"] is True
    # Should have default roles
    slugs = {r["slug"] for r in project["roles"]}
    assert "manager" in slugs
    assert "architect" in slugs
    assert "developer" in slugs
    assert "tester" in slugs


@pytest.mark.asyncio
async def test_create_project_unauthenticated(client: AsyncClient):
    resp = await client.post("/api/v1/projects/", json={"name": "Fail"})
    assert resp.status_code in (401, 403)


@pytest.mark.asyncio
async def test_list_projects(client: AsyncClient):
    data = await create_test_user(client)
    token = data["access_token"]
    await _create_project(client, token, "Project A")
    await _create_project(client, token, "Project B")
    resp = await client.get("/api/v1/projects/", headers=auth_headers(token))
    assert resp.status_code == 200
    projects = resp.json()
    assert len(projects) == 2


@pytest.mark.asyncio
async def test_get_project(client: AsyncClient):
    data = await create_test_user(client)
    token = data["access_token"]
    project = await _create_project(client, token)
    resp = await client.get(f"/api/v1/projects/{project['id']}", headers=auth_headers(token))
    assert resp.status_code == 200
    assert resp.json()["name"] == "Test Project"


@pytest.mark.asyncio
async def test_get_other_users_project(client: AsyncClient):
    user1 = await create_test_user(client, "user1@test.com")
    user2 = await create_test_user(client, "user2@test.com")
    project = await _create_project(client, user1["access_token"])
    # User 2 should not see user 1's project
    resp = await client.get(f"/api/v1/projects/{project['id']}", headers=auth_headers(user2["access_token"]))
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_delete_project(client: AsyncClient):
    data = await create_test_user(client)
    token = data["access_token"]
    project = await _create_project(client, token)
    resp = await client.delete(f"/api/v1/projects/{project['id']}", headers=auth_headers(token))
    assert resp.status_code == 200
    # Should no longer appear in list
    resp = await client.get("/api/v1/projects/", headers=auth_headers(token))
    assert len(resp.json()) == 0


@pytest.mark.asyncio
async def test_update_role_provider(client: AsyncClient):
    data = await create_test_user(client)
    token = data["access_token"]
    project = await _create_project(client, token)
    resp = await client.put(
        f"/api/v1/projects/{project['id']}/roles/developer/provider",
        json={"provider": "openai", "model": "gpt-4o"},
        headers=auth_headers(token),
    )
    assert resp.status_code == 200
    updated = resp.json()
    assert updated["provider"] == "openai"
    assert updated["model"] == "gpt-4o"


@pytest.mark.asyncio
async def test_update_nonexistent_role(client: AsyncClient):
    data = await create_test_user(client)
    token = data["access_token"]
    project = await _create_project(client, token)
    resp = await client.put(
        f"/api/v1/projects/{project['id']}/roles/nonexistent/provider",
        json={"provider": "openai", "model": "gpt-4o"},
        headers=auth_headers(token),
    )
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_default_roles_have_correct_providers(client: AsyncClient):
    data = await create_test_user(client)
    project = await _create_project(client, data["access_token"])
    roles_by_slug = {r["slug"]: r for r in project["roles"]}
    # All default roles use anthropic
    for slug in ("manager", "architect", "developer"):
        assert roles_by_slug[slug]["provider"] == "anthropic"
    # Tester uses haiku
    assert "haiku" in roles_by_slug["tester"]["model"]
    # Developer has 3 instances
    assert roles_by_slug["developer"]["max_instances"] == 3
