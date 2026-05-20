"""Vaaklite v1 document drafting — integration tests.

Per architect msg 5738 spec lock + tester:0 msg 5742 acceptance slate.
Covers the 7 endpoints under /api/v1/projects/{project_id}/documents/*
plus core service-layer happy + edge paths.
"""

import pytest

from app.api.documents import get_completion_fn
from app.main import app
from tests.conftest import auth_headers, create_test_user


SECRET = "testpass123"


@pytest.fixture
def fake_llm():
    """Override the LLM completion dependency with a deterministic fake.

    Yields a dict capturing the (model, system, prompt) of the last call
    so tests can assert what the agent was asked to draft. No network
    call is ever made — architect ruling msg 5793 keeps the real LLM as
    the v1 ship path but tests inject this fake for CI determinism.
    """
    captured: dict = {"calls": 0}

    async def _fake(model: str, system: str, prompt: str) -> str:
        captured["calls"] += 1
        captured["model"] = model
        captured["system"] = system
        captured["prompt"] = prompt
        return "AI-generated draft, first paragraph.\n\nSecond paragraph of the draft."

    app.dependency_overrides[get_completion_fn] = lambda: _fake
    yield captured
    app.dependency_overrides.pop(get_completion_fn, None)


@pytest.fixture
async def signed_in_user(client):
    """Sign up a fresh user + return their access token."""
    resp = await create_test_user(client, email="vaaklite@example.com")
    return resp["access_token"]


@pytest.fixture
async def discussion_project(client, signed_in_user):
    """Create a discussion-mode project with the simple-rotation template."""
    resp = await client.post(
        "/api/v1/projects/",
        json={"name": "Vaaklite Test Project", "mode": "discussion", "template": "simple-rotation"},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 201, resp.text
    return resp.json()


@pytest.fixture
async def coding_project(client, signed_in_user):
    """Create a default (coding-mode) project for negative tests."""
    resp = await client.post(
        "/api/v1/projects/",
        json={"name": "Coding Test Project"},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 201, resp.text
    return resp.json()


# ---------- Project create_project mode/template plumbing ----------


async def test_create_project_default_mode_is_coding(client, coding_project):
    assert coding_project["mode"] == "coding"
    assert coding_project["template"] is None
    # Legacy DEFAULT_ROLES roster seeded
    assert set(coding_project["roles"].keys()) >= {"manager", "architect", "developer", "tester"}


async def test_create_project_discussion_mode_seeds_template_roster(client, discussion_project):
    assert discussion_project["mode"] == "discussion"
    assert discussion_project["template"] == "simple-rotation"
    # simple-rotation roster: moderator + writer + reviewer
    role_slugs = set(discussion_project["roles"].keys())
    assert "moderator" in role_slugs
    assert "writer" in role_slugs
    assert "reviewer" in role_slugs


async def test_create_project_unknown_template_falls_back_to_simple_rotation(client, signed_in_user):
    resp = await client.post(
        "/api/v1/projects/",
        json={"name": "Unknown Tpl", "mode": "discussion", "template": "does-not-exist"},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 201, resp.text
    body = resp.json()
    assert body["mode"] == "discussion"
    # Service layer normalizes unknown template to simple-rotation
    assert body["template"] == "simple-rotation"


async def test_create_project_delphi_debate_template(client, signed_in_user):
    resp = await client.post(
        "/api/v1/projects/",
        json={"name": "Delphi", "mode": "discussion", "template": "delphi-debate"},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 201
    body = resp.json()
    assert "expert" in body["roles"]
    assert "synthesizer" in body["roles"]


# ---------- Document create + outline auto-derivation ----------


async def test_create_document_auto_derives_outline(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={"title": "Vision Brief", "topic": "Where the product goes next"},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 201, resp.text
    doc = resp.json()
    assert doc["title"] == "Vision Brief"
    assert doc["topic"] == "Where the product goes next"
    assert doc["phase"] == "drafting"
    assert doc["current_section_idx"] == 0
    assert doc["sections"][0]["status"] == "drafting"
    # Outline auto-derived from simple-rotation: writer + reviewer
    assert len(doc["sections"]) >= 1


async def test_create_document_with_explicit_outline(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "Custom Doc",
            "topic": "Test",
            "sections": [
                {"title": "Section A", "assigned_role": "writer"},
                {"title": "Section B", "assigned_role": "reviewer"},
            ],
        },
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 201, resp.text
    doc = resp.json()
    assert [s["title"] for s in doc["sections"]] == ["Section A", "Section B"]
    assert doc["sections"][0]["assigned_role"] == "writer"


async def test_create_document_rejects_coding_mode_project(client, signed_in_user, coding_project):
    pid = coding_project["id"]
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={"title": "Should fail", "topic": "Not allowed"},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 400
    assert "discussion" in resp.json()["detail"]


async def test_create_document_other_user_cannot_access(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    other = await create_test_user(client, email="other@example.com")
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={"title": "X", "topic": ""},
        headers=auth_headers(other["access_token"]),
    )
    assert resp.status_code == 404


# ---------- Submit + accept rotation flow ----------


async def test_submit_section_flips_to_review_pending(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "Test Doc",
            "topic": "",
            "sections": [
                {"title": "S1", "assigned_role": "writer"},
                {"title": "S2", "assigned_role": "reviewer"},
            ],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]

    resp = await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/submit",
        json={"section_idx": 0, "role_seat": "writer:0", "body": "Drafted content"},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 200, resp.text
    doc = resp.json()
    s0 = next(s for s in doc["sections"] if s["idx"] == 0)
    assert s0["status"] == "review_pending"
    assert s0["body"] == "Drafted content"


async def test_accept_section_advances_mic(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "Test Doc",
            "topic": "",
            "sections": [
                {"title": "S1", "assigned_role": "writer"},
                {"title": "S2", "assigned_role": "reviewer"},
            ],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]

    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/submit",
        json={"section_idx": 0, "role_seat": "writer:0", "body": "Drafted"},
        headers=auth_headers(signed_in_user),
    )
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/accept",
        json={"section_idx": 0},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 200
    doc = resp.json()
    s0 = next(s for s in doc["sections"] if s["idx"] == 0)
    s1 = next(s for s in doc["sections"] if s["idx"] == 1)
    assert s0["status"] == "accepted"
    assert s1["status"] == "drafting"
    assert doc["current_section_idx"] == 1
    assert doc["current_role"] == "reviewer"


async def test_accept_last_section_moves_phase_to_review(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "Solo",
            "topic": "",
            "sections": [{"title": "Only", "assigned_role": "writer"}],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]
    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/submit",
        json={"section_idx": 0, "role_seat": "writer:0", "body": "Done"},
        headers=auth_headers(signed_in_user),
    )
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/accept",
        json={"section_idx": 0},
        headers=auth_headers(signed_in_user),
    )
    doc = resp.json()
    assert doc["phase"] == "review"
    assert doc["current_section_idx"] is None
    assert doc["current_role"] is None


async def test_accept_rejects_drafting_status(client, signed_in_user, discussion_project):
    """Can only accept sections that are in review_pending — not drafting/pending."""
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "X",
            "topic": "",
            "sections": [
                {"title": "A", "assigned_role": "writer"},
                {"title": "B", "assigned_role": "reviewer"},
            ],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]
    # Try to accept idx 0 while still in 'drafting'
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/accept",
        json={"section_idx": 0},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 400
    assert "review_pending" in resp.json()["detail"]


async def test_resubmit_after_accept_rejected(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "X",
            "topic": "",
            "sections": [{"title": "A", "assigned_role": "writer"}],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]
    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/submit",
        json={"section_idx": 0, "role_seat": "writer:0", "body": "first"},
        headers=auth_headers(signed_in_user),
    )
    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/accept",
        json={"section_idx": 0},
        headers=auth_headers(signed_in_user),
    )
    # Now try to redraft an accepted section
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/submit",
        json={"section_idx": 0, "role_seat": "writer:0", "body": "second"},
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 400
    assert "accepted" in resp.json()["detail"]


# ---------- Finalize + markdown download ----------


async def test_finalize_locks_markdown(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "Final",
            "topic": "Test topic",
            "sections": [{"title": "Only", "assigned_role": "writer"}],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]
    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/submit",
        json={"section_idx": 0, "role_seat": "writer:0", "body": "Body text"},
        headers=auth_headers(signed_in_user),
    )
    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/accept",
        json={"section_idx": 0},
        headers=auth_headers(signed_in_user),
    )

    resp = await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/finalize",
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 200
    doc = resp.json()
    assert doc["phase"] == "final"
    assert doc["final_markdown"] is not None
    assert "Body text" in doc["final_markdown"]
    assert "# Final" in doc["final_markdown"]

    # Download endpoint returns the final markdown
    download = await client.get(
        f"/api/v1/projects/{pid}/documents/{doc_id}/markdown",
        headers=auth_headers(signed_in_user),
    )
    assert download.status_code == 200
    md = download.json()
    assert md["phase"] == "final"
    assert "Body text" in md["markdown"]


async def test_download_returns_current_markdown_when_not_final(
    client, signed_in_user, discussion_project
):
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={"title": "Draft", "topic": ""},
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]
    resp = await client.get(
        f"/api/v1/projects/{pid}/documents/{doc_id}/markdown",
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 200
    md = resp.json()
    assert md["phase"] in ("drafting", "review", "revision")
    assert "# Draft" in md["markdown"]


# ---------- List + get ----------


async def test_list_documents_under_project(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    for title in ("Doc A", "Doc B", "Doc C"):
        await client.post(
            f"/api/v1/projects/{pid}/documents",
            json={"title": title, "topic": ""},
            headers=auth_headers(signed_in_user),
        )
    resp = await client.get(
        f"/api/v1/projects/{pid}/documents",
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 200
    docs = resp.json()
    assert len(docs) == 3
    assert {d["title"] for d in docs} == {"Doc A", "Doc B", "Doc C"}


async def test_get_single_document_includes_sections(client, signed_in_user, discussion_project):
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "X",
            "topic": "",
            "sections": [{"title": "Alpha", "assigned_role": "writer"}],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]
    resp = await client.get(
        f"/api/v1/projects/{pid}/documents/{doc_id}",
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 200
    doc = resp.json()
    assert doc["title"] == "X"
    assert doc["sections"][0]["title"] == "Alpha"


# ---------- Agent drafting (LLM wire-up) ----------


async def test_draft_current_generates_and_submits(
    client, signed_in_user, discussion_project, fake_llm
):
    """draft-current invokes the LLM, persists the body, flips to review_pending."""
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "Vision",
            "topic": "Where the product goes",
            "sections": [{"title": "Opening", "assigned_role": "writer"}],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]

    resp = await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/draft-current",
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 200, resp.text
    doc = resp.json()
    s0 = next(s for s in doc["sections"] if s["idx"] == 0)
    assert s0["status"] == "review_pending"
    assert "AI-generated draft" in s0["body"]
    assert fake_llm["calls"] == 1
    # The agent prompt carries the section title it was asked to draft
    assert "Opening" in fake_llm["prompt"]


async def test_draft_current_uses_assigned_role_model(
    client, signed_in_user, discussion_project, fake_llm
):
    """The drafting call uses the assigned role's configured model."""
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "Doc",
            "topic": "",
            "sections": [{"title": "S", "assigned_role": "writer"}],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]
    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/draft-current",
        headers=auth_headers(signed_in_user),
    )
    # simple-rotation seeds writer with claude-sonnet-4-6
    assert fake_llm["model"] == "claude-sonnet-4-6"


async def test_draft_current_full_rotation_to_review(
    client, signed_in_user, discussion_project, fake_llm
):
    """Draft + accept each section in turn → document reaches REVIEW phase."""
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "Rotation",
            "topic": "",
            "sections": [
                {"title": "First", "assigned_role": "writer"},
                {"title": "Second", "assigned_role": "reviewer"},
            ],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]

    for idx in (0, 1):
        draft = await client.post(
            f"/api/v1/projects/{pid}/documents/{doc_id}/draft-current",
            headers=auth_headers(signed_in_user),
        )
        assert draft.status_code == 200, draft.text
        accept = await client.post(
            f"/api/v1/projects/{pid}/documents/{doc_id}/accept",
            json={"section_idx": idx},
            headers=auth_headers(signed_in_user),
        )
        assert accept.status_code == 200, accept.text

    doc = accept.json()
    assert doc["phase"] == "review"
    assert all(s["status"] == "accepted" for s in doc["sections"])
    assert fake_llm["calls"] == 2


async def test_draft_current_rejects_coding_project(
    client, signed_in_user, coding_project, fake_llm
):
    """draft-current is a discussion-mode-only capability."""
    pid = coding_project["id"]
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents/999/draft-current",
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 400
    assert "discussion" in resp.json()["detail"]
    assert fake_llm["calls"] == 0


async def test_draft_current_rejects_after_finalize(
    client, signed_in_user, discussion_project, fake_llm
):
    """Once a document is FINAL, agent drafting is rejected (cross-phase guard)."""
    pid = discussion_project["id"]
    create_resp = await client.post(
        f"/api/v1/projects/{pid}/documents",
        json={
            "title": "Done",
            "topic": "",
            "sections": [{"title": "Only", "assigned_role": "writer"}],
        },
        headers=auth_headers(signed_in_user),
    )
    doc_id = create_resp.json()["id"]
    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/draft-current",
        headers=auth_headers(signed_in_user),
    )
    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/accept",
        json={"section_idx": 0},
        headers=auth_headers(signed_in_user),
    )
    await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/finalize",
        headers=auth_headers(signed_in_user),
    )
    calls_before = fake_llm["calls"]
    resp = await client.post(
        f"/api/v1/projects/{pid}/documents/{doc_id}/draft-current",
        headers=auth_headers(signed_in_user),
    )
    assert resp.status_code == 400
    assert "DRAFTING" in resp.json()["detail"]
    assert fake_llm["calls"] == calls_before
