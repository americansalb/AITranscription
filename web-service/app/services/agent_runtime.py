"""Server-side agent runtime — manages agent loops for web-based collaboration.

Each active role in a project runs as an async task. The loop:
1. Poll the message board for new messages directed to this role
2. Build context from briefing + history + new messages
3. Call the provider proxy (LiteLLM) for a completion
4. Parse response and post any messages back to the board
5. Sleep and repeat

Context is persisted to the database, not held in memory.
On restart, agents reload context from the DB.
"""

import asyncio
import logging
from dataclasses import dataclass, field

logger = logging.getLogger(__name__)


@dataclass
class AgentState:
    """Runtime state for a single agent instance."""

    project_id: str
    role_slug: str
    instance: int = 0
    model: str = "claude-sonnet-4-6"
    is_running: bool = False
    last_seen_message_id: int = 0
    context_tokens: int = 0
    task: asyncio.Task | None = field(default=None, repr=False)


# Registry of active agents: key = "project_id:role_slug:instance"
_active_agents: dict[str, AgentState] = {}


def _agent_key(project_id: str, role_slug: str, instance: int = 0) -> str:
    return f"{project_id}:{role_slug}:{instance}"


async def start_agent(
    project_id: str,
    role_slug: str,
    instance: int = 0,
    model: str = "claude-sonnet-4-6",
    briefing: str = "",
    user_id: int = 0,
) -> AgentState:
    """Start an agent loop for a role in a project."""
    key = _agent_key(project_id, role_slug, instance)

    if key in _active_agents and _active_agents[key].is_running:
        raise ValueError(f"Agent {key} is already running")

    state = AgentState(
        project_id=project_id,
        role_slug=role_slug,
        instance=instance,
        model=model,
        is_running=True,
    )

    state.task = asyncio.create_task(
        _agent_loop(state, briefing, user_id),
        name=f"agent-{key}",
    )

    _active_agents[key] = state
    logger.info("Started agent %s with model %s", key, model)
    return state


async def stop_agent(project_id: str, role_slug: str, instance: int = 0) -> None:
    """Stop a running agent."""
    key = _agent_key(project_id, role_slug, instance)
    state = _active_agents.get(key)

    if not state or not state.is_running:
        raise ValueError(f"Agent {key} is not running")

    state.is_running = False
    if state.task:
        state.task.cancel()
    del _active_agents[key]
    logger.info("Stopped agent %s", key)


def get_active_agents(project_id: str | None = None) -> list[AgentState]:
    """List active agents, optionally filtered by project."""
    agents = list(_active_agents.values())
    if project_id:
        agents = [a for a in agents if a.project_id == project_id]
    return agents


async def _agent_loop(state: AgentState, briefing: str, user_id: int) -> None:
    """Core agent loop — runs until stopped or cancelled."""
    from app.config import settings

    logger.info("Agent loop started: %s:%s:%d", state.project_id, state.role_slug, state.instance)

    try:
        while state.is_running:
            # 1. Poll for new messages
            new_messages = await _poll_messages(state)

            if new_messages:
                # 2. Build context
                context = _build_context(briefing, state, new_messages)

                # 3. Call provider proxy
                try:
                    from app.services.provider_proxy import proxy_completion
                    from app.services.briefing_sanitizer import build_system_prompt

                    system = build_system_prompt(
                        role_slug=state.role_slug,
                        role_title=state.role_slug.replace("-", " ").title(),
                        instance=state.instance,
                        user_briefing=briefing,
                    )
                    result = await proxy_completion(
                        user_id=user_id,
                        model=state.model,
                        messages=context,
                        system=system,
                    )

                    # 4. Parse and post response
                    if result.content:
                        await _post_response(state, result.content)

                    state.context_tokens = result.input_tokens

                except Exception as e:
                    logger.error("Agent %s completion failed: %s", state.role_slug, e)

            # 5. Sleep
            await asyncio.sleep(settings.agent_poll_interval_seconds)

    except asyncio.CancelledError:
        logger.info("Agent %s cancelled", state.role_slug)
    except Exception as e:
        logger.error("Agent %s crashed: %s", state.role_slug, e)
    finally:
        state.is_running = False


async def _poll_messages(state: AgentState) -> list[dict]:
    """Poll the message board for new messages directed to this role."""
    # TODO: query DB for messages where to=state.role_slug or to='all'
    #       and id > state.last_seen_message_id
    return []


def _build_context(briefing: str, state: AgentState, new_messages: list[dict]) -> list[dict]:
    """Build the chat context for the LLM call.

    Uses sandboxed prompt construction:
    - System: immutable platform instructions + role identity
    - User briefing: treated as data, not instructions
    - Message history: recent board messages as conversation
    """
    messages = []

    # Add new board messages as user messages
    for msg in new_messages:
        messages.append({
            "role": "user",
            "content": f"[{msg.get('from', 'unknown')}] ({msg.get('type', 'message')}): {msg.get('subject', '')}\n\n{msg.get('body', '')}",
        })

    return messages


async def _post_response(state: AgentState, content: str) -> None:
    """Post the agent's response to the message board."""
    # TODO: parse content for message structure (to, type, subject, body)
    #       and insert into DB + broadcast via WebSocket
    logger.info("Agent %s:%d would post: %s...", state.role_slug, state.instance, content[:100])
