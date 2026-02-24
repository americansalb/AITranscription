"""Tests for audience voting and pool management endpoints.

Covers:
  - POST /audience/vote — collect AI audience votes (mocked LLM calls)
  - GET /audience/pools — list available pools
  - GET /audience/pools/{pool_id} — get specific pool
  - POST /audience/pools — create custom pool
  - DELETE /audience/pools/{pool_id} — delete custom pool
  - GET /audience/personas — list personas by pool
"""
import pytest
from unittest.mock import AsyncMock, MagicMock, patch

from tests.conftest import make_user


# === Mock audience pool objects ===

def make_persona(**overrides):
    """Create a mock Persona."""
    p = MagicMock()
    defaults = {
        "name": "Tech Analyst",
        "background": "10 years in software engineering",
        "values": "efficiency, innovation",
        "style": "analytical and direct",
        "provider": "openai",
    }
    defaults.update(overrides)
    for k, v in defaults.items():
        setattr(p, k, v)
    return p


def make_pool(pool_id="general", name="General Public", builtin=True, personas=None):
    """Create a mock AudiencePool."""
    pool = MagicMock()
    pool.id = pool_id
    pool.name = name
    pool.description = "General audience pool"
    pool.builtin = builtin
    pool.member_count = 27
    pool.providers = ["groq", "openai", "anthropic"]
    pool.personas = personas or [
        make_persona(name=f"Persona {i}", provider=["groq", "openai", "anthropic"][i % 3])
        for i in range(3)
    ]
    return pool


# === GET /audience/pools ===

class TestListPools:

    async def test_list_pools_returns_list(self, client):
        """Listing pools returns array of pool metadata."""
        mock_pools = [
            {"id": "general", "name": "General Public", "member_count": 27, "builtin": True},
            {"id": "software-dev", "name": "Software Developers", "member_count": 27, "builtin": True},
        ]
        with patch("app.api.audience.list_pools", return_value=mock_pools):
            resp = await client.get("/api/v1/audience/pools")

        assert resp.status_code == 200
        data = resp.json()
        assert len(data) == 2
        assert data[0]["id"] == "general"
        assert data[1]["id"] == "software-dev"

    async def test_list_pools_empty(self, client):
        """Empty pool list returns empty array."""
        with patch("app.api.audience.list_pools", return_value=[]):
            resp = await client.get("/api/v1/audience/pools")

        assert resp.status_code == 200
        assert resp.json() == []


# === GET /audience/pools/{pool_id} ===

class TestGetPool:

    async def test_get_pool_success(self, client):
        """Get existing pool returns full pool data with personas."""
        pool = make_pool()
        with patch("app.api.audience.load_pool", return_value=pool):
            resp = await client.get("/api/v1/audience/pools/general")

        assert resp.status_code == 200
        data = resp.json()
        assert data["id"] == "general"
        assert data["name"] == "General Public"
        assert data["builtin"] is True
        assert len(data["personas"]) == 3

    async def test_get_pool_not_found(self, client):
        """Get nonexistent pool returns 404."""
        with patch("app.api.audience.load_pool", return_value=None):
            resp = await client.get("/api/v1/audience/pools/nonexistent")

        assert resp.status_code == 404
        assert "not found" in resp.json()["detail"]

    async def test_get_pool_invalid_id(self, client):
        """Get pool with invalid ID returns 400."""
        with patch("app.api.audience.load_pool", side_effect=ValueError("Invalid pool ID")):
            resp = await client.get("/api/v1/audience/pools/invalid!")

        assert resp.status_code == 400


# === POST /audience/pools ===

class TestCreatePool:

    async def test_create_pool_success(self, client, auth_headers):
        """Create a custom pool with valid data succeeds."""
        user = make_user()
        personas = [
            {
                "name": f"Expert {i}",
                "background": "domain expert",
                "values": "accuracy",
                "style": "formal",
                "provider": ["groq", "openai", "anthropic"][i % 3],
            }
            for i in range(9)
        ]

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.audience.load_pool", return_value=None),
            patch("app.api.audience.save_pool") as mock_save,
        ):
            resp = await client.post("/api/v1/audience/pools", json={
                "id": "my-custom-pool",
                "name": "My Custom Pool",
                "description": "A test pool",
                "personas": personas,
            }, headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["id"] == "my-custom-pool"
        assert data["name"] == "My Custom Pool"
        assert data["member_count"] == 9
        mock_save.assert_called_once()

    async def test_create_pool_duplicate(self, client, auth_headers):
        """Creating pool with existing ID returns 409."""
        user = make_user()
        existing_pool = make_pool(pool_id="taken")

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.audience.load_pool", return_value=existing_pool),
        ):
            resp = await client.post("/api/v1/audience/pools", json={
                "id": "taken",
                "name": "Duplicate",
                "personas": [{"name": "A", "background": "B", "values": "C", "style": "D", "provider": "openai"}],
            }, headers=auth_headers)

        assert resp.status_code == 409
        assert "already exists" in resp.json()["detail"]

    async def test_create_pool_invalid_id_format(self, client, auth_headers):
        """Pool ID with invalid characters returns 400."""
        user = make_user()

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
        ):
            resp = await client.post("/api/v1/audience/pools", json={
                "id": "Invalid ID!",
                "name": "Bad ID",
                "personas": [{"name": "A", "background": "B", "values": "C", "style": "D", "provider": "openai"}],
            }, headers=auth_headers)

        assert resp.status_code == 400
        assert "lowercase" in resp.json()["detail"]

    async def test_create_pool_invalid_provider(self, client, auth_headers):
        """Pool with invalid provider returns 400."""
        user = make_user()

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.audience.load_pool", return_value=None),
        ):
            resp = await client.post("/api/v1/audience/pools", json={
                "id": "bad-provider",
                "name": "Bad Provider Pool",
                "personas": [{"name": "A", "background": "B", "values": "C", "style": "D", "provider": "invalid-llm"}],
            }, headers=auth_headers)

        assert resp.status_code == 400
        assert "Invalid provider" in resp.json()["detail"]

    async def test_create_pool_no_auth(self, client):
        """Creating pool without auth returns 401."""
        resp = await client.post("/api/v1/audience/pools", json={
            "id": "test",
            "name": "Test",
            "personas": [],
        })
        assert resp.status_code in (401, 422)


# === DELETE /audience/pools/{pool_id} ===

class TestDeletePool:

    async def test_delete_pool_success(self, client, auth_headers):
        """Deleting a custom pool succeeds."""
        user = make_user()

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.audience.delete_pool", return_value=True),
        ):
            resp = await client.delete("/api/v1/audience/pools/my-custom-pool", headers=auth_headers)

        assert resp.status_code == 200
        assert resp.json()["deleted"] == "my-custom-pool"

    async def test_delete_builtin_pool_forbidden(self, client, auth_headers):
        """Deleting a built-in pool returns 403."""
        user = make_user()
        builtin_pool = make_pool(pool_id="general", builtin=True)

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.audience.delete_pool", return_value=False),
            patch("app.api.audience.load_pool", return_value=builtin_pool),
        ):
            resp = await client.delete("/api/v1/audience/pools/general", headers=auth_headers)

        assert resp.status_code == 403
        assert "built-in" in resp.json()["detail"]

    async def test_delete_nonexistent_pool(self, client, auth_headers):
        """Deleting nonexistent pool returns 404."""
        user = make_user()

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.audience.delete_pool", return_value=False),
            patch("app.api.audience.load_pool", return_value=None),
        ):
            resp = await client.delete("/api/v1/audience/pools/gone", headers=auth_headers)

        assert resp.status_code == 404

    async def test_delete_pool_no_auth(self, client):
        """Deleting pool without auth returns 401."""
        resp = await client.delete("/api/v1/audience/pools/some-pool")
        assert resp.status_code == 401


# === GET /audience/personas ===

class TestListPersonas:

    async def test_list_personas_default_pool(self, client):
        """List personas without pool param uses 'general'."""
        pool = make_pool()
        with patch("app.api.audience.load_pool", return_value=pool) as mock_load:
            resp = await client.get("/api/v1/audience/personas")

        assert resp.status_code == 200
        data = resp.json()
        assert len(data) == 3
        assert all("name" in p for p in data)
        assert all("provider" in p for p in data)
        mock_load.assert_called_once_with("general")

    async def test_list_personas_specific_pool(self, client):
        """List personas for a specific pool."""
        pool = make_pool(pool_id="software-dev", name="Software Developers")
        with patch("app.api.audience.load_pool", return_value=pool) as mock_load:
            resp = await client.get("/api/v1/audience/personas?pool=software-dev")

        assert resp.status_code == 200
        mock_load.assert_called_once_with("software-dev")

    async def test_list_personas_pool_not_found(self, client):
        """List personas for nonexistent pool returns 404."""
        with patch("app.api.audience.load_pool", return_value=None):
            resp = await client.get("/api/v1/audience/personas?pool=nonexistent")

        assert resp.status_code == 404


# === POST /audience/vote ===

class TestAudienceVote:

    async def test_vote_success(self, client, auth_headers):
        """Successful vote returns tally and individual votes."""
        user = make_user()
        mock_result = {
            "topic": "AI will replace developers",
            "phase": "post",
            "pool": "general",
            "pool_name": "General Public",
            "total_voters": 3,
            "tally": {"for": 2, "against": 1},
            "tally_by_provider": {
                "openai": {"for": 1, "against": 0},
                "anthropic": {"for": 1, "against": 0},
                "groq": {"for": 0, "against": 1},
            },
            "votes": [
                {
                    "persona": "Tech Analyst",
                    "background": "10 years in tech",
                    "provider": "openai",
                    "model": "gpt-4o",
                    "vote": "for",
                    "rationale": "Automation trends support this.",
                    "latency_ms": 500,
                },
            ],
            "total_latency_ms": 1500,
        }

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.audience.collect_audience_votes", return_value=mock_result),
        ):
            resp = await client.post("/api/v1/audience/vote", json={
                "topic": "AI will replace developers",
                "arguments": "LLMs can write code now.",
                "phase": "post",
            }, headers=auth_headers)

        assert resp.status_code == 200
        data = resp.json()
        assert data["topic"] == "AI will replace developers"
        assert data["total_voters"] == 3
        assert "for" in data["tally"]
        assert len(data["votes"]) == 1

    async def test_vote_no_auth(self, client):
        """Vote without auth returns 401."""
        resp = await client.post("/api/v1/audience/vote", json={
            "topic": "Test topic",
        })
        assert resp.status_code == 401

    async def test_vote_collection_failed(self, client, auth_headers):
        """Vote collection that returns error with no votes returns 400."""
        user = make_user()
        mock_result = {"error": "All providers failed", "votes": []}

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.audience.collect_audience_votes", return_value=mock_result),
        ):
            resp = await client.post("/api/v1/audience/vote", json={
                "topic": "Test topic",
            }, headers=auth_headers)

        assert resp.status_code == 400
        assert "failed" in resp.json()["detail"]

    async def test_vote_invalid_request(self, client, auth_headers):
        """Vote with ValueError from service returns 400."""
        user = make_user()

        with (
            patch("app.api.auth.get_user_by_id", return_value=user),
            patch("app.api.audience.collect_audience_votes", side_effect=ValueError("Bad topic")),
        ):
            resp = await client.post("/api/v1/audience/vote", json={
                "topic": "x",
            }, headers=auth_headers)

        assert resp.status_code == 400
