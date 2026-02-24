"""LLM provider proxy — routes requests through LiteLLM with metering."""

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from shared.schemas.collab import LLMProvider

router = APIRouter()


class CompletionRequest(BaseModel):
    """Request to the provider proxy — used by the agent runtime."""

    project_id: str
    role_slug: str
    messages: list[dict] = Field(description="Chat messages in OpenAI format")
    tools: list[dict] | None = Field(default=None, description="Tool definitions")
    system: str | None = Field(default=None, description="System prompt")
    stream: bool = False


class CompletionResponse(BaseModel):
    """Response from the provider proxy."""

    content: str
    tool_calls: list[dict] = []
    usage: dict = Field(description="Token counts: input_tokens, output_tokens")
    cost_usd: float = Field(description="Estimated cost in USD at our markup rate")
    provider: str
    model: str


class UsageSummary(BaseModel):
    """Usage summary for a user."""

    total_tokens: int
    total_cost_usd: float
    monthly_limit_tokens: int
    remaining_tokens: int
    provider_breakdown: dict[str, dict] = {}


# --- Endpoints ---

@router.post("/completion", response_model=CompletionResponse)
async def proxy_completion(request: CompletionRequest):
    """Route an LLM completion through the metered proxy.

    1. Look up the role's assigned provider and model
    2. Check user's usage against plan limits
    3. Route to LiteLLM → provider API
    4. Meter tokens and record cost
    5. Return response
    """
    # TODO: implement with LiteLLM
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.get("/usage", response_model=UsageSummary)
async def get_usage():
    """Get current user's usage summary."""
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.get("/models")
async def list_available_models():
    """List all available models across configured providers."""
    return {
        "anthropic": [
            {"id": "claude-opus-4-6", "name": "Claude Opus 4.6", "context_window": 200000},
            {"id": "claude-sonnet-4-6", "name": "Claude Sonnet 4.6", "context_window": 200000},
            {"id": "claude-haiku-4-5-20251001", "name": "Claude Haiku 4.5", "context_window": 200000},
        ],
        "openai": [
            {"id": "gpt-4o", "name": "GPT-4o", "context_window": 128000},
            {"id": "gpt-4o-mini", "name": "GPT-4o Mini", "context_window": 128000},
        ],
        "google": [
            {"id": "gemini-2.0-flash", "name": "Gemini 2.0 Flash", "context_window": 1000000},
            {"id": "gemini-2.0-pro", "name": "Gemini 2.0 Pro", "context_window": 1000000},
        ],
    }
