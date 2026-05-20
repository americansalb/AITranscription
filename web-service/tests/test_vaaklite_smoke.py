"""Vaaklite v1 — end-to-end smoke test.

Walks the 7-item acceptance slate from vaaklite-spec-2026-05-19.md in a
single flow: sign up -> log in -> create a discussion project -> roles
seeded from a preset -> create a document -> agents draft every section
in rotation -> finalize -> download the markdown.

The LLM is the injected fake (architect ruling msg 5793 keeps the real
LLM as the ship path; CI uses the fake for determinism — no network).
"""

import pytest

from app.api.documents import get_completion_fn
from app.main import app
from tests.conftest import auth_headers


@pytest.fixture
def fake_llm():
    """Override the agent-drafting completion dependency with a fake."""

    async def _fake(model: str, system: str, prompt: str) -> str:
        return f"Drafted section body via {model}. Context: {len(prompt)} chars."

    app.dependency_overrides[get_completion_fn] = lambda: _fake
    yield
    app.dependency_overrides.pop(get_completion_fn, None)


async def test_vaaklite_full_smoke(client, fake_llm):
    """The full 7-item Vaaklite acceptance flow, end to end."""

    # 1. Sign up
    signup = await client.post(
        "/api/v1/auth/signup",
        json={"email": "smoke@example.com", "password": "smokepass123", "full_name": "Smoke"},
    )
    assert signup.status_code == 201, signup.text

    # 2. Log in
    login = await client.post(
        "/api/v1/auth/login",
        json={"email": "smoke@example.com", "password": "smokepass123"},
    )
    assert login.status_code == 200, login.text
    headers = auth_headers(login.json()["access_token"])

    # 3. Create a discussion-mode project
    proj_resp = await client.post(
        "/api/v1/projects/",
        json={"name": "Smoke Doc Project", "mode": "discussion", "template": "simple-rotation"},
        headers=headers,
    )
    assert proj_resp.status_code == 201, proj_resp.text
    project = proj_resp.json()
    pid = project["id"]
    assert project["mode"] == "discussion"

    # 4. Roles seeded from the preset (configure 2-4 roles from preset)
    assert {"moderator", "writer", "reviewer"} <= set(project["roles"].keys())

    # 5. Start a drafting session (create a document on a topic)
    doc_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={"title": "Smoke Vision", "topic": "Prove the rotation works end to end"},
        headers=headers,
    )
    assert doc_resp.status_code == 201, doc_resp.text
    doc = doc_resp.json()
    doc_id = doc["id"]
    assert doc["phase"] == "drafting"
    section_count = len(doc["sections"])
    assert section_count >= 1

    # 6. Agents take turns drafting sections — draft + accept each in rotation
    accept_resp = None
    for idx in range(section_count):
        drafted = await client.post(
            f"/api/v1/projects/{pid}/documents/{doc_id}/draft-current", headers=headers
        )
        assert drafted.status_code == 200, drafted.text
        section = next(s for s in drafted.json()["sections"] if s["idx"] == idx)
        assert section["status"] == "review_pending"
        assert section["body"], "agent draft should not be empty"

        accept_resp = await client.post(
            f"/api/v1/projects/{pid}/documents/{doc_id}/accept",
            json={"section_idx": idx},
            headers=headers,
        )
        assert accept_resp.status_code == 200, accept_resp.text

    review = accept_resp.json()
    assert review["phase"] == "review"
    assert all(s["status"] == "accepted" for s in review["sections"])

    # 7. Final document persists + is downloadable as markdown
    final = await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/finalize", headers=headers
    )
    assert final.status_code == 200, final.text
    assert final.json()["phase"] == "final"

    download = await client.get(
        f"/api/v1/projects/{pid}/documents/{doc_id}/markdown", headers=headers
    )
    assert download.status_code == 200, download.text
    markdown = download.json()
    assert markdown["phase"] == "final"
    assert "# Smoke Vision" in markdown["markdown"]
    assert "Drafted section body" in markdown["markdown"]


async def test_vaaklite_smoke_persists_across_relogin(client, fake_llm):
    """Smoke item 7 corollary — a finalized document survives a fresh login."""

    await client.post(
        "/api/v1/auth/signup",
        json={"email": "persist@example.com", "password": "persistpass1", "full_name": "P"},
    )
    first = await client.post(
        "/api/v1/auth/login",
        json={"email": "persist@example.com", "password": "persistpass1"},
    )
    headers = auth_headers(first.json()["access_token"])

    proj = await client.post(
        "/api/v1/projects/",
        json={"name": "Persist", "mode": "discussion", "template": "simple-rotation"},
        headers=headers,
    )
    pid = proj.json()["id"]
    doc = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={"title": "Persisted Doc", "topic": ""},
        headers=headers,
    )
    doc_id = doc.json()["id"]

    # Fresh login — new token, same account
    second = await client.post(
        "/api/v1/auth/login",
        json={"email": "persist@example.com", "password": "persistpass1"},
    )
    new_headers = auth_headers(second.json()["access_token"])

    reloaded = await client.get(
        f"/api/v1/projects/{pid}/documents/{doc_id}", headers=new_headers
    )
    assert reloaded.status_code == 200, reloaded.text
    assert reloaded.json()["title"] == "Persisted Doc"
