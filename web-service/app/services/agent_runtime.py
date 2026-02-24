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
import re
from dataclasses import dataclass, field

from sqlalchemy import select

logger = logging.getLogger(__name__)

# Max history messages to include in context (prevents unbounded token growth)
_MAX_HISTORY_MESSAGES = 50


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

    # On startup, load recent history so the agent has context
    history = await _load_history(state)

    try:
        while state.is_running:
            # 1. Poll for new messages
            new_messages = await _poll_messages(state)

            if new_messages:
                # Append to rolling history, trim to max window
                history.extend(new_messages)
                history = history[-_MAX_HISTORY_MESSAGES:]

                # 2. Build context from full history window
                context = _build_context(briefing, state, history)

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


async def _load_history(state: AgentState) -> list[dict]:
    """Load recent message history for an agent starting up.

    Fetches the last N messages directed to this role (or 'all') so the agent
    has context from before it was started. Updates last_seen_message_id.
    """
    from app.database import async_session
    from app.models import Message

    project_id_int = int(state.project_id)
    agent_from = f"{state.role_slug}:{state.instance}"

    async with async_session() as db:
        query = (
            select(Message)
            .where(
                Message.project_id == project_id_int,
                Message.to_role.in_([state.role_slug, "all", agent_from]),
            )
            .order_by(Message.id.desc())
            .limit(_MAX_HISTORY_MESSAGES)
        )
        result = await db.execute(query)
        messages = list(reversed(result.scalars().all()))  # chronological order

    if messages:
        state.last_seen_message_id = messages[-1].id
        logger.info(
            "Agent %s:%d loaded %d history messages (last_id=%d)",
            state.role_slug, state.instance, len(messages), state.last_seen_message_id,
        )

    return [
        {
            "id": m.id,
            "from": m.from_role,
            "to": m.to_role,
            "type": m.msg_type,
            "subject": m.subject,
            "body": m.body,
            "timestamp": m.created_at.isoformat() if m.created_at else "",
        }
        for m in messages
    ]


async def _poll_messages(state: AgentState) -> list[dict]:
    """Poll the message board for new messages directed to this role.

    Queries for messages where:
    - project_id matches
    - to_role is this agent's role slug, 'all', or a wildcard like 'developer:0'
    - id > last_seen_message_id (only unseen messages)

    Also excludes messages FROM this agent (don't echo-loop).
    Updates last_seen_message_id for next poll.
    """
    from app.database import async_session
    from app.models import Message

    project_id_int = int(state.project_id)
    agent_from = f"{state.role_slug}:{state.instance}"

    async with async_session() as db:
        query = (
            select(Message)
            .where(
                Message.project_id == project_id_int,
                Message.id > state.last_seen_message_id,
                Message.from_role != agent_from,  # don't read own messages
                Message.to_role.in_([state.role_slug, "all", agent_from]),
            )
            .order_by(Message.id.asc())
            .limit(100)  # safety cap per poll cycle
        )
        result = await db.execute(query)
        messages = result.scalars().all()

    if messages:
        state.last_seen_message_id = messages[-1].id
        logger.debug(
            "Agent %s:%d polled %d new messages (last_id=%d)",
            state.role_slug, state.instance, len(messages), state.last_seen_message_id,
        )

    return [
        {
            "id": m.id,
            "from": m.from_role,
            "to": m.to_role,
            "type": m.msg_type,
            "subject": m.subject,
            "body": m.body,
            "timestamp": m.created_at.isoformat() if m.created_at else "",
        }
        for m in messages
    ]


def _build_context(briefing: str, state: AgentState, new_messages: list[dict]) -> list[dict]:
    """Build the chat context for the LLM call.

    Uses sandboxed prompt construction:
    - System prompt (handled by caller via briefing_sanitizer)
    - Message history: recent board messages as conversation turns
    - Messages FROM this agent → assistant role; all others → user role

    This creates a natural chat flow where the agent's own prior messages
    appear as its previous responses, maintaining coherent dialogue.
    """
    agent_from = f"{state.role_slug}:{state.instance}"
    messages = []

    for msg in new_messages:
        sender = msg.get("from", "unknown")
        msg_type = msg.get("type", "message")
        subject = msg.get("subject", "")
        body = msg.get("body", "")

        # Format the header line with sender, type, and subject
        header = f"[{sender}] ({msg_type})"
        if subject:
            header += f": {subject}"

        content = f"{header}\n\n{body}" if body else header

        # Messages from this agent instance → assistant turns
        # Everything else → user turns
        if sender == agent_from:
            messages.append({"role": "assistant", "content": content})
        else:
            messages.append({"role": "user", "content": content})

    # LLM APIs require the conversation to start with a user message.
    # If the first message is an assistant turn (our own prior message),
    # prepend a synthetic context marker.
    if messages and messages[0]["role"] == "assistant":
        messages.insert(0, {
            "role": "user",
            "content": "[system] (context) You are resuming from your previous messages on the board.",
        })

    return messages


async def _post_response(state: AgentState, content: str) -> None:
    """Post the agent's response to the message board.

    Parses the LLM output for structured message directives. Supports:

    1. Single structured message with headers:
       TO: developer
       TYPE: directive
       SUBJECT: Implement feature X

       Body text here...

    2. Multi-message output separated by '---':
       TO: all
       TYPE: status
       SUBJECT: Work complete
       Body of first message
       ---
       TO: manager
       TYPE: handoff
       SUBJECT: Ready for review
       Body of second message

    3. Unstructured fallback: entire content posted as a broadcast to 'all'.
    """
    from app.database import async_session
    from app.models import Message
    from app.api.messages import manager

    project_id_int = int(state.project_id)
    agent_from = f"{state.role_slug}:{state.instance}"

    # Split on '---' line for multi-message support
    segments = re.split(r"\n---\n", content.strip())
    parsed_messages = []

    for segment in segments:
        segment = segment.strip()
        if not segment:
            continue
        parsed_messages.append(_parse_message_segment(segment, agent_from))

    # Fallback: if nothing parsed, send entire content as broadcast
    if not parsed_messages:
        parsed_messages = [{
            "to": "all",
            "type": "status",
            "subject": "",
            "body": content.strip(),
        }]

    async with async_session() as db:
        for msg_data in parsed_messages:
            msg = Message(
                project_id=project_id_int,
                from_role=agent_from,
                to_role=msg_data["to"],
                msg_type=msg_data["type"],
                subject=msg_data["subject"],
                body=msg_data["body"],
            )
            db.add(msg)
            await db.commit()
            await db.refresh(msg)

            # Broadcast via WebSocket
            response = {
                "id": msg.id,
                "from": msg.from_role,
                "to": msg.to_role,
                "type": msg.msg_type,
                "subject": msg.subject,
                "body": msg.body,
                "timestamp": msg.created_at.isoformat() if msg.created_at else "",
                "metadata": {},
            }
            await manager.broadcast(project_id_int, response)

            # Auto-trigger continuous review for status messages
            if msg.msg_type == "status":
                try:
                    from app.api.discussions import maybe_auto_trigger_continuous
                    await maybe_auto_trigger_continuous(
                        project_id=project_id_int,
                        from_role=msg.from_role,
                        msg_type=msg.msg_type,
                        message_id=msg.id,
                        subject=msg.subject,
                        db=db,
                    )
                except Exception as e:
                    logger.warning("Continuous auto-trigger failed: %s", e)

            logger.info(
                "Agent %s posted: to=%s type=%s subject=%s (%d chars)",
                agent_from, msg.to_role, msg.msg_type,
                msg.subject[:60] if msg.subject else "(none)",
                len(msg.body),
            )


# Regex for parsing message headers (TO:, TYPE:, SUBJECT:) at the start of a segment
_HEADER_RE = re.compile(
    r"^(?:TO:\s*(?P<to>\S+)\s*\n)?"
    r"(?:TYPE:\s*(?P<type>\S+)\s*\n)?"
    r"(?:SUBJECT:\s*(?P<subject>.+?)\s*\n)?",
    re.IGNORECASE,
)


def _parse_message_segment(segment: str, default_from: str) -> dict:
    """Parse a single message segment for TO/TYPE/SUBJECT headers.

    Returns a dict with keys: to, type, subject, body.
    Falls back to broadcast if no headers found.
    """
    match = _HEADER_RE.match(segment)

    to = "all"
    msg_type = "message"
    subject = ""
    body = segment

    if match and any(match.group(g) for g in ("to", "type", "subject")):
        to = match.group("to") or "all"
        msg_type = match.group("type") or "message"
        subject = match.group("subject") or ""
        # Body is everything after the matched headers
        body = segment[match.end():].strip()

    return {
        "to": to,
        "type": msg_type,
        "subject": subject,
        "body": body or segment,  # fallback: use full segment if body is empty
    }
