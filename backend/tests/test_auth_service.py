"""Tests for the auth service — JWT token creation and decoding.

These tests use the pure Python functions (no database needed):
  - create_access_token
  - decode_access_token
  - hash_password / verify_password
"""
import pytest
from datetime import timedelta
from unittest.mock import patch

from app.services.auth import (
    create_access_token,
    decode_access_token,
    hash_password,
    verify_password,
)


# === JWT Token Tests ===

class TestJWTTokens:

    def test_create_and_decode_token(self):
        """Token created for user_id should decode back to that user_id."""
        token = create_access_token(user_id=42)
        decoded_id = decode_access_token(token)
        assert decoded_id == 42

    def test_decode_returns_int(self):
        """Decoded user_id should be an integer, not a string."""
        token = create_access_token(user_id=1)
        decoded_id = decode_access_token(token)
        assert isinstance(decoded_id, int)

    def test_custom_expiry(self):
        """Token with custom expiry should still decode correctly."""
        token = create_access_token(user_id=7, expires_delta=timedelta(hours=1))
        decoded_id = decode_access_token(token)
        assert decoded_id == 7

    def test_expired_token_returns_none(self):
        """Expired token should return None on decode."""
        token = create_access_token(user_id=1, expires_delta=timedelta(seconds=-10))
        decoded_id = decode_access_token(token)
        assert decoded_id is None

    def test_invalid_token_returns_none(self):
        """Garbage token should return None."""
        decoded_id = decode_access_token("not.a.real.token")
        assert decoded_id is None

    def test_empty_token_returns_none(self):
        decoded_id = decode_access_token("")
        assert decoded_id is None

    def test_different_users_different_tokens(self):
        """Tokens for different user IDs should be distinct."""
        token1 = create_access_token(user_id=1)
        token2 = create_access_token(user_id=2)
        assert token1 != token2
        assert decode_access_token(token1) == 1
        assert decode_access_token(token2) == 2


# === Password Hashing Tests ===

class TestPasswordHashing:

    def test_hash_and_verify(self):
        """Hashed password should verify against the original."""
        password = "my-secure-password"
        hashed = hash_password(password)
        assert verify_password(password, hashed) is True

    def test_wrong_password_fails(self):
        """Wrong password should fail verification."""
        hashed = hash_password("correct-password")
        assert verify_password("wrong-password", hashed) is False

    def test_hash_is_not_plaintext(self):
        """Hash should not be the same as the plaintext password."""
        password = "my-password"
        hashed = hash_password(password)
        assert hashed != password
        assert len(hashed) > 20  # bcrypt hashes are long

    def test_same_password_different_hashes(self):
        """Two hashes of the same password should be different (salt)."""
        password = "same-password"
        hash1 = hash_password(password)
        hash2 = hash_password(password)
        assert hash1 != hash2
        # Both should still verify
        assert verify_password(password, hash1) is True
        assert verify_password(password, hash2) is True
