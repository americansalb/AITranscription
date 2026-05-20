"""Unit tests for rate limiter components: RateBucket, eviction, identity extraction.

The RateLimitMiddleware itself is bypassed in tests (VAAK_WEB_TESTING=1),
so we test the individual components directly.
"""

import time
from unittest.mock import MagicMock

import pytest


# --- RateBucket ---

class TestRateBucket:
    """Tests for the sliding window rate bucket."""

    def test_allow_under_limit(self):
        """Requests under the limit are allowed."""
        from app.middleware.rate_limiter import RateBucket

        bucket = RateBucket()
        for _ in range(5):
            assert bucket.allow(limit=10) is True

    def test_deny_at_limit(self):
        """Request at the limit is denied."""
        from app.middleware.rate_limiter import RateBucket

        bucket = RateBucket()
        for _ in range(10):
            assert bucket.allow(limit=10) is True

        assert bucket.allow(limit=10) is False

    def test_window_expiry(self):
        """Old timestamps expire and free up capacity."""
        from app.middleware.rate_limiter import RateBucket

        bucket = RateBucket()
        # Fill with timestamps that are 70 seconds old (outside 60s window)
        old_time = time.monotonic() - 70
        bucket.timestamps = [old_time] * 10

        # Should be allowed — old timestamps are outside window
        assert bucket.allow(limit=10, window_seconds=60.0) is True

    def test_window_boundary(self):
        """Timestamps right at the boundary are excluded."""
        from app.middleware.rate_limiter import RateBucket

        bucket = RateBucket()
        # 59 seconds ago — still inside 60s window
        recent = time.monotonic() - 59
        bucket.timestamps = [recent] * 10

        assert bucket.allow(limit=10, window_seconds=60.0) is False

    def test_custom_window_size(self):
        """Custom window size is respected."""
        from app.middleware.rate_limiter import RateBucket

        bucket = RateBucket()
        # 5 seconds ago
        recent = time.monotonic() - 5
        bucket.timestamps = [recent] * 3

        # 10-second window: still inside
        assert bucket.allow(limit=3, window_seconds=10.0) is False
        # 3-second window: timestamps are outside
        assert bucket.allow(limit=3, window_seconds=3.0) is True

    def test_is_stale_empty_bucket(self):
        """Empty bucket is stale."""
        from app.middleware.rate_limiter import RateBucket

        bucket = RateBucket()
        assert bucket.is_stale() is True

    def test_is_stale_old_activity(self):
        """Bucket with only old activity is stale."""
        from app.middleware.rate_limiter import RateBucket

        bucket = RateBucket()
        bucket.timestamps = [time.monotonic() - 400]  # 400s ago > 300s max_age
        assert bucket.is_stale(max_age_seconds=300.0) is True

    def test_is_stale_recent_activity(self):
        """Bucket with recent activity is not stale."""
        from app.middleware.rate_limiter import RateBucket

        bucket = RateBucket()
        bucket.timestamps = [time.monotonic() - 10]  # 10s ago
        assert bucket.is_stale(max_age_seconds=300.0) is False

    def test_timestamps_pruned_on_allow(self):
        """allow() prunes expired timestamps from the list."""
        from app.middleware.rate_limiter import RateBucket

        bucket = RateBucket()
        old = time.monotonic() - 120
        bucket.timestamps = [old] * 100
        assert len(bucket.timestamps) == 100

        bucket.allow(limit=10, window_seconds=60.0)
        # Old timestamps pruned, only the new one remains
        assert len(bucket.timestamps) == 1


# --- Eviction ---

class TestEviction:
    """Tests for stale bucket eviction."""

    def test_eviction_removes_stale_buckets(self):
        """Stale buckets are removed during eviction."""
        import app.middleware.rate_limiter as mod

        # Save and reset state
        old_buckets = dict(mod._buckets)
        old_last = mod._last_eviction
        mod._buckets.clear()
        mod._last_eviction = 0.0  # Force eviction to run

        try:
            # Add a stale bucket (empty = stale)
            mod._buckets["stale:1"] = mod.RateBucket()
            # Add a fresh bucket
            fresh = mod.RateBucket()
            fresh.timestamps = [time.monotonic()]
            mod._buckets["fresh:1"] = fresh

            mod._maybe_evict_stale_buckets()

            assert "stale:1" not in mod._buckets
            assert "fresh:1" in mod._buckets
        finally:
            # Restore original state
            mod._buckets.clear()
            mod._buckets.update(old_buckets)
            mod._last_eviction = old_last

    def test_eviction_respects_interval(self):
        """Eviction doesn't run if interval hasn't elapsed."""
        import app.middleware.rate_limiter as mod

        old_buckets = dict(mod._buckets)
        old_last = mod._last_eviction

        try:
            mod._buckets.clear()
            mod._last_eviction = time.monotonic()  # Just ran

            # Add stale bucket
            mod._buckets["stale:2"] = mod.RateBucket()

            mod._maybe_evict_stale_buckets()

            # Stale bucket should still exist (eviction skipped due to interval)
            assert "stale:2" in mod._buckets
        finally:
            mod._buckets.clear()
            mod._buckets.update(old_buckets)
            mod._last_eviction = old_last


# --- Identity Extraction ---

class TestExtractUserId:
    """Tests for _extract_user_id which determines rate limit key."""

    def test_authenticated_user(self, monkeypatch):
        """JWT Bearer token extracts user ID."""
        from app.middleware.rate_limiter import _extract_user_id

        # Mock the decode function to return a user ID
        monkeypatch.setattr(
            "app.middleware.rate_limiter.decode_access_token",
            lambda token: 42,
            raising=False,
        )

        request = MagicMock()
        request.headers = {"Authorization": "Bearer valid-token-here"}
        request.client.host = "192.168.1.1"

        # Need to patch the import inside the function
        import app.api.auth as auth_mod
        original = getattr(auth_mod, "decode_access_token", None)
        monkeypatch.setattr(auth_mod, "decode_access_token", lambda token: 42)

        result = _extract_user_id(request)
        assert result == "user:42"

    def test_unauthenticated_falls_back_to_ip(self):
        """No auth header falls back to client IP."""
        from app.middleware.rate_limiter import _extract_user_id

        request = MagicMock()
        request.headers = {}
        request.client.host = "10.0.0.1"

        result = _extract_user_id(request)
        assert result == "ip:10.0.0.1"

    def test_invalid_token_falls_back_to_ip(self, monkeypatch):
        """Invalid JWT falls back to client IP."""
        from app.middleware.rate_limiter import _extract_user_id
        import app.api.auth as auth_mod

        monkeypatch.setattr(auth_mod, "decode_access_token", lambda token: None)

        request = MagicMock()
        request.headers = {"Authorization": "Bearer bad-token"}
        request.client.host = "172.16.0.5"

        result = _extract_user_id(request)
        assert result == "ip:172.16.0.5"

    def test_no_client_ip(self):
        """Missing client info uses 'unknown'."""
        from app.middleware.rate_limiter import _extract_user_id

        request = MagicMock()
        request.headers = {}
        request.client = None

        result = _extract_user_id(request)
        assert result == "ip:unknown"

    def test_non_bearer_auth_ignored(self):
        """Non-Bearer auth header falls back to IP."""
        from app.middleware.rate_limiter import _extract_user_id

        request = MagicMock()
        request.headers = {"Authorization": "Basic dXNlcjpwYXNz"}
        request.client.host = "10.0.0.2"

        result = _extract_user_id(request)
        assert result == "ip:10.0.0.2"
