"""Rule-based corrector using user-defined patterns.

Handles:
- Custom dictionary words
- Regex-based replacements
- Common transcription error patterns
- Whisper hallucination detection
"""
import logging
import re
from typing import Optional

from sqlalchemy import select, update
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.learning import CorrectionRule

logger = logging.getLogger(__name__)


# Built-in patterns for common transcription errors
BUILTIN_PATTERNS = [
    # Common filler removal
    (r"\b(um|uh|er|ah|hmm)\b\s*", "", "filler"),
    # Repeated words (stuttering)
    (r"\b(\w+)(\s+\1)+\b", r"\1", "stutter"),
    # Double spaces
    (r"  +", " ", "spacing"),
]


# Known Whisper hallucination phrases (trained on YouTube content)
# These are phrases Whisper outputs when there's silence or unclear audio

# OBVIOUS hallucinations - these are YouTube-style phrases that nobody would
# actually dictate. Safe to remove even without audio duration info.
OBVIOUS_HALLUCINATIONS = {
    "thank you for watching",
    "thank you for watching.",
    "thanks for watching",
    "thanks for watching.",
    "thank you for listening",
    "thank you for listening.",
    "thanks for listening",
    "thanks for listening.",
    "see you next time",
    "see you next time.",
    "see you in the next video",
    "see you in the next one",
    "don't forget to subscribe",
    "please subscribe",
    "like and subscribe",
    "please like and subscribe",
    # Foreign language hallucinations
    "字幕",  # Chinese "subtitles"
    "자막",  # Korean "subtitles"
    "ご視聴ありがとうございました",  # Japanese "thank you for watching"
    "請訂閱",  # Chinese "please subscribe"
    "подписывайтесь",  # Russian "subscribe"
}

# These phrases are ONLY suspicious if audio is very short (< 0.5s)
# Because someone might actually say "thank you" or "okay" legitimately
SHORT_AUDIO_SUSPICIOUS = {
    "thank you",
    "thank you.",
    "thanks",
    "thanks.",
    "bye",
    "bye.",
    "goodbye",
    "goodbye.",
    "see you",
    "see you.",
    "okay",
    "okay.",
    "ok",
    "ok.",
    "yeah",
    "yeah.",
    "yes",
    "yes.",
    "no",
    "no.",
    "hmm",
    "hmm.",
    "...",
    "you",
    "you.",
    "the",
    "the.",
}

# Trailing phrases that are suspicious when they appear disconnected at the end
TRAILING_HALLUCINATIONS = [
    "okay",
    "ok",
    "yeah",
    "yes",
    "right",
    "so",
    "anyway",
    "alright",
    "bye",
    "thanks",
    "thank you",
]


class HallucinationDetector:
    """Detects and filters Whisper hallucinations with context-awareness.

    This is VERY conservative - it only removes text when we're almost certain
    it's a hallucination. We never want to remove something the user actually said.

    Auto-removes only:
    1. Obvious YouTube phrases ("thank you for watching", "please subscribe")
    2. Short audio (< 0.5s) with suspicious single words (needs duration info)
    3. Pure repetition ("the the the the")

    Does NOT auto-remove:
    - "thank you", "yeah", "okay" alone - user might have actually said these
    - Anything without audio duration evidence (unless it's an obvious YouTube phrase)
    """

    def __init__(self):
        self.obvious_hallucinations = OBVIOUS_HALLUCINATIONS
        self.short_audio_suspicious = SHORT_AUDIO_SUSPICIOUS
        self.trailing_phrases = TRAILING_HALLUCINATIONS

    def detect(
        self,
        text: str,
        audio_duration_seconds: Optional[float] = None,
    ) -> dict:
        """Detect hallucinations in transcribed text.

        Args:
            text: The transcribed text
            audio_duration_seconds: Duration of the audio (if known)

        Returns:
            Dict with detection results and corrected text
        """
        original = text
        cleaned = text.strip()
        cleaned_lower = cleaned.lower()

        detections = []
        is_hallucination = False
        corrected = text
        confidence = 0.0

        # === CHECK 1: Obvious YouTube-style hallucinations ===
        # These are phrases nobody would actually dictate
        if cleaned_lower in self.obvious_hallucinations:
            is_hallucination = True
            confidence = 0.98
            corrected = ""
            detections.append({
                "type": "obvious_hallucination",
                "phrase": cleaned,
                "reason": "YouTube-style phrase that wouldn't be dictated",
            })
            return self._result(original, corrected, is_hallucination, confidence, detections)

        # === CHECK 2: Very short audio with suspicious content ===
        # Only triggers if we HAVE audio duration AND it's very short
        # This protects real "thank you" or "okay" responses
        if audio_duration_seconds is not None and audio_duration_seconds < 0.5:
            if cleaned_lower in self.short_audio_suspicious:
                is_hallucination = True
                confidence = 0.92
                corrected = ""
                detections.append({
                    "type": "short_audio_hallucination",
                    "phrase": cleaned,
                    "duration": audio_duration_seconds,
                    "reason": f"Audio too short ({audio_duration_seconds:.2f}s) for this phrase",
                })
                return self._result(original, corrected, is_hallucination, confidence, detections)

        # === CHECK 3: Trailing disconnected hallucination ===
        # Check if text ends with a trailing hallucination that's grammatically disconnected
        corrected, trailing_detection = self._check_trailing_hallucination(cleaned)
        if trailing_detection:
            detections.append(trailing_detection)
            confidence = trailing_detection.get("confidence", 0.7)

        # === CHECK 4: Repeated single word/phrase ===
        # Whisper sometimes outputs the same word repeatedly on silence
        repetition_detection = self._check_repetition(cleaned)
        if repetition_detection:
            detections.append(repetition_detection)
            if repetition_detection.get("is_pure_repetition"):
                is_hallucination = True
                corrected = ""
                confidence = 0.9

        return self._result(original, corrected, is_hallucination, confidence, detections)

    def _check_trailing_hallucination(self, text: str) -> tuple[str, Optional[dict]]:
        """Check for trailing hallucinations that are grammatically disconnected.

        We only remove if:
        1. The word appears at the very end
        2. It's preceded by a sentence-ending punctuation OR
        3. It's preceded by a pause indicator (comma, dash) with no grammatical connection

        We do NOT remove if:
        - It's part of a coherent sentence ("I said okay to him")
        - It follows a question ("Is that okay")
        """
        text_stripped = text.strip()
        words = text_stripped.split()

        if len(words) < 2:
            return text, None

        last_word = words[-1].lower().rstrip(".,!?")

        if last_word not in self.trailing_phrases:
            return text, None

        # Get everything before the last word
        before_last = " ".join(words[:-1])

        # Check if it's grammatically disconnected
        # Disconnected if: ends with period, or ends with comma/dash before the trailing word

        # Pattern 1: "Some complete sentence. Okay"
        if re.search(r'[.!?]\s*$', before_last):
            # The sentence was complete, trailing word is disconnected
            corrected = before_last.strip()
            return corrected, {
                "type": "trailing_disconnected",
                "removed": words[-1],
                "reason": "Trailing word after sentence-ending punctuation",
                "confidence": 0.85,
            }

        # Pattern 2: "Blah blah, okay" or "Blah blah - yeah"
        # This is trickier - could be legitimate
        # Only flag if the trailing word is VERY common hallucination
        if last_word in {"okay", "ok", "yeah"} and re.search(r'[,\-–—]\s*$', before_last):
            # Check if the sentence structure suggests it's added
            # If the text before the comma is a complete thought, likely hallucination
            before_comma = re.sub(r'[,\-–—]\s*$', '', before_last).strip()

            # Heuristic: If there's a verb in the content and it makes sense without the trailing word
            # This is a softer check - we flag but with lower confidence
            return text, {
                "type": "trailing_suspicious",
                "word": last_word,
                "reason": "Possibly disconnected trailing word (review recommended)",
                "confidence": 0.5,  # Lower confidence - don't auto-remove
            }

        return text, None

    def _check_repetition(self, text: str) -> Optional[dict]:
        """Check for repeated words/phrases indicating hallucination.

        Whisper sometimes outputs: "the the the the" or "you you you" on silence
        """
        words = text.lower().split()

        if len(words) < 2:
            return None

        # Check if all words are the same
        unique_words = set(words)
        if len(unique_words) == 1:
            return {
                "type": "pure_repetition",
                "word": words[0],
                "count": len(words),
                "reason": f"Single word '{words[0]}' repeated {len(words)} times",
                "is_pure_repetition": True,
            }

        # Check for high repetition ratio
        if len(words) >= 4:
            word_counts = {}
            for w in words:
                word_counts[w] = word_counts.get(w, 0) + 1

            max_count = max(word_counts.values())
            if max_count >= len(words) * 0.7:  # 70%+ is the same word
                repeated_word = max(word_counts, key=word_counts.get)
                return {
                    "type": "high_repetition",
                    "word": repeated_word,
                    "ratio": max_count / len(words),
                    "reason": f"Word '{repeated_word}' appears {max_count}/{len(words)} times",
                    "is_pure_repetition": False,
                }

        return None

    def _result(
        self,
        original: str,
        corrected: str,
        is_hallucination: bool,
        confidence: float,
        detections: list,
    ) -> dict:
        """Format the detection result."""
        return {
            "original": original,
            "corrected": corrected,
            "is_hallucination": is_hallucination,
            "confidence": confidence,
            "detections": detections,
            "changed": original != corrected,
        }


class RuleBasedCorrector:
    """Applies rule-based corrections using user-defined and built-in patterns."""

    def __init__(self, db: AsyncSession, user_id: int):
        self.db = db
        self.user_id = user_id
        self._rules_cache: Optional[list[CorrectionRule]] = None

    async def get_user_rules(self, force_refresh: bool = False) -> list[CorrectionRule]:
        """Get user's correction rules from database (cached)."""
        if self._rules_cache is None or force_refresh:
            result = await self.db.execute(
                select(CorrectionRule)
                .where(CorrectionRule.user_id == self.user_id)
                .order_by(CorrectionRule.priority.desc())
            )
            self._rules_cache = list(result.scalars())

        return self._rules_cache

    async def add_rule(
        self,
        pattern: str,
        replacement: str,
        is_regex: bool = False,
        priority: int = 0,
    ) -> CorrectionRule:
        """Add a new correction rule."""
        # Validate regex if is_regex
        if is_regex:
            try:
                re.compile(pattern)
            except re.error as e:
                raise ValueError(f"Invalid regex pattern: {e}")

        rule = CorrectionRule(
            user_id=self.user_id,
            pattern=pattern,
            replacement=replacement,
            is_regex=is_regex,
            priority=priority,
        )
        self.db.add(rule)
        await self.db.commit()
        await self.db.refresh(rule)

        # Invalidate cache
        self._rules_cache = None

        logger.info(f"Added correction rule for user {self.user_id}: {pattern}")
        return rule

    async def delete_rule(self, rule_id: int) -> bool:
        """Delete a correction rule."""
        result = await self.db.execute(
            select(CorrectionRule).where(
                CorrectionRule.id == rule_id,
                CorrectionRule.user_id == self.user_id,
            )
        )
        rule = result.scalar_one_or_none()

        if rule:
            await self.db.delete(rule)
            await self.db.commit()
            self._rules_cache = None
            return True
        return False

    async def update_rule_hit_count(self, rule_id: int) -> None:
        """Increment the hit count for a rule."""
        await self.db.execute(
            update(CorrectionRule)
            .where(CorrectionRule.id == rule_id)
            .values(hit_count=CorrectionRule.hit_count + 1)
        )
        await self.db.commit()

    def apply_builtin_rules(self, text: str) -> tuple[str, list[dict]]:
        """Apply built-in correction patterns.

        Returns:
            Tuple of (corrected_text, list of applied corrections)
        """
        corrections = []
        result = text

        for pattern, replacement, rule_type in BUILTIN_PATTERNS:
            matches = list(re.finditer(pattern, result, re.IGNORECASE))
            if matches:
                new_result = re.sub(pattern, replacement, result, flags=re.IGNORECASE)
                if new_result != result:
                    corrections.append({
                        "type": "builtin",
                        "rule_type": rule_type,
                        "pattern": pattern,
                        "matches": len(matches),
                    })
                    result = new_result

        return result, corrections

    async def apply_user_rules(self, text: str) -> tuple[str, list[dict]]:
        """Apply user-defined correction rules.

        Returns:
            Tuple of (corrected_text, list of applied corrections)
        """
        rules = await self.get_user_rules()
        corrections = []
        result = text

        for rule in rules:
            try:
                if rule.is_regex:
                    # Regex replacement
                    new_result = re.sub(rule.pattern, rule.replacement, result)
                else:
                    # Simple string replacement (case-insensitive)
                    pattern = re.escape(rule.pattern)
                    new_result = re.sub(
                        pattern, rule.replacement, result, flags=re.IGNORECASE
                    )

                if new_result != result:
                    corrections.append({
                        "type": "user_rule",
                        "rule_id": rule.id,
                        "pattern": rule.pattern,
                        "replacement": rule.replacement,
                    })
                    # Update hit count asynchronously
                    await self.update_rule_hit_count(rule.id)
                    result = new_result

            except re.error as e:
                logger.warning(f"Invalid rule pattern {rule.id}: {e}")

        return result, corrections

    async def correct(self, text: str) -> dict:
        """Apply all rule-based corrections.

        Returns:
            Dict with corrected text and metadata
        """
        original = text

        # Apply built-in rules first
        result, builtin_corrections = self.apply_builtin_rules(text)

        # Apply user rules
        result, user_corrections = await self.apply_user_rules(result)

        all_corrections = builtin_corrections + user_corrections

        return {
            "original": original,
            "corrected": result,
            "changed": original != result,
            "corrections_applied": len(all_corrections),
            "corrections": all_corrections,
            "source": "rule_based",
            "confidence": 1.0 if all_corrections else 0.0,  # Rules are deterministic
        }

    async def get_rules_list(self) -> list[dict]:
        """Get list of user rules for display."""
        rules = await self.get_user_rules()
        return [
            {
                "id": r.id,
                "pattern": r.pattern,
                "replacement": r.replacement,
                "is_regex": r.is_regex,
                "priority": r.priority,
                "hit_count": r.hit_count,
            }
            for r in rules
        ]
