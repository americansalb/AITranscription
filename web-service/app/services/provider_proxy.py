"""LLM Provider Proxy — metered routing through LiteLLM.

This is the core revenue-generating component. Every LLM API call goes through here.

Flow:
1. Check user's usage against plan limits (pre-flight)
2. Estimate cost and reject if over per-message ceiling (safety)
3. Route to LiteLLM with the correct provider API key
4. Meter tokens and record cost at markup rate
5. Return normalized response
"""

import logging
import time
from dataclasses import dataclass

from app.config import settings

logger = logging.getLogger(__name__)


@dataclass
class ProxyResult:
    """Result from a proxied LLM completion."""

    content: str
    tool_calls: list[dict]
    input_tokens: int
    output_tokens: int
    raw_cost_usd: float
    marked_up_cost_usd: float
    provider: str
    model: str
    latency_ms: float


# Approximate cost per 1M tokens (input/output) for common models
MODEL_COSTS = {
    # Anthropic
    "claude-opus-4-6": (15.0, 75.0),
    "claude-sonnet-4-6": (3.0, 15.0),
    "claude-haiku-4-5-20251001": (0.80, 4.0),
    # OpenAI
    "gpt-4o": (2.50, 10.0),
    "gpt-4o-mini": (0.15, 0.60),
    # Google
    "gemini-2.0-flash": (0.10, 0.40),
    "gemini-2.0-pro": (1.25, 5.0),
}


def estimate_cost(model: str, input_tokens: int, output_tokens: int) -> float:
    """Estimate raw API cost in USD."""
    costs = MODEL_COSTS.get(model, (5.0, 15.0))  # conservative default
    input_cost = (input_tokens / 1_000_000) * costs[0]
    output_cost = (output_tokens / 1_000_000) * costs[1]
    return input_cost + output_cost


async def proxy_completion(
    user_id: int,
    model: str,
    messages: list[dict],
    tools: list[dict] | None = None,
    system: str | None = None,
    stream: bool = False,
    byok_key: str | None = None,
) -> ProxyResult:
    """Route an LLM completion through the metered proxy.

    Args:
        user_id: User making the request (for usage tracking)
        model: Model ID (e.g., "claude-sonnet-4-6", "gpt-4o")
        messages: Chat messages in OpenAI format
        tools: Optional tool definitions
        system: Optional system prompt
        stream: Whether to stream the response
        byok_key: If provided, use this key instead of platform key
    """
    # 1. Pre-flight: estimate cost and check limits
    estimated_input_tokens = sum(len(m.get("content", "")) // 4 for m in messages)
    # Conservative output estimate: assume up to 4096 output tokens for pre-flight check
    estimated_output_tokens = 4096
    estimated_cost = estimate_cost(model, estimated_input_tokens, estimated_output_tokens)

    if estimated_cost > settings.max_cost_per_message:
        raise ValueError(
            f"Estimated cost ${estimated_cost:.2f} exceeds per-message limit "
            f"${settings.max_cost_per_message:.2f}"
        )

    # TODO: Check user's monthly usage against plan limits

    # 2. Determine which API key to use
    api_key = byok_key  # BYOK takes priority
    if not api_key:
        # Map model to provider API key. LiteLLM handles provider routing internally,
        # but we need to supply the correct API key.
        provider_keys = {
            "anthropic": settings.anthropic_api_key,
            "openai": settings.openai_api_key,
            "google": settings.google_ai_api_key,
        }
        try:
            import litellm
            provider = litellm.get_llm_provider(model)[1]  # returns (model, provider, ...)
            api_key = provider_keys.get(provider, "")
        except Exception:
            # Fallback: prefix matching for common models
            if "claude" in model:
                api_key = settings.anthropic_api_key
            elif "gpt" in model or model.startswith("o"):
                api_key = settings.openai_api_key
            elif "gemini" in model:
                api_key = settings.google_ai_api_key

    if not api_key:
        raise ValueError(f"No API key configured for model {model}")

    # 3. Call LiteLLM
    start = time.monotonic()

    try:
        import litellm

        kwargs = {
            "model": model,
            "messages": messages,
            "api_key": api_key,
            "stream": stream,
        }
        if tools:
            kwargs["tools"] = tools
        if system:
            # LiteLLM handles system prompt injection per provider
            kwargs["messages"] = [{"role": "system", "content": system}] + messages

        response = await litellm.acompletion(**kwargs)

    except ImportError:
        raise RuntimeError(
            "LiteLLM is not installed. Run: pip install litellm"
        )

    latency_ms = (time.monotonic() - start) * 1000

    # 4. Extract response data
    content = response.choices[0].message.content or ""
    tool_calls = []
    if response.choices[0].message.tool_calls:
        tool_calls = [
            {
                "id": tc.id,
                "type": tc.type,
                "function": {
                    "name": tc.function.name,
                    "arguments": tc.function.arguments,
                },
            }
            for tc in response.choices[0].message.tool_calls
        ]

    input_tokens = response.usage.prompt_tokens
    output_tokens = response.usage.completion_tokens

    # 5. Calculate costs
    raw_cost = estimate_cost(model, input_tokens, output_tokens)
    marked_up_cost = raw_cost * settings.markup_multiplier

    # TODO: Record usage in database (user_id, tokens, cost, model, timestamp)

    provider = "anthropic" if model.startswith("claude") else "openai" if model.startswith("gpt") else "google"

    logger.info(
        "Proxy completion: user=%d model=%s tokens=%d/%d cost=$%.4f (marked up $%.4f) latency=%dms",
        user_id, model, input_tokens, output_tokens, raw_cost, marked_up_cost, latency_ms,
    )

    return ProxyResult(
        content=content,
        tool_calls=tool_calls,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        raw_cost_usd=raw_cost,
        marked_up_cost_usd=marked_up_cost,
        provider=provider,
        model=model,
        latency_ms=latency_ms,
    )
