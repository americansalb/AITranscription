"""Shared Pydantic schemas for API request/response contracts."""

from shared.schemas.transcription import (
    TranscribeResponse,
    PolishRequest,
    PolishResponse,
    TranscribeAndPolishResponse,
)
from shared.schemas.common import ErrorResponse, HealthResponse
from shared.schemas.collab import (
    RoleConfig,
    ProjectConfig,
    BoardMessage,
    ProviderAssignment,
)

__all__ = [
    "TranscribeResponse",
    "PolishRequest",
    "PolishResponse",
    "TranscribeAndPolishResponse",
    "ErrorResponse",
    "HealthResponse",
    "RoleConfig",
    "ProjectConfig",
    "BoardMessage",
    "ProviderAssignment",
]
