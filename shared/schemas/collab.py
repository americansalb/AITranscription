"""Shared schemas for the collaboration system — used by both desktop and web backends."""

from enum import Enum
from pydantic import BaseModel, Field


class LLMProvider(str, Enum):
    """Supported LLM providers."""

    ANTHROPIC = "anthropic"
    OPENAI = "openai"
    GOOGLE = "google"


class ProviderAssignment(BaseModel):
    """Maps a role to its LLM provider and model."""

    provider: LLMProvider = LLMProvider.ANTHROPIC
    model: str = Field(default="claude-sonnet-4-6", description="Model ID for this role")


class MessageType(str, Enum):
    """Types of messages on the collaboration board."""

    DIRECTIVE = "directive"
    QUESTION = "question"
    ANSWER = "answer"
    STATUS = "status"
    HANDOFF = "handoff"
    REVIEW = "review"
    APPROVAL = "approval"
    REVISION = "revision"
    BROADCAST = "broadcast"
    VOTE = "vote"


class BoardMessage(BaseModel):
    """A single message on the collaboration board."""

    id: int = 0
    from_role: str = Field(alias="from", description="sender role:instance e.g. 'architect:0'")
    to: str = Field(description="target role slug or 'all'")
    type: MessageType
    subject: str
    body: str
    timestamp: str = ""
    metadata: dict = {}

    model_config = {"populate_by_name": True}


class RolePermission(str, Enum):
    """Permissions a role can have."""

    READ_CODE = "read_code"
    WRITE_CODE = "write_code"
    RUN_COMMANDS = "run_commands"
    REVIEW_CODE = "review_code"
    ASSIGN_TASKS = "assign_tasks"
    MANAGE_ROLES = "manage_roles"
    BROADCAST = "broadcast"
    WEB_SEARCH = "web_search"


class RoleConfig(BaseModel):
    """Configuration for a team role."""

    title: str
    description: str = ""
    tags: list[str] = []
    permissions: list[str] = []
    max_instances: int = Field(default=1, alias="maxInstances")
    provider: ProviderAssignment = ProviderAssignment()

    model_config = {"populate_by_name": True}


class ProjectConfig(BaseModel):
    """Configuration for a collaboration project."""

    name: str
    roles: dict[str, RoleConfig] = {}
    settings: dict = {}
