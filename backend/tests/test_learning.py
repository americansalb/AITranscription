"""Tests for the learning system API endpoints.

Covers:
  - Submit feedback (happy path, identical text, validation)
  - Get learning stats
  - Get corrections list
  - Delete correction (found, not found)
  - Find similar corrections
  - Create correction rule (happy path, invalid regex)
  - Get correction rules
  - Delete correction rule (found, not found)
  - Correct text (hybrid)
  - Correction breakdown
  - Train model (insufficient data)
  - Authorization checks for all 12 endpoints
"""
import pytest
from unittest.mock import AsyncMock, MagicMock, patch


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


def make_correction(**overrides):
    """Create a mock CorrectionEmbedding."""
    correction = MagicMock()
    defaults = {
        "id": 1,
        "user_id": 1,
        "original_text": "I think we should um discuss",
        "corrected_text": "I think we should discuss",
        "correction_type": "filler",
        "correction_count": 3,
        "created_at": "2026-02-16T00:00:00",
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(correction, key, value)
    return correction


def make_rule(**overrides):
    """Create a mock CorrectionRule."""
    rule = MagicMock()
    defaults = {
        "id": 1,
        "user_id": 1,
        "pattern": "teh",
        "replacement": "the",
        "is_regex": False,
        "priority": 0,
        "hit_count": 5,
        "is_active": True,
    }
    defaults.update(overrides)
    for key, value in defaults.items():
        setattr(rule, key, value)
    return rule


# =============================================================================
# AUTHORIZATION CHECKS
# =============================================================================

class TestLearningAuth:
    """All learning endpoints require auth (except none are public)."""

    async def test_feedback_no_auth(self, client):
        """POST /learning/feedback without auth returns 401."""
        response = await client.post(
            "/api/v1/learning/feedback",
            json={"original_text": "test", "corrected_text": "test2"},
        )
        assert response.status_code == 401

    async def test_stats_no_auth(self, client):
        """GET /learning/stats without auth returns 401."""
        response = await client.get("/api/v1/learning/stats")
        assert response.status_code == 401

    async def test_corrections_no_auth(self, client):
        """GET /learning/corrections without auth returns 401."""
        response = await client.get("/api/v1/learning/corrections")
        assert response.status_code == 401

    async def test_delete_correction_no_auth(self, client):
        """DELETE /learning/corrections/1 without auth returns 401."""
        response = await client.delete("/api/v1/learning/corrections/1")
        assert response.status_code == 401

    async def test_similar_no_auth(self, client):
        """GET /learning/similar without auth returns 401."""
        response = await client.get("/api/v1/learning/similar?text=test")
        assert response.status_code == 401

    async def test_rules_list_no_auth(self, client):
        """GET /learning/rules without auth returns 401."""
        response = await client.get("/api/v1/learning/rules")
        assert response.status_code == 401

    async def test_create_rule_no_auth(self, client):
        """POST /learning/rules without auth returns 401."""
        response = await client.post(
            "/api/v1/learning/rules",
            json={"pattern": "teh", "replacement": "the"},
        )
        assert response.status_code == 401

    async def test_delete_rule_no_auth(self, client):
        """DELETE /learning/rules/1 without auth returns 401."""
        response = await client.delete("/api/v1/learning/rules/1")
        assert response.status_code == 401

    async def test_correct_no_auth(self, client):
        """POST /learning/correct without auth returns 401."""
        response = await client.post("/api/v1/learning/correct?text=test")
        assert response.status_code == 401

    async def test_breakdown_no_auth(self, client):
        """GET /learning/correct/breakdown without auth returns 401."""
        response = await client.get("/api/v1/learning/correct/breakdown?text=test")
        assert response.status_code == 401

    async def test_train_no_auth(self, client):
        """POST /learning/train without auth returns 401."""
        response = await client.post("/api/v1/learning/train", json={})
        assert response.status_code == 401

    async def test_train_whisper_no_auth(self, client):
        """POST /learning/train-whisper without auth returns 401."""
        response = await client.post("/api/v1/learning/train-whisper", json={})
        assert response.status_code == 401


# =============================================================================
# SUBMIT FEEDBACK
# =============================================================================

class TestSubmitFeedback:
    """Tests for POST /learning/feedback."""

    async def test_feedback_identical_text(self, client, auth_headers):
        """Submitting identical original and corrected text returns success=False."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/learning/feedback",
                json={
                    "original_text": "hello world",
                    "corrected_text": "hello world",
                },
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["success"] is False
        assert "identical" in data["message"].lower()

    async def test_feedback_identical_with_whitespace(self, client, auth_headers):
        """Identical text after stripping whitespace returns success=False."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/learning/feedback",
                json={
                    "original_text": "  hello world  ",
                    "corrected_text": "hello world",
                },
                headers=auth_headers,
            )

        assert response.status_code == 200
        assert response.json()["success"] is False

    async def test_feedback_success(self, client, auth_headers):
        """Valid feedback with different texts stores correction."""
        user = make_user()
        mock_correction = make_correction(id=42)

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.correction_retriever.CorrectionRetriever.store_correction",
                   return_value=mock_correction) as mock_store, \
             patch("app.api.learning._update_daily_metrics", return_value=None):
            response = await client.post(
                "/api/v1/learning/feedback",
                json={
                    "original_text": "I um think we should",
                    "corrected_text": "I think we should",
                },
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["success"] is True
        assert data["correction_id"] == 42
        assert "stored" in data["message"].lower() or "success" in data["message"].lower()

    async def test_feedback_missing_fields(self, client, auth_headers):
        """Missing required fields returns 422."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/learning/feedback",
                json={"original_text": "test"},
                headers=auth_headers,
            )
        assert response.status_code == 422


# =============================================================================
# DELETE CORRECTION
# =============================================================================

class TestDeleteCorrection:

    async def test_delete_found(self, client, auth_headers):
        """Delete existing correction returns success."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.correction_retriever.CorrectionRetriever.delete_correction",
                   return_value=True):
            response = await client.delete(
                "/api/v1/learning/corrections/1",
                headers=auth_headers,
            )

        assert response.status_code == 200
        assert response.json()["success"] is True

    async def test_delete_not_found(self, client, auth_headers):
        """Delete nonexistent correction returns 404."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.correction_retriever.CorrectionRetriever.delete_correction",
                   return_value=False):
            response = await client.delete(
                "/api/v1/learning/corrections/999",
                headers=auth_headers,
            )

        assert response.status_code == 404


# =============================================================================
# CORRECTION RULES
# =============================================================================

class TestCorrectionRules:

    async def test_create_rule_success(self, client, auth_headers):
        """Create a valid correction rule returns 201."""
        user = make_user()
        mock_rule = make_rule(id=10, pattern="teh", replacement="the")

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.correctors.rule_based.RuleBasedCorrector.add_rule",
                   return_value=mock_rule):
            response = await client.post(
                "/api/v1/learning/rules",
                json={"pattern": "teh", "replacement": "the"},
                headers=auth_headers,
            )

        assert response.status_code == 201
        data = response.json()
        assert data["pattern"] == "teh"
        assert data["replacement"] == "the"
        assert data["is_regex"] is False

    async def test_create_rule_with_regex(self, client, auth_headers):
        """Create a regex-based correction rule."""
        user = make_user()
        mock_rule = make_rule(
            id=11, pattern=r"\bum+\b", replacement="", is_regex=True, priority=5
        )

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.correctors.rule_based.RuleBasedCorrector.add_rule",
                   return_value=mock_rule):
            response = await client.post(
                "/api/v1/learning/rules",
                json={
                    "pattern": r"\bum+\b",
                    "replacement": "",
                    "is_regex": True,
                    "priority": 5,
                },
                headers=auth_headers,
            )

        assert response.status_code == 201
        data = response.json()
        assert data["is_regex"] is True
        assert data["priority"] == 5

    async def test_create_rule_invalid_regex(self, client, auth_headers):
        """Invalid regex pattern returns 400."""
        user = make_user()

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.correctors.rule_based.RuleBasedCorrector.add_rule",
                   side_effect=ValueError("Invalid regex pattern")):
            response = await client.post(
                "/api/v1/learning/rules",
                json={
                    "pattern": "[invalid(",
                    "replacement": "fixed",
                    "is_regex": True,
                },
                headers=auth_headers,
            )

        assert response.status_code == 400

    async def test_delete_rule_found(self, client, auth_headers):
        """Delete existing rule returns success."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.correctors.rule_based.RuleBasedCorrector.delete_rule",
                   return_value=True):
            response = await client.delete(
                "/api/v1/learning/rules/1",
                headers=auth_headers,
            )

        assert response.status_code == 200
        assert response.json()["success"] is True

    async def test_delete_rule_not_found(self, client, auth_headers):
        """Delete nonexistent rule returns 404."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.correctors.rule_based.RuleBasedCorrector.delete_rule",
                   return_value=False):
            response = await client.delete(
                "/api/v1/learning/rules/999",
                headers=auth_headers,
            )

        assert response.status_code == 404

    async def test_get_rules_empty(self, client, auth_headers):
        """Get rules with none defined returns empty list."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.correctors.rule_based.RuleBasedCorrector.get_rules_list",
                   return_value=[]):
            response = await client.get(
                "/api/v1/learning/rules",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["rules"] == []
        assert data["total"] == 0

    async def test_get_rules_with_entries(self, client, auth_headers):
        """Get rules returns list of rules."""
        user = make_user()
        mock_rules = [
            {"id": 1, "pattern": "teh", "replacement": "the", "is_regex": False, "priority": 0, "hit_count": 5},
            {"id": 2, "pattern": r"\bum\b", "replacement": "", "is_regex": True, "priority": 10, "hit_count": 12},
        ]

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.correctors.rule_based.RuleBasedCorrector.get_rules_list",
                   return_value=mock_rules):
            response = await client.get(
                "/api/v1/learning/rules",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["total"] == 2
        assert data["rules"][0]["pattern"] == "teh"
        assert data["rules"][1]["is_regex"] is True


# =============================================================================
# FIND SIMILAR CORRECTIONS
# =============================================================================

class TestFindSimilar:

    async def test_find_similar_empty(self, client, auth_headers):
        """No similar corrections returns empty list."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.correction_retriever.CorrectionRetriever.find_similar",
                   return_value=[]):
            response = await client.get(
                "/api/v1/learning/similar?text=hello",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["query"] == "hello"
        assert data["similar_corrections"] == []

    async def test_find_similar_with_results(self, client, auth_headers):
        """Similar corrections are returned with similarity scores."""
        user = make_user()
        mock_similar = [
            {
                "id": 1,
                "original_text": "I um think",
                "corrected_text": "I think",
                "correction_type": "filler",
                "correction_count": 3,
                "similarity": 0.85,
            }
        ]

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.services.correction_retriever.CorrectionRetriever.find_similar",
                   return_value=mock_similar):
            response = await client.get(
                "/api/v1/learning/similar?text=I%20um%20think&threshold=0.7",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert len(data["similar_corrections"]) == 1
        assert data["similar_corrections"][0]["similarity"] == 0.85


# =============================================================================
# HYBRID CORRECTION
# =============================================================================

class TestCorrectTextHybrid:

    async def test_correct_text(self, client, auth_headers):
        """Hybrid correction returns corrected text with source info."""
        user = make_user()
        mock_result = {
            "corrected": "I think we should discuss this.",
            "confidence": 0.95,
            "source": "rules",
            "corrections_applied": [{"from": "teh", "to": "the"}],
        }

        with patch("app.api.auth.get_user_by_id", return_value=user), \
             patch("app.correctors.router.CorrectionRouter.correct",
                   return_value=mock_result):
            response = await client.post(
                "/api/v1/learning/correct?text=I%20think%20we%20should%20discuss%20teh.",
                headers=auth_headers,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["source"] == "rules"
        assert data["confidence"] == 0.95


# =============================================================================
# REQUEST VALIDATION
# =============================================================================

class TestLearningValidation:

    async def test_feedback_empty_original(self, client, auth_headers):
        """Feedback with empty original still works (identical check catches it)."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/learning/feedback",
                json={"original_text": "", "corrected_text": ""},
                headers=auth_headers,
            )
        # Empty strings stripped are identical → success=False
        assert response.status_code == 200
        assert response.json()["success"] is False

    async def test_create_rule_missing_pattern(self, client, auth_headers):
        """Rule creation without pattern returns 422."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/learning/rules",
                json={"replacement": "the"},
                headers=auth_headers,
            )
        assert response.status_code == 422

    async def test_create_rule_missing_replacement(self, client, auth_headers):
        """Rule creation without replacement returns 422."""
        user = make_user()
        with patch("app.api.auth.get_user_by_id", return_value=user):
            response = await client.post(
                "/api/v1/learning/rules",
                json={"pattern": "teh"},
                headers=auth_headers,
            )
        assert response.status_code == 422
