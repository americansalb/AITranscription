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
    # Roles is a dict keyed by slug
    assert "manager" in project["roles"]
    assert "architect" in project["roles"]
    assert "developer" in project["roles"]
    assert "tester" in project["roles"]


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
    assert resp.status_code in (200, 204)
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
    roles = project["roles"]
    # All default roles use anthropic
    for slug in ("manager", "architect", "developer"):
        assert roles[slug]["provider"]["provider"] == "anthropic"
    # Tester uses haiku
    assert "haiku" in roles["tester"]["provider"]["model"]
    # Developer has 3 instances
    assert roles["developer"]["maxInstances"] == 3


# --- Role CRUD ---

@pytest.mark.asyncio
async def test_create_role(client: AsyncClient):
    data = await create_test_user(client, "role-create@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)
    pid = project["id"]

    resp = await client.post(
        f"/api/v1/projects/{pid}/roles",
        json={
            "slug": "researcher",
            "title": "Researcher",
            "description": "Deep research and fact-checking",
            "maxInstances": 2,
        },
        headers=auth_headers(token),
    )
    assert resp.status_code == 201
    role = resp.json()
    assert role["slug"] == "researcher"
    assert role["title"] == "Researcher"
    assert role["maxInstances"] == 2

    # Verify it appears in project
    resp = await client.get(f"/api/v1/projects/{pid}", headers=auth_headers(token))
    assert "researcher" in resp.json()["roles"]


@pytest.mark.asyncio
async def test_create_duplicate_role(client: AsyncClient):
    data = await create_test_user(client, "role-dup@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)
    pid = project["id"]

    resp = await client.post(
        f"/api/v1/projects/{pid}/roles",
        json={"slug": "manager", "title": "Duplicate Manager"},
        headers=auth_headers(token),
    )
    assert resp.status_code == 409


@pytest.mark.asyncio
async def test_delete_role(client: AsyncClient):
    data = await create_test_user(client, "role-del@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)
    pid = project["id"]

    resp = await client.delete(
        f"/api/v1/projects/{pid}/roles/tester",
        headers=auth_headers(token),
    )
    assert resp.status_code == 204

    # Verify it's gone
    resp = await client.get(f"/api/v1/projects/{pid}", headers=auth_headers(token))
    assert "tester" not in resp.json()["roles"]


@pytest.mark.asyncio
async def test_delete_nonexistent_role(client: AsyncClient):
    data = await create_test_user(client, "role-delnone@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)

    resp = await client.delete(
        f"/api/v1/projects/{project['id']}/roles/nosuchrole",
        headers=auth_headers(token),
    )
    assert resp.status_code == 404


# --- Briefings ---

@pytest.mark.asyncio
async def test_get_briefing(client: AsyncClient):
    data = await create_test_user(client, "brief-get@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)

    resp = await client.get(
        f"/api/v1/projects/{project['id']}/roles/developer/briefing",
        headers=auth_headers(token),
    )
    assert resp.status_code == 200
    assert "briefing" in resp.json()


@pytest.mark.asyncio
async def test_update_briefing(client: AsyncClient):
    data = await create_test_user(client, "brief-put@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)
    pid = project["id"]

    resp = await client.put(
        f"/api/v1/projects/{pid}/roles/developer/briefing",
        json={"briefing": "You are the lead developer."},
        headers=auth_headers(token),
    )
    assert resp.status_code == 200

    # Read it back
    resp = await client.get(
        f"/api/v1/projects/{pid}/roles/developer/briefing",
        headers=auth_headers(token),
    )
    assert resp.json()["briefing"] == "You are the lead developer."


# --- Agent actions ---

@pytest.mark.asyncio
async def test_buzz_agent(client: AsyncClient):
    data = await create_test_user(client, "buzz@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)

    resp = await client.post(
        f"/api/v1/projects/{project['id']}/roles/developer/buzz",
        json={"instance": 0},
        headers=auth_headers(token),
    )
    assert resp.status_code == 200
    assert resp.json()["status"] == "buzzed"


@pytest.mark.asyncio
async def test_interrupt_agent(client: AsyncClient):
    data = await create_test_user(client, "interrupt@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)

    resp = await client.post(
        f"/api/v1/projects/{project['id']}/roles/developer/interrupt",
        json={"reason": "Stop what you're doing", "instance": 0},
        headers=auth_headers(token),
    )
    assert resp.status_code == 200
    assert resp.json()["status"] == "interrupted"


@pytest.mark.asyncio
async def test_buzz_nonexistent_role(client: AsyncClient):
    data = await create_test_user(client, "buzz-none@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)

    resp = await client.post(
        f"/api/v1/projects/{project['id']}/roles/fakerobot/buzz",
        json={"instance": 0},
        headers=auth_headers(token),
    )
    assert resp.status_code == 404


# --- Stubs ---

@pytest.mark.asyncio
async def test_file_claims_returns_empty(client: AsyncClient):
    data = await create_test_user(client, "claims@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)

    resp = await client.get(
        f"/api/v1/projects/{project['id']}/claims",
        headers=auth_headers(token),
    )
    assert resp.status_code == 200
    assert resp.json() == []


@pytest.mark.asyncio
async def test_sections_returns_default(client: AsyncClient):
    data = await create_test_user(client, "sections@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)

    resp = await client.get(
        f"/api/v1/projects/{project['id']}/sections",
        headers=auth_headers(token),
    )
    assert resp.status_code == 200
    sections = resp.json()
    assert len(sections) == 1
    assert sections[0]["slug"] == "default"


@pytest.mark.asyncio
async def test_switch_section(client: AsyncClient):
    data = await create_test_user(client, "switch@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)

    resp = await client.post(
        f"/api/v1/projects/{project['id']}/sections/default/switch",
        headers=auth_headers(token),
    )
    assert resp.status_code == 200


# =============================================================================
# EDGE CASES & SECURITY (added by tester)
# =============================================================================

async def test_create_project_empty_name(client: AsyncClient):
    """Empty name returns 422 (Pydantic min_length=1)."""
    data = await create_test_user(client, "empty-name@test.com")
    resp = await client.post(
        "/api/v1/projects/",
        json={"name": ""},
        headers=auth_headers(data["access_token"]),
    )
    assert resp.status_code == 422


async def test_create_project_name_too_long(client: AsyncClient):
    """Name over 100 chars returns 422 (Pydantic max_length=100)."""
    data = await create_test_user(client, "long-name@test.com")
    resp = await client.post(
        "/api/v1/projects/",
        json={"name": "x" * 101},
        headers=auth_headers(data["access_token"]),
    )
    assert resp.status_code == 422


async def test_delete_other_users_project(client: AsyncClient):
    """Cannot delete another user's project (ownership check)."""
    owner = await create_test_user(client, "owner-del@test.com")
    attacker = await create_test_user(client, "attacker-del@test.com")
    project = await _create_project(client, owner["access_token"], "Owner's Project")

    resp = await client.delete(
        f"/api/v1/projects/{project['id']}",
        headers=auth_headers(attacker["access_token"]),
    )
    assert resp.status_code == 404  # appears as "not found" to non-owner


async def test_get_deleted_project_returns_404(client: AsyncClient):
    """Soft-deleted project should NOT be retrievable (bug was fixed: _get_user_project now filters by is_active)."""
    data = await create_test_user(client, "del-get@test.com")
    token = data["access_token"]
    project = await _create_project(client, token)

    await client.delete(
        f"/api/v1/projects/{project['id']}",
        headers=auth_headers(token),
    )

    # GET should return 404 after soft-delete
    resp = await client.get(
        f"/api/v1/projects/{project['id']}",
        headers=auth_headers(token),
    )
    assert resp.status_code == 404


async def test_update_provider_other_users_project(client: AsyncClient):
    """Cannot update role provider on another user's project."""
    owner = await create_test_user(client, "owner-prov@test.com")
    attacker = await create_test_user(client, "attacker-prov@test.com")
    project = await _create_project(client, owner["access_token"])

    resp = await client.put(
        f"/api/v1/projects/{project['id']}/roles/developer/provider",
        json={"provider": "openai", "model": "gpt-4o"},
        headers=auth_headers(attacker["access_token"]),
    )
    assert resp.status_code == 404


async def test_create_role_invalid_slug(client: AsyncClient):
    """Role slug with invalid chars returns 422."""
    data = await create_test_user(client, "slug-bad@test.com")
    project = await _create_project(client, data["access_token"])

    resp = await client.post(
        f"/api/v1/projects/{project['id']}/roles",
        json={"slug": "INVALID SLUG!", "title": "Bad"},
        headers=auth_headers(data["access_token"]),
    )
    assert resp.status_code == 422


async def test_multiple_projects_isolation(client: AsyncClient):
    """Roles in one project don't affect another."""
    data = await create_test_user(client, "isolate@test.com")
    token = data["access_token"]

    p1 = await _create_project(client, token, "Project 1")
    p2 = await _create_project(client, token, "Project 2")

    # Update developer provider in p1 only
    await client.put(
        f"/api/v1/projects/{p1['id']}/roles/developer/provider",
        json={"provider": "openai", "model": "gpt-4o"},
        headers=auth_headers(token),
    )

    # p2's developer should still be anthropic
    resp = await client.get(
        f"/api/v1/projects/{p2['id']}",
        headers=auth_headers(token),
    )
    roles = resp.json()["roles"]
    assert roles["developer"]["provider"]["provider"] == "anthropic"


async def test_nonexistent_project_returns_404(client: AsyncClient):
    """Accessing project ID 99999 returns 404."""
    data = await create_test_user(client, "noproject@test.com")
    resp = await client.get(
        "/api/v1/projects/99999",
        headers=auth_headers(data["access_token"]),
    )
    assert resp.status_code == 404


async def test_list_empty_projects(client: AsyncClient):
    """New user with no projects gets empty list."""
    data = await create_test_user(client, "newuser@test.com")
    resp = await client.get("/api/v1/projects/", headers=auth_headers(data["access_token"]))
    assert resp.status_code == 200
    assert resp.json() == []
