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
from datetime import datetime, timedelta, timezone

from sqlalchemy import or_, select, update

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


class UsageLimitExceeded(Exception):
    """Raised when agent completion would exceed the user's monthly token limit."""
    pass


async def _meter_agent_completion(
    user_id: int,
    project_id: int,
    model: str,
    messages: list[dict],
    system: str,
    max_tokens: int,
    timeout: int,
) -> "ProxyResult":
    """Metered agent completion — enforces the same billing rules as REST /completion.

    1. Load user from DB
    2. Lazy-reset monthly counters if in a new month
    3. Check usage against plan limits (raises UsageLimitExceeded if over)
    4. Look up BYOK key if applicable
    5. Call proxy_completion
    6. Record UsageRecord + atomically update user counters

    This ensures agent completions are fully visible to billing and subject to limits.
    """
    from app.config import settings
    from app.database import async_session
    from app.models import SubscriptionTier, UsageRecord, WebUser
    from app.services.provider_proxy import proxy_completion

    async with async_session() as db:
        # 1. Load user
        result = await db.execute(select(WebUser).where(WebUser.id == user_id))
        user = result.scalar_one_or_none()
        if not user:
            raise ValueError(f"User {user_id} not found")
        if not user.is_active:
            raise ValueError(f"User {user_id} is inactive")

        # 2. Lazy monthly reset (same logic as providers.py:_maybe_reset_monthly_usage)
        now = datetime.now(timezone.utc)
        if user.usage_reset_at is None or (
            user.usage_reset_at.year != now.year or user.usage_reset_at.month != now.month
        ):
            await db.execute(
                update(WebUser)
                .where(WebUser.id == user.id)
                .values(monthly_tokens_used=0, monthly_cost_usd=0.0, usage_reset_at=now)
            )
            await db.commit()
            await db.refresh(user)

        # 3. Check usage limits
        if user.tier == SubscriptionTier.FREE:
            monthly_limit = settings.free_tier_monthly_tokens
        elif user.tier == SubscriptionTier.PRO:
            monthly_limit = settings.pro_tier_monthly_tokens
        elif user.tier == SubscriptionTier.BYOK:
            monthly_limit = 999_999_999
        else:
            monthly_limit = settings.free_tier_monthly_tokens

        if user.monthly_tokens_used >= monthly_limit:
            raise UsageLimitExceeded(
                f"Monthly token limit ({monthly_limit:,}) reached for user {user_id}"
            )

        # 3b. Per-session budget (project cost in last 24h)
        from sqlalchemy import func as sa_func
        cutoff = now - timedelta(hours=24)
        session_result = await db.execute(
            select(sa_func.coalesce(sa_func.sum(UsageRecord.marked_up_cost_usd), 0.0))
            .where(
                UsageRecord.user_id == user_id,
                UsageRecord.project_id == project_id,
                UsageRecord.created_at >= cutoff,
            )
        )
        session_cost = float(session_result.scalar())
        if session_cost >= settings.max_cost_per_session:
            raise UsageLimitExceeded(
                f"Project session budget (${settings.max_cost_per_session:.2f}/day) "
                f"exceeded for project {project_id} (current: ${session_cost:.2f})"
            )

        # 4. BYOK key lookup
        byok_key = None
        if user.tier == SubscriptionTier.BYOK:
            if "claude" in model:
                byok_key = user.byok_anthropic_key
            elif "gpt" in model or model.startswith("o"):
                byok_key = user.byok_openai_key
            elif "gemini" in model:
                byok_key = user.byok_google_key

            if not byok_key:
                raise ValueError(
                    f"BYOK user {user_id} has no API key for model {model}"
                )

    # 5. Call proxy (outside DB session to avoid holding connection during LLM call)
    proxy_result = await asyncio.wait_for(
        proxy_completion(
            user_id=user_id,
            model=model,
            messages=messages,
            system=system,
            byok_key=byok_key,
            max_tokens=max_tokens,
        ),
        timeout=timeout,
    )

    # 6. Record usage (new DB session for the write)
    total_tokens = proxy_result.input_tokens + proxy_result.output_tokens

    async with async_session() as db:
        record = UsageRecord(
            user_id=user_id,
            project_id=project_id,
            model=proxy_result.model,
            provider=proxy_result.provider,
            input_tokens=proxy_result.input_tokens,
            output_tokens=proxy_result.output_tokens,
            raw_cost_usd=proxy_result.raw_cost_usd,
            marked_up_cost_usd=proxy_result.marked_up_cost_usd,
        )
        db.add(record)

        # Atomic counter update (same pattern as providers.py)
        await db.execute(
            update(WebUser)
            .where(WebUser.id == user_id)
            .values(
                monthly_tokens_used=WebUser.monthly_tokens_used + total_tokens,
                monthly_cost_usd=WebUser.monthly_cost_usd + proxy_result.marked_up_cost_usd,
            )
        )
        await db.commit()

    logger.info(
        "Agent metered: user=%d model=%s tokens=%d cost=$%.4f",
        user_id, model, total_tokens, proxy_result.marked_up_cost_usd,
    )

    return proxy_result


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

                # 3. Metered completion (R1: full billing, R3: timeout, R6: max_tokens)
                try:
                    from app.services.briefing_sanitizer import build_system_prompt

                    system = build_system_prompt(
                        role_slug=state.role_slug,
                        role_title=state.role_slug.replace("-", " ").title(),
                        instance=state.instance,
                        user_briefing=briefing,
                    )
                    result = await _meter_agent_completion(
                        user_id=user_id,
                        project_id=int(state.project_id),
                        model=state.model,
                        messages=context,
                        system=system,
                        max_tokens=settings.agent_max_response_tokens,
                        timeout=settings.agent_completion_timeout_seconds,
                    )

                    # 4. Parse and post response, then add to history (R2)
                    if result.content:
                        posted = await _post_response(state, result.content)
                        history.extend(posted)
                        history = history[-_MAX_HISTORY_MESSAGES:]

                    state.context_tokens = result.input_tokens

                except UsageLimitExceeded as e:
                    logger.warning("Agent %s stopped — %s", state.role_slug, e)
                    state.is_running = False
                    break
                except asyncio.TimeoutError:
                    logger.error("Agent %s completion timed out after %ds",
                                 state.role_slug, settings.agent_completion_timeout_seconds)
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

    Fetches the last N messages that are either:
    - Directed TO this role (incoming messages)
    - Sent FROM this agent (outgoing responses — needed for assistant turns in context)

    This ensures the agent has full conversational context on restart.
    Updates last_seen_message_id.
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
                or_(
                    # Incoming: messages directed to this role
                    Message.to_role.in_([state.role_slug, "all", agent_from]),
                    # Outgoing: this agent's own responses (for assistant turns)
                    Message.from_role == agent_from,
                ),
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


async def _post_response(state: AgentState, content: str) -> list[dict]:
    """Post the agent's response to the message board.

    Returns list of posted message dicts (for adding to in-memory history).

    Parses the LLM output for structured message directives. Supports:

    1. Single structured message with headers:
       TO: developer
       TYPE: directive
       SUBJECT: Implement feature X

       Body text here...

    2. Multi-message output separated by '===MSG===':
       TO: all
       TYPE: status
       SUBJECT: Work complete
       Body of first message
       ===MSG===
       TO: manager
       TYPE: handoff
       SUBJECT: Ready for review
       Body of second message

    3. Unstructured fallback: entire content posted as a broadcast to 'all'.
    """
    from app.database import async_session
    from app.models import Message
    from app.api.messages import manager as ws_manager

    project_id_int = int(state.project_id)
    agent_from = f"{state.role_slug}:{state.instance}"

    # R5: Use distinctive separator that won't collide with markdown hr
    segments = re.split(r"\n===MSG===\n", content.strip())
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

    posted: list[dict] = []

    # R7: Wrap all messages in a single transaction
    async with async_session() as db:
        db_messages = []
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
            db_messages.append(msg)

        # Single commit for all messages (R7: atomic multi-message post)
        await db.commit()

        # Refresh all to get IDs, then broadcast
        for msg in db_messages:
            await db.refresh(msg)

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
            await ws_manager.broadcast(project_id_int, response)
            posted.append(response)

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

    return posted


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
