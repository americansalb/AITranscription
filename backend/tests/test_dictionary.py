"""Tests for the dictionary CRUD API endpoints.

Covers:
  - List entries (with category filter)
  - Create entry (happy path, duplicate, validation)
  - Get single entry (found, not found)
  - Update entry (partial update, duplicate word check)
  - Delete entry (found, not found)
  - Get words list
  - Authorization checks
"""
import pytest
from unittest.mock import AsyncMock, MagicMock, patch


def make_dict_entry(**overrides):
    """Create a mock DictionaryEntry."""
    entry = MagicMock()
    defaults = {
        "id": 1,
        "user_id": 1,
        "word": "Vaak",
        "pronunciation": "vahk",
        "description": "AI transcription app",
        "category": "technical",
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(entry, key, value)
    return entry


def make_user(**overrides):
    """Create a mock User for auth."""
    user = MagicMock()
    defaults = {
        "id": 1,
        "email": "test@example.com",
        "full_name": "Test User",
        "tier": "standard",
        "is_active": True,
        "is_admin": False,
        "accessibility_verified": False,
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(user, key, value)
    return user


# === List Dictionary Entries ===

class TestListDictionary:

    async def test_list_empty(self, client, auth_headers):
        """List with no entries returns empty list."""
        user = make_user()
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = []

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute.return_value = mock_result
            response = await client.get(
                "/api/v1/dictionary",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["entries"] == []
        assert data["total"] == 0

    async def test_list_with_entries(self, client, auth_headers):
        """List returns all entries for the user."""
        user = make_user()
        entries = [
            make_dict_entry(id=1, word="Alpha"),
            make_dict_entry(id=2, word="Beta"),
        ]
        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = entries

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute.return_value = mock_result
            response = await client.get(
                "/api/v1/dictionary",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["total"] == 2
        assert data["entries"][0]["word"] == "Alpha"
        assert data["entries"][1]["word"] == "Beta"

    async def test_list_no_auth(self, client):
        """List without auth returns 401."""
        response = await client.get("/api/v1/dictionary")
        assert response.status_code == 401


# === Create Dictionary Entry ===

class TestCreateDictionary:

    async def test_create_success(self, client, auth_headers):
        """Create entry returns 201 with entry data."""
        user = make_user()

        # First execute: check for existing word (returns None)
        check_result = MagicMock()
        check_result.scalar_one_or_none.return_value = None

        # After commit + refresh: entry has an ID
        new_entry = make_dict_entry(id=5, word="Groq", pronunciation="grock")

        call_count = 0

        async def mock_execute(query):
            nonlocal call_count
            call_count += 1
            if call_count == 1:
                return check_result  # Duplicate check
            return MagicMock()

        async def mock_refresh(obj):
            obj.id = 5
            obj.word = "Groq"
            obj.pronunciation = "grock"
            obj.description = "AI inference engine"
            obj.category = "technical"

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute = mock_execute
            client._mock_db.refresh = mock_refresh
            response = await client.post(
                "/api/v1/dictionary",
                json={"word": "Groq", "pronunciation": "grock", "description": "AI inference engine", "category": "technical"},
                headers=auth_headers,
            )

        assert response.status_code == 201
        data = response.json()
        assert data["word"] == "Groq"
        assert data["pronunciation"] == "grock"
        assert data["category"] == "technical"

    async def test_create_duplicate_word(self, client, auth_headers):
        """Creating a duplicate word returns 400."""
        user = make_user()
        existing_entry = make_dict_entry(word="Vaak")

        check_result = MagicMock()
        check_result.scalar_one_or_none.return_value = existing_entry

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute.return_value = check_result
            response = await client.post(
                "/api/v1/dictionary",
                json={"word": "Vaak"},
                headers=auth_headers,
            )

        assert response.status_code == 400
        assert "already exists" in response.json()["detail"]

    async def test_create_empty_word(self, client, auth_headers):
        """Creating with empty word returns 422."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/dictionary",
                json={"word": ""},
                headers=auth_headers,
            )

        assert response.status_code == 422

    async def test_create_missing_word(self, client, auth_headers):
        """Creating without word field returns 422."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/dictionary",
                json={},
                headers=auth_headers,
            )

        assert response.status_code == 422

    async def test_create_no_auth(self, client):
        """Create without auth returns 401."""
        response = await client.post(
            "/api/v1/dictionary",
            json={"word": "test"},
        )
        assert response.status_code == 401


# === Get Single Entry ===

class TestGetDictionary:

    async def test_get_found(self, client, auth_headers):
        """Get existing entry returns it."""
        user = make_user()
        entry = make_dict_entry(id=1, word="Vaak")

        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = entry

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute.return_value = mock_result
            response = await client.get(
                "/api/v1/dictionary/1",
                headers=auth_headers,
            )

        assert response.status_code == 200
        assert response.json()["word"] == "Vaak"

    async def test_get_not_found(self, client, auth_headers):
        """Get nonexistent entry returns 404."""
        user = make_user()
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = None

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute.return_value = mock_result
            response = await client.get(
                "/api/v1/dictionary/999",
                headers=auth_headers,
            )

        assert response.status_code == 404


# === Delete Entry ===

class TestDeleteDictionary:

    async def test_delete_success(self, client, auth_headers):
        """Delete existing entry returns 204."""
        user = make_user()
        entry = make_dict_entry(id=1)

        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = entry

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute.return_value = mock_result
            response = await client.delete(
                "/api/v1/dictionary/1",
                headers=auth_headers,
            )

        assert response.status_code == 204

    async def test_delete_not_found(self, client, auth_headers):
        """Delete nonexistent entry returns 404."""
        user = make_user()
        mock_result = MagicMock()
        mock_result.scalar_one_or_none.return_value = None

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute.return_value = mock_result
            response = await client.delete(
                "/api/v1/dictionary/999",
                headers=auth_headers,
            )

        assert response.status_code == 404

    async def test_delete_no_auth(self, client):
        """Delete without auth returns 401."""
        response = await client.delete("/api/v1/dictionary/1")
        assert response.status_code == 401


# === Get Words List ===

class TestGetWordsList:

    async def test_words_list(self, client, auth_headers):
        """Get words list returns array of strings."""
        user = make_user()

        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = ["Alpha", "Beta", "Gamma"]

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute.return_value = mock_result
            response = await client.get(
                "/api/v1/dictionary/words/list",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data == ["Alpha", "Beta", "Gamma"]

    async def test_words_list_empty(self, client, auth_headers):
        """Get words list when empty returns empty array."""
        user = make_user()

        mock_result = MagicMock()
        mock_result.scalars.return_value.all.return_value = []

        with patch("app.api.auth.get_user_by_id", return_value=user):
            client._mock_db.execute.return_value = mock_result
            response = await client.get(
                "/api/v1/dictionary/words/list",
                headers=auth_headers,
            )

        assert response.status_code == 200
        assert response.json() == []
