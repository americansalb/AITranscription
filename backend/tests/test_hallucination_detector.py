"""Tests for the HallucinationDetector.

The hallucination detector is pure Python — no database or external services needed.
Tests the 4-layer detection system:
  1. Obvious YouTube-style hallucinations
  2. Short audio with suspicious content
  3. Trailing disconnected hallucinations
  4. Repeated word detection
"""
import pytest

from app.correctors.rule_based import HallucinationDetector


@pytest.fixture
def detector():
    return HallucinationDetector()


# === Layer 1: Obvious hallucinations ===

class TestObviousHallucinations:
    """YouTube-style phrases that nobody would actually dictate."""

    @pytest.mark.parametrize("phrase", [
        "thank you for watching",
        "Thank you for watching.",
        "thanks for watching",
        "see you next time",
        "don't forget to subscribe",
        "please like and subscribe",
        "like and subscribe",
    ])
    def test_obvious_hallucination_detected(self, detector, phrase):
        result = detector.detect(phrase)
        assert result["is_hallucination"] is True
        assert result["confidence"] >= 0.95
        assert result["corrected"] == ""
        assert len(result["detections"]) > 0
        assert result["detections"][0]["type"] == "obvious_hallucination"

    @pytest.mark.parametrize("phrase", [
        "thank you for watching",
        "THANK YOU FOR WATCHING",
        "Thank You For Watching.",
    ])
    def test_obvious_hallucination_case_insensitive(self, detector, phrase):
        result = detector.detect(phrase)
        assert result["is_hallucination"] is True

    def test_foreign_language_hallucinations(self, detector):
        """Foreign language YouTube phrases should be caught."""
        for phrase in ["字幕", "자막", "ご視聴ありがとうございました"]:
            result = detector.detect(phrase)
            assert result["is_hallucination"] is True, f"Failed for: {phrase}"

    def test_normal_text_not_flagged(self, detector):
        """Real transcription text should not be flagged."""
        result = detector.detect("I need to send an email to the team about the meeting")
        assert result["is_hallucination"] is False
        assert result["corrected"] == "I need to send an email to the team about the meeting"

    def test_partial_match_not_flagged(self, detector):
        """Text containing hallucination phrases embedded in real sentences should not be flagged."""
        result = detector.detect("I want to thank you for watching over the kids")
        assert result["is_hallucination"] is False


# === Layer 2: Short audio suspicious content ===

class TestShortAudioHallucinations:

    def test_short_audio_with_suspicious_word(self, detector):
        """Very short audio with a single suspicious word should be flagged."""
        result = detector.detect("thank you", audio_duration_seconds=0.3)
        assert result["is_hallucination"] is True
        assert result["confidence"] >= 0.90
        assert result["detections"][0]["type"] == "short_audio_hallucination"

    def test_short_audio_threshold(self, detector):
        """Audio at exactly 0.5s should NOT trigger the short audio filter."""
        result = detector.detect("thank you", audio_duration_seconds=0.5)
        assert result["is_hallucination"] is False

    def test_normal_duration_suspicious_word(self, detector):
        """Normal duration audio with 'thank you' should NOT be flagged (user really said it)."""
        result = detector.detect("thank you", audio_duration_seconds=2.0)
        assert result["is_hallucination"] is False

    def test_no_duration_info(self, detector):
        """Without audio duration, suspicious single words should NOT be auto-removed."""
        result = detector.detect("thank you")
        assert result["is_hallucination"] is False

    @pytest.mark.parametrize("phrase", ["okay", "yeah", "yes", "no", "hmm", "bye"])
    def test_short_audio_common_words(self, detector, phrase):
        """Common short words should be caught on very short audio."""
        result = detector.detect(phrase, audio_duration_seconds=0.2)
        assert result["is_hallucination"] is True


# === Layer 3: Trailing disconnected hallucinations ===

class TestTrailingHallucinations:

    def test_trailing_after_period(self, detector):
        """Trailing word after complete sentence should be removed."""
        result = detector.detect("I finished the report. Okay")
        assert "Okay" not in result["corrected"]
        assert len(result["detections"]) > 0

    def test_trailing_after_exclamation(self, detector):
        result = detector.detect("That's great! Yeah")
        assert "Yeah" not in result["corrected"]

    def test_trailing_word_in_context_not_removed(self, detector):
        """Trailing word that's part of a coherent sentence should be preserved."""
        result = detector.detect("Is that okay")
        # "okay" is part of the sentence — should not be removed
        assert "okay" in result["corrected"].lower()

    def test_single_word_not_treated_as_trailing(self, detector):
        """A single word should not be treated as trailing hallucination."""
        result = detector.detect("okay")
        # Single word — not enough context to judge trailing
        assert result["corrected"] == "okay"


# === Layer 4: Repetition detection ===

class TestRepetitionDetection:

    def test_pure_repetition(self, detector):
        """Same word repeated many times should be flagged as hallucination."""
        result = detector.detect("the the the the the")
        assert result["is_hallucination"] is True
        assert result["corrected"] == ""
        assert any(d["type"] == "pure_repetition" for d in result["detections"])

    def test_two_word_repetition(self, detector):
        """Even just two identical words count as pure repetition."""
        result = detector.detect("you you")
        assert any(d.get("is_pure_repetition") for d in result["detections"])

    def test_high_repetition_ratio(self, detector):
        """70%+ repetition of one word should be detected."""
        result = detector.detect("the the the the hello")
        detections = result["detections"]
        assert any(d["type"] == "high_repetition" for d in detections)

    def test_normal_text_no_repetition(self, detector):
        """Normal text should not trigger repetition detection."""
        result = detector.detect("I went to the store and bought some milk")
        repetition_detections = [d for d in result["detections"] if "repetition" in d.get("type", "")]
        assert len(repetition_detections) == 0


# === Edge cases ===

class TestEdgeCases:

    def test_empty_string(self, detector):
        result = detector.detect("")
        assert result["is_hallucination"] is False

    def test_whitespace_only(self, detector):
        result = detector.detect("   ")
        assert result["is_hallucination"] is False

    def test_result_structure(self, detector):
        """All results should have the standard keys."""
        result = detector.detect("hello world")
        assert "original" in result
        assert "corrected" in result
        assert "is_hallucination" in result
        assert "confidence" in result
        assert "detections" in result
        assert "changed" in result
