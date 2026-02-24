"""BYOK key encryption — Fernet symmetric encryption for API keys at rest.

Uses the VAAK_WEB_FERNET_KEY environment variable. If not set,
encryption is a no-op (passthrough) — allows dev/test without encryption.

Generate a key: python -c "from cryptography.fernet import Fernet; print(Fernet.generate_key().decode())"
"""

import logging

from cryptography.fernet import Fernet, InvalidToken

from app.config import settings

logger = logging.getLogger(__name__)

_fernet: Fernet | None = None


def _get_fernet() -> Fernet | None:
    """Lazy-init Fernet instance from config."""
    global _fernet
    if _fernet is not None:
        return _fernet
    if settings.fernet_key:
        try:
            _fernet = Fernet(settings.fernet_key.encode())
            return _fernet
        except Exception as e:
            logger.error("Invalid VAAK_WEB_FERNET_KEY: %s", e)
    return None


def encrypt_key(plaintext: str) -> str:
    """Encrypt an API key for storage. Returns ciphertext string.

    If no Fernet key is configured, returns plaintext unchanged (dev mode).
    """
    if not plaintext:
        return plaintext
    f = _get_fernet()
    if f is None:
        return plaintext
    return f.encrypt(plaintext.encode()).decode()


def decrypt_key(ciphertext: str) -> str:
    """Decrypt an API key from storage. Returns plaintext string.

    If no Fernet key is configured, returns ciphertext unchanged (dev mode).
    Handles legacy plaintext keys gracefully — if decryption fails,
    assumes the value is already plaintext.
    """
    if not ciphertext:
        return ciphertext
    f = _get_fernet()
    if f is None:
        return ciphertext
    try:
        return f.decrypt(ciphertext.encode()).decode()
    except InvalidToken:
        # Legacy unencrypted key — return as-is
        logger.debug("Key appears to be unencrypted (legacy), returning as-is")
        return ciphertext
