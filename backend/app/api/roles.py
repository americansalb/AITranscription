"""Role design API — LLM-driven conversational role creation."""

import logging
from typing import Optional

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from app.services.role_designer import design_role

logger = logging.getLogger(__name__)
router = APIRouter(tags=["roles"])


# ---------------------------------------------------------------------------
# Request / Response models
# ---------------------------------------------------------------------------

class ChatMessage(BaseModel):
    role: str = Field(..., description="'user' or 'assistant'")
    content: str = Field(..., description="Message content")


class RoleDesignRequest(BaseModel):
    messages: list[ChatMessage] = Field(..., description="Conversation history")
    project_context: dict = Field(
        default_factory=dict,
        description="Project context: {roles: {slug: {title, description, tags, permissions, max_instances}}}",
    )


class RoleConfigOutput(BaseModel):
    title: str
    slug: str
    description: str
    tags: list[str]
    permissions: list[str]
    max_instances: int
    briefing: str


class RoleDesignResponse(BaseModel):
    reply: str = Field(..., description="LLM's conversational reply")
    role_config: Optional[RoleConfigOutput] = Field(
        None, description="Generated role config (present when LLM has enough info)"
    )


# ---------------------------------------------------------------------------
# Endpoint
# ---------------------------------------------------------------------------

@router.post("/roles/design", response_model=RoleDesignResponse)
async def post_role_design(req: RoleDesignRequest):
    """Run one turn of the LLM role design conversation.

    Send the conversation history and project context. The LLM will either
    ask a follow-up question (role_config=null) or generate a complete role
    configuration (role_config populated).
    """
    if not req.messages:
        raise HTTPException(status_code=400, detail="At least one message is required")

    # Validate message format
    for msg in req.messages:
        if msg.role not in ("user", "assistant"):
            raise HTTPException(
                status_code=400,
                detail=f"Invalid message role '{msg.role}'. Must be 'user' or 'assistant'.",
            )

    try:
        result = await design_role(
            messages=[{"role": m.role, "content": m.content} for m in req.messages],
            project_context=req.project_context,
        )
        return result
    except RuntimeError as e:
        logger.error("Role design error: %s", e)
        raise HTTPException(status_code=502, detail="Role design service unavailable")
    except Exception as e:
        logger.exception("Unexpected error in role design")
        raise HTTPException(status_code=500, detail="Internal error")
