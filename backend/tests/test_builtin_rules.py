"""Tests for the built-in rule-based corrections.

Tests the apply_builtin_rules method which is synchronous and doesn't need a database.
Covers:
  - Filler word removal (um, uh, er, ah, hmm)
  - Stutter/repeated word correction
  - Double space normalization
"""
import pytest

from app.correctors.rule_based import RuleBasedCorrector, BUILTIN_PATTERNS


@pytest.fixture
def corrector():
    """RuleBasedCorrector with mock DB (only used for builtin rules here)."""
    from unittest.mock import AsyncMock
    mock_db = AsyncMock()
    return RuleBasedCorrector(db=mock_db, user_id=1)


# === Filler word removal ===

class TestFillerRemoval:

    @pytest.mark.parametrize("filler", ["um", "uh", "er", "ah", "hmm"])
    def test_single_filler_removed(self, corrector, filler):
        text = f"I {filler} need to send an email"
        result, corrections = corrector.apply_builtin_rules(text)
        assert filler not in result.lower()
        assert "filler" in [c["rule_type"] for c in corrections]

    def test_multiple_fillers_removed(self, corrector):
        text = "um I uh need to er send an email"
        result, corrections = corrector.apply_builtin_rules(text)
        assert "um" not in result.lower().split()
        assert "uh" not in result.lower().split()
        assert "er" not in result.lower().split()

    def test_filler_at_start(self, corrector):
        text = "Um hello there"
        result, _ = corrector.apply_builtin_rules(text)
        assert not result.lower().startswith("um ")

    def test_filler_case_insensitive(self, corrector):
        text = "I UM need to UH do this"
        result, _ = corrector.apply_builtin_rules(text)
        assert "UM" not in result
        assert "UH" not in result

    def test_word_containing_filler_not_removed(self, corrector):
        """Words like 'umbrella' should not be affected by filler removal."""
        text = "I need an umbrella"
        result, _ = corrector.apply_builtin_rules(text)
        assert "umbrella" in result


# === Stutter correction ===

class TestStutterCorrection:

    def test_simple_stutter(self, corrector):
        text = "I I need to send an email"
        result, corrections = corrector.apply_builtin_rules(text)
        assert "I I" not in result
        assert "I need" in result

    def test_triple_stutter(self, corrector):
        text = "the the the meeting is at noon"
        result, _ = corrector.apply_builtin_rules(text)
        assert "the the" not in result

    def test_no_stutter_different_words(self, corrector):
        text = "I need to go to the store"
        result, _ = corrector.apply_builtin_rules(text)
        assert result.strip() == text.strip()


# === Spacing normalization ===

class TestSpacingNormalization:

    def test_double_spaces_fixed(self, corrector):
        text = "hello  world"
        result, corrections = corrector.apply_builtin_rules(text)
        assert "  " not in result
        assert result == "hello world"

    def test_triple_spaces_fixed(self, corrector):
        text = "hello   world"
        result, _ = corrector.apply_builtin_rules(text)
        assert "  " not in result

    def test_single_spaces_preserved(self, corrector):
        text = "hello world"
        result, _ = corrector.apply_builtin_rules(text)
        assert result == "hello world"


# === Combined corrections ===

class TestCombinedCorrections:

    def test_filler_and_spacing(self, corrector):
        text = "I um  need to send an email"
        result, corrections = corrector.apply_builtin_rules(text)
        assert "um" not in result.lower().split()
        assert "  " not in result

    def test_no_changes_clean_text(self, corrector):
        """Clean text should pass through unchanged."""
        text = "Please send the quarterly report to the finance team"
        result, corrections = corrector.apply_builtin_rules(text)
        assert result == text
        assert len(corrections) == 0
