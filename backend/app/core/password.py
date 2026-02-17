"""Shared password validation logic."""

import re


def validate_password_strength(password: str) -> str:
    """Validate password meets strength requirements.

    Requirements: 8+ chars, at least one uppercase, one lowercase, one digit.
    Raises ValueError if any requirement is not met.
    Returns the password unchanged if valid.
    """
    if len(password) < 8:
        raise ValueError("Password must be at least 8 characters")
    if len(password) > 72:
        raise ValueError("Password must be at most 72 characters")
    if not re.search(r"[A-Z]", password):
        raise ValueError("Password must contain at least one uppercase letter")
    if not re.search(r"[a-z]", password):
        raise ValueError("Password must contain at least one lowercase letter")
    if not re.search(r"\d", password):
        raise ValueError("Password must contain at least one digit")
    return password
