"""Discussion system — Delphi, Oxford, Red Team, and Continuous Review.

Ports the desktop file-based discussion system (collab.rs + vaak-mcp.rs) to
async SQLAlchemy + FastAPI. Supports:
- Delphi: blind submissions, Fisher-Yates anonymized aggregation, manual rounds
- Oxford: team-based (for/against) aggregation
- Red Team: same as Delphi structurally
- Continuous: auto-triggered by status messages, keyword tally, silence=consent
"""

import logging
import random
import re
import uuid
from datetime import datetime, timezone

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.api.messages import manager as ws_manager
from app.database import get_db
from app.models import (
    Discussion,
    DiscussionMode,
    DiscussionPhase,
    DiscussionRound,
    DiscussionSubmission,
    Message,
    Project,
    WebUser,
)

logger = logging.getLogger(__name__)
router = APIRouter()


# ---------------------------------------------------------------------------
# Keyword classification for continuous review
# ---------------------------------------------------------------------------

_AGREE_RE = re.compile(
    r"\b(agree|lgtm|\+1|looks good|ship it|fine with|approve[sd]?|accepted|concur|sounds good|makes sense|no objection)\b",
    re.IGNORECASE,
)
_DISAGREE_RE = re.compile(
    r"\b(disagree|object|\-1|block|reject|nack|oppose|concerned|problem with|issue with)\b",
    re.IGNORECASE,
)
_ALTERNATIVE_RE = re.compile(
    r"\b(alternative|suggest|instead|how about|what if|counter.?proposal|rather)\b",
    re.IGNORECASE,
)


def _classify_response(body: str) -> str:
    """Classify a continuous-review response. Order: disagree > alternative > agree > default agree."""
    if _DISAGREE_RE.search(body):
        return "disagree"
    if _ALTERNATIVE_RE.search(body):
        return "alternative"
    if _AGREE_RE.search(body):
        return "agree"
    return "agree"  # Unclassified defaults to agree


# ---------------------------------------------------------------------------
# Schemas
# ---------------------------------------------------------------------------

class StartDiscussionRequest(BaseModel):
    mode: str = Field(description="delphi, oxford, red_team, or continuous")
    topic: str = Field(min_length=1, max_length=2000)
    participants: list[str] = Field(default_factory=list, description="role:instance IDs")
    max_rounds: int = Field(default=10, ge=1, le=999)
    timeout_minutes: int = Field(default=15, ge=1)
    auto_close_timeout_seconds: int = Field(default=60, ge=0, le=600)


class SubmissionRequest(BaseModel):
    body: str = Field(min_length=1, max_length=10000)


class SetTeamsRequest(BaseModel):
    teams: dict = Field(description='{"for": ["role:0"], "against": ["role:1"]}')


class SetTimeoutRequest(BaseModel):
    timeout_seconds: int = Field(ge=0, le=600)


class TrackSubmissionRequest(BaseModel):
    """Track an existing board message as a submission (used by agent runtime)."""
    from_role: str = Field(description="role:instance that submitted")
    message_id: int = Field(description="ID of the submission message on the board")


class RoundResponse(BaseModel):
    number: int
    topic: str | None
    auto_triggered: bool
    trigger_from: str | None
    opened_at: str
    closed_at: str | None
    submission_count: int
    aggregate: dict | None


class DiscussionResponse(BaseModel):
    id: int
    project_id: int
    mode: str
    topic: str
    is_active: bool
    phase: str
    moderator: str | None
    participants: list
    current_round: int
    max_rounds: int
    timeout_minutes: int
    auto_close_timeout_seconds: int
    teams: dict | None
    rounds: list[RoundResponse]
    started_at: str
    ended_at: str | None


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

async def _get_user_project(db: AsyncSession, project_id: int, user_id: int) -> Project:
    result = await db.execute(
        select(Project).where(Project.id == project_id, Project.owner_id == user_id)
    )
    project = result.scalar_one_or_none()
    if not project:
        raise HTTPException(status_code=404, detail="Project not found")
    return project


async def _get_discussion(
    db: AsyncSession, project_id: int, discussion_id: int, user_id: int,
    *, require_active: bool = True,
) -> Discussion:
    await _get_user_project(db, project_id, user_id)
    result = await db.execute(
        select(Discussion).where(
            Discussion.id == discussion_id,
            Discussion.project_id == project_id,
        )
    )
    discussion = result.scalar_one_or_none()
    if not discussion:
        raise HTTPException(status_code=404, detail="Discussion not found")
    if require_active and not discussion.is_active:
        raise HTTPException(status_code=409, detail="Discussion is no longer active")
    return discussion


async def _get_current_round(db: AsyncSession, discussion: Discussion) -> DiscussionRound:
    result = await db.execute(
        select(DiscussionRound).where(
            DiscussionRound.discussion_id == discussion.id,
            DiscussionRound.number == discussion.current_round,
        )
    )
    rnd = result.scalar_one_or_none()
    if not rnd:
        raise HTTPException(status_code=409, detail="No open round found")
    return rnd


async def _post_system_message(
    db: AsyncSession,
    project_id: int,
    subject: str,
    body: str,
) -> Message:
    """Post a system moderation message and broadcast via WebSocket."""
    msg = Message(
        project_id=project_id,
        from_role="system",
        to_role="all",
        msg_type="moderation",
        subject=subject,
        body=body,
    )
    db.add(msg)
    await db.flush()
    await db.refresh(msg)

    await ws_manager.broadcast(project_id, {
        "id": msg.id,
        "project_id": msg.project_id,
        "from_role": msg.from_role,
        "to_role": msg.to_role,
        "msg_type": msg.msg_type,
        "subject": msg.subject,
        "body": msg.body,
        "created_at": msg.created_at.isoformat(),
    })
    return msg


def _discussion_response(discussion: Discussion) -> DiscussionResponse:
    return DiscussionResponse(
        id=discussion.id,
        project_id=discussion.project_id,
        mode=discussion.mode.value,
        topic=discussion.topic,
        is_active=discussion.is_active,
        phase=discussion.phase.value,
        moderator=discussion.moderator,
        participants=discussion.participants or [],
        current_round=discussion.current_round,
        max_rounds=discussion.max_rounds,
        timeout_minutes=discussion.timeout_minutes,
        auto_close_timeout_seconds=discussion.auto_close_timeout_seconds,
        teams=discussion.teams,
        rounds=[
            RoundResponse(
                number=r.number,
                topic=r.topic,
                auto_triggered=r.auto_triggered,
                trigger_from=r.trigger_from,
                opened_at=r.opened_at.isoformat(),
                closed_at=r.closed_at.isoformat() if r.closed_at else None,
                submission_count=len(r.submissions),
                aggregate=r.aggregate,
            )
            for r in discussion.rounds
        ],
        started_at=discussion.started_at.isoformat(),
        ended_at=discussion.ended_at.isoformat() if discussion.ended_at else None,
    )


# ---------------------------------------------------------------------------
# Aggregate generation
# ---------------------------------------------------------------------------

async def _generate_delphi_aggregate(
    db: AsyncSession, rnd: DiscussionRound, topic: str, mode_label: str,
) -> tuple[str, dict]:
    """Anonymized aggregate with Fisher-Yates shuffle (Delphi / Red Team)."""
    entries = []
    for sub in rnd.submissions:
        result = await db.execute(select(Message).where(Message.id == sub.message_id))
        msg = result.scalar_one_or_none()
        if msg:
            entries.append({"body": msg.body, "submitted_at": sub.submitted_at.isoformat()})

    # Fisher-Yates shuffle with random UUID seed
    seed = uuid.uuid4().int
    rng = random.Random(seed)
    for i in range(len(entries) - 1, 0, -1):
        j = rng.randint(0, i)
        entries[i], entries[j] = entries[j], entries[i]

    # Build markdown body
    lines = [
        f"## {mode_label.title()} Round {rnd.number} Aggregate — {len(entries)} submissions",
        f"**Topic**: {topic}",
        "",
        "*Order randomized. Identities anonymized.*",
        "",
        "---",
    ]
    numbered = []
    for idx, entry in enumerate(entries, 1):
        lines.extend(["", f"### Participant {idx}", entry["body"]])
        numbered.append({"participant_number": idx, "body": entry["body"]})

    aggregate_json = {
        "type": "anonymized",
        "mode": mode_label,
        "round": rnd.number,
        "submission_count": len(entries),
        "entries": numbered,
        "shuffle_seed": str(seed),
    }
    return "\n".join(lines), aggregate_json


async def _generate_oxford_aggregate(
    db: AsyncSession, rnd: DiscussionRound, topic: str, teams: dict | None,
) -> tuple[str, dict]:
    """Team-grouped aggregate for Oxford debates."""
    for_team = set(teams.get("for", [])) if teams else set()
    against_team = set(teams.get("against", [])) if teams else set()

    for_subs: list[str] = []
    against_subs: list[str] = []
    unassigned_subs: list[str] = []

    for sub in rnd.submissions:
        result = await db.execute(select(Message).where(Message.id == sub.message_id))
        msg = result.scalar_one_or_none()
        if not msg:
            continue
        if sub.from_role in for_team:
            for_subs.append(msg.body)
        elif sub.from_role in against_team:
            against_subs.append(msg.body)
        else:
            unassigned_subs.append(msg.body)

    total = len(for_subs) + len(against_subs) + len(unassigned_subs)
    lines = [
        f"## Oxford Round {rnd.number} Aggregate — {total} submissions",
        f"**Topic**: {topic}",
        "",
    ]
    if for_subs:
        lines.append("### FOR the proposition")
        lines.append("")
        for idx, body in enumerate(for_subs, 1):
            lines.extend([f"**Argument {idx}:** {body}", ""])
    if against_subs:
        lines.append("### AGAINST the proposition")
        lines.append("")
        for idx, body in enumerate(against_subs, 1):
            lines.extend([f"**Argument {idx}:** {body}", ""])
    if unassigned_subs:
        lines.append("### Unassigned")
        lines.append("")
        for idx, body in enumerate(unassigned_subs, 1):
            lines.extend([f"**Submission {idx}:** {body}", ""])

    aggregate_json = {
        "type": "oxford_teams",
        "round": rnd.number,
        "for_count": len(for_subs),
        "against_count": len(against_subs),
        "unassigned_count": len(unassigned_subs),
    }
    return "\n".join(lines), aggregate_json


async def _generate_continuous_aggregate(
    db: AsyncSession, rnd: DiscussionRound, discussion: Discussion,
) -> tuple[str, dict]:
    """Lightweight tally for continuous review. Silence = consent."""
    participants = discussion.participants or []
    author = rnd.trigger_from
    non_author = [p for p in participants if p != author]

    submitted_roles: set[str] = set()
    agree_count = 0
    disagree_reasons: list[str] = []
    alternative_proposals: list[str] = []

    for sub in rnd.submissions:
        submitted_roles.add(sub.from_role)
        result = await db.execute(select(Message).where(Message.id == sub.message_id))
        msg = result.scalar_one_or_none()
        if not msg:
            agree_count += 1
            continue
        classification = _classify_response(msg.body)
        if classification == "agree":
            agree_count += 1
        elif classification == "disagree":
            disagree_reasons.append(msg.body[:200])
        elif classification == "alternative":
            alternative_proposals.append(msg.body[:200])

    silent = [p for p in non_author if p not in submitted_roles]
    agree_count += len(silent)  # Silence = consent

    disagree_count = len(disagree_reasons)
    alt_count = len(alternative_proposals)
    verdict = "APPROVED" if disagree_count == 0 else "DISPUTED"

    lines = [f"## Continuous Review Round {rnd.number} — {verdict}"]
    if rnd.topic:
        lines.append(f"**Topic**: {rnd.topic}")
    lines.append(f"**Triggered by**: {author or 'unknown'}")
    lines.append("")
    lines.append(
        f"**{agree_count}** agree ({len(silent)} silent = approve) | "
        f"**{disagree_count}** disagree | **{alt_count}** alternatives"
    )
    if disagree_reasons:
        lines.extend(["", "**Disagreements:**"])
        for reason in disagree_reasons:
            lines.append(f"- {reason}")
    if alternative_proposals:
        lines.extend(["", "**Alternatives:**"])
        for alt in alternative_proposals:
            lines.append(f"- {alt}")

    aggregate_json = {
        "type": "continuous_tally",
        "round": rnd.number,
        "verdict": verdict,
        "agree": agree_count,
        "disagree": disagree_count,
        "alternatives": alt_count,
        "silent_count": len(silent),
        "total": agree_count + disagree_count + alt_count,
    }
    return "\n".join(lines), aggregate_json


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------

@router.post("/{project_id}/discussions", status_code=201)
async def start_discussion(
    project_id: int,
    request: StartDiscussionRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Start a new structured discussion in a project."""
    await _get_user_project(db, project_id, user.id)

    # Only one active discussion per project
    existing = await db.execute(
        select(Discussion).where(
            Discussion.project_id == project_id,
            Discussion.is_active == True,  # noqa: E712
        )
    )
    if existing.scalar_one_or_none():
        raise HTTPException(status_code=409, detail="A discussion is already active in this project")

    try:
        mode = DiscussionMode(request.mode)
    except ValueError:
        raise HTTPException(status_code=400, detail=f"Invalid mode: {request.mode}")

    is_continuous = mode == DiscussionMode.CONTINUOUS
    is_delphi = mode == DiscussionMode.DELPHI

    # Phase logic matches desktop: continuous=REVIEWING, delphi=PREPARING, others=SUBMITTING
    if is_continuous:
        phase = DiscussionPhase.REVIEWING
        initial_round = 0
    elif is_delphi:
        phase = DiscussionPhase.PREPARING
        initial_round = 0
    else:
        phase = DiscussionPhase.SUBMITTING
        initial_round = 1

    discussion = Discussion(
        project_id=project_id,
        mode=mode,
        topic=request.topic,
        phase=phase,
        moderator=f"human:{user.id}",
        participants=request.participants,
        current_round=initial_round,
        max_rounds=999 if is_continuous else request.max_rounds,
        timeout_minutes=request.timeout_minutes,
        auto_close_timeout_seconds=request.auto_close_timeout_seconds if is_continuous else 0,
    )
    db.add(discussion)
    await db.flush()

    # Create first round for modes that start with one open
    if initial_round == 1:
        db.add(DiscussionRound(
            discussion_id=discussion.id,
            number=1,
            topic=request.topic,
        ))

    # Announce
    if is_continuous:
        body = (
            f"Continuous review started: **{request.topic}**\n\n"
            "Review windows open automatically when developers post status updates. "
            "Respond with `agree` / `disagree: [reason]` / `alternative: [proposal]`.\n"
            f"Auto-close timeout: {discussion.auto_close_timeout_seconds}s. Silence = consent."
        )
    else:
        body = (
            f"{mode.value.replace('_', ' ').title()} discussion started: **{request.topic}**\n\n"
            f"Participants: {', '.join(request.participants) or 'all'}\n"
            + (f"Round 1 is now open for submissions." if initial_round == 1
               else "Moderator will open Round 1 when ready.")
        )

    await _post_system_message(db, project_id, f"Discussion started: {request.topic[:100]}", body)

    await db.commit()
    await db.refresh(discussion)

    logger.info("Discussion started: id=%d mode=%s project=%d", discussion.id, mode.value, project_id)
    return _discussion_response(discussion)


@router.get("/{project_id}/discussions/active", response_model=DiscussionResponse | None)
async def get_active_discussion(
    project_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Get the currently active discussion for a project, if any."""
    await _get_user_project(db, project_id, user.id)
    result = await db.execute(
        select(Discussion).where(
            Discussion.project_id == project_id,
            Discussion.is_active == True,  # noqa: E712
        )
    )
    discussion = result.scalar_one_or_none()
    if not discussion:
        return None
    return _discussion_response(discussion)


@router.get("/{project_id}/discussions/{discussion_id}", response_model=DiscussionResponse)
async def get_discussion_by_id(
    project_id: int,
    discussion_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Get a specific discussion, including ended ones."""
    disc = await _get_discussion(db, project_id, discussion_id, user.id, require_active=False)
    return _discussion_response(disc)


@router.post("/{project_id}/discussions/{discussion_id}/open-round")
async def open_next_round(
    project_id: int,
    discussion_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Open the next round of a discussion (moderator action)."""
    discussion = await _get_discussion(db, project_id, discussion_id, user.id)

    if discussion.phase not in (DiscussionPhase.REVIEWING, DiscussionPhase.PREPARING):
        raise HTTPException(status_code=409, detail=f"Cannot open round in phase '{discussion.phase.value}'")
    if discussion.current_round >= discussion.max_rounds:
        raise HTTPException(status_code=409, detail="Maximum rounds reached")

    discussion.current_round += 1
    discussion.phase = DiscussionPhase.SUBMITTING

    db.add(DiscussionRound(
        discussion_id=discussion.id,
        number=discussion.current_round,
        topic=discussion.topic,
    ))

    await _post_system_message(
        db, project_id,
        f"Round {discussion.current_round} opened",
        f"Round {discussion.current_round} is now open for submissions.\n**Topic**: {discussion.topic[:200]}",
    )

    await db.commit()
    return {"round": discussion.current_round, "phase": "submitting"}


@router.post("/{project_id}/discussions/{discussion_id}/submit")
async def submit_to_round(
    project_id: int,
    discussion_id: int,
    request: SubmissionRequest,
    role_slug: str = "human",
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Submit a response to the current discussion round (human user)."""
    discussion = await _get_discussion(db, project_id, discussion_id, user.id)

    if discussion.phase != DiscussionPhase.SUBMITTING:
        raise HTTPException(status_code=409, detail="Not accepting submissions right now")

    current_round = await _get_current_round(db, discussion)

    from_label = f"{role_slug}:{user.id}"

    # Check for duplicate submission
    for existing in current_round.submissions:
        if existing.from_role == from_label:
            raise HTTPException(status_code=409, detail="Already submitted for this round")

    # Post as board message
    msg = Message(
        project_id=project_id,
        from_role=from_label,
        to_role="discussion",
        msg_type="submission",
        subject=f"Round {discussion.current_round} submission",
        body=request.body,
    )
    db.add(msg)
    await db.flush()

    db.add(DiscussionSubmission(
        round_id=current_round.id,
        from_role=from_label,
        message_id=msg.id,
    ))
    await db.commit()

    return {"status": "submitted", "round": discussion.current_round}


@router.post("/{project_id}/discussions/{discussion_id}/track-submission")
async def track_submission(
    project_id: int,
    discussion_id: int,
    request: TrackSubmissionRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Track an existing board message as a discussion submission (agent runtime use)."""
    discussion = await _get_discussion(db, project_id, discussion_id, user.id)

    if discussion.phase != DiscussionPhase.SUBMITTING:
        raise HTTPException(status_code=409, detail="Not accepting submissions right now")

    current_round = await _get_current_round(db, discussion)

    # Duplicate check
    for existing in current_round.submissions:
        if existing.from_role == request.from_role:
            raise HTTPException(status_code=409, detail="Already submitted for this round")

    # Verify message exists
    msg_result = await db.execute(select(Message).where(Message.id == request.message_id))
    if not msg_result.scalar_one_or_none():
        raise HTTPException(status_code=404, detail="Message not found")

    db.add(DiscussionSubmission(
        round_id=current_round.id,
        from_role=request.from_role,
        message_id=request.message_id,
    ))
    await db.flush()

    # Check quorum for continuous mode
    quorum_reached = False
    if discussion.mode == DiscussionMode.CONTINUOUS:
        participants = discussion.participants or []
        non_author = [p for p in participants if p != current_round.trigger_from]
        submitted_roles = {s.from_role for s in current_round.submissions}
        submitted_roles.add(request.from_role)
        if non_author and all(p in submitted_roles for p in non_author):
            quorum_reached = True

    await db.commit()

    return {
        "status": "submitted",
        "round": discussion.current_round,
        "from_role": request.from_role,
        "quorum_reached": quorum_reached,
    }


@router.post("/{project_id}/discussions/{discussion_id}/close-round")
async def close_round(
    project_id: int,
    discussion_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Close the current round and generate aggregate."""
    discussion = await _get_discussion(db, project_id, discussion_id, user.id)

    if discussion.phase != DiscussionPhase.SUBMITTING:
        raise HTTPException(status_code=409, detail="No round to close")

    current_round = await _get_current_round(db, discussion)
    current_round.closed_at = datetime.now(timezone.utc)

    # Generate aggregate based on mode
    if discussion.mode == DiscussionMode.CONTINUOUS:
        body, aggregate = await _generate_continuous_aggregate(db, current_round, discussion)
    elif discussion.mode == DiscussionMode.OXFORD:
        body, aggregate = await _generate_oxford_aggregate(db, current_round, discussion.topic, discussion.teams)
    else:  # Delphi / Red Team
        body, aggregate = await _generate_delphi_aggregate(db, current_round, discussion.topic, discussion.mode.value)

    current_round.aggregate = aggregate

    agg_msg = await _post_system_message(
        db, project_id,
        f"Round {discussion.current_round} Aggregate",
        body,
    )
    current_round.aggregate_message_id = agg_msg.id
    discussion.phase = DiscussionPhase.REVIEWING

    await db.commit()

    return {"round": discussion.current_round, "aggregate": aggregate, "phase": "reviewing"}


@router.post("/{project_id}/discussions/{discussion_id}/end")
async def end_discussion(
    project_id: int,
    discussion_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """End an active discussion."""
    discussion = await _get_discussion(db, project_id, discussion_id, user.id)

    discussion.is_active = False
    discussion.phase = DiscussionPhase.COMPLETE
    discussion.ended_at = datetime.now(timezone.utc)

    await _post_system_message(
        db, project_id,
        f"Discussion ended: {discussion.topic[:100]}",
        f"The {discussion.mode.value.replace('_', ' ')} discussion has been concluded "
        f"after {discussion.current_round} rounds.",
    )

    await db.commit()
    return {"status": "ended", "rounds_completed": discussion.current_round}


@router.post("/{project_id}/discussions/{discussion_id}/teams")
async def set_teams(
    project_id: int,
    discussion_id: int,
    request: SetTeamsRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Set teams for Oxford-style debate."""
    discussion = await _get_discussion(db, project_id, discussion_id, user.id)

    if discussion.mode != DiscussionMode.OXFORD:
        raise HTTPException(status_code=400, detail="Teams only apply to Oxford mode")
    if "for" not in request.teams or "against" not in request.teams:
        raise HTTPException(status_code=400, detail="Teams must have 'for' and 'against' keys")

    discussion.teams = request.teams
    await db.commit()
    return {"teams": discussion.teams}


@router.post("/{project_id}/discussions/{discussion_id}/set-timeout")
async def set_timeout(
    project_id: int,
    discussion_id: int,
    request: SetTimeoutRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Set auto-close timeout for continuous review."""
    discussion = await _get_discussion(db, project_id, discussion_id, user.id)

    if discussion.mode != DiscussionMode.CONTINUOUS:
        raise HTTPException(status_code=400, detail="Timeout only applies to continuous mode")

    discussion.auto_close_timeout_seconds = request.timeout_seconds
    await db.commit()
    return {"auto_close_timeout_seconds": discussion.auto_close_timeout_seconds}


# ---------------------------------------------------------------------------
# Continuous review auto-trigger (called from message send pipeline)
# ---------------------------------------------------------------------------

async def maybe_auto_trigger_continuous(
    project_id: int,
    from_role: str,
    msg_type: str,
    message_id: int,
    subject: str,
    db: AsyncSession,
) -> bool:
    """Auto-trigger a continuous review round when a status message is posted.

    Called from the message send pipeline (messages.py).
    Returns True if a new round was opened.
    """
    if msg_type != "status":
        return False

    result = await db.execute(
        select(Discussion).where(
            Discussion.project_id == project_id,
            Discussion.is_active == True,  # noqa: E712
            Discussion.mode == DiscussionMode.CONTINUOUS,
        )
    )
    disc = result.scalar_one_or_none()
    if not disc:
        return False

    # Only auto-trigger when in REVIEWING phase (waiting for status triggers)
    if disc.phase != DiscussionPhase.REVIEWING:
        return False

    # Don't auto-trigger if this author already has an open (unclosed) round
    for r in disc.rounds:
        if r.trigger_from == from_role and r.closed_at is None:
            return False

    # Create auto-triggered round
    next_number = disc.current_round + 1
    new_round = DiscussionRound(
        discussion_id=disc.id,
        number=next_number,
        topic=subject or None,
        auto_triggered=True,
        trigger_from=from_role,
        trigger_message_id=message_id,
        opened_at=datetime.now(timezone.utc),
    )
    db.add(new_round)

    disc.current_round = next_number
    disc.phase = DiscussionPhase.SUBMITTING

    await _post_system_message(
        db, project_id,
        f"Review round {next_number} (auto-triggered)",
        f"Auto-triggered by status from **{from_role}**: {subject}\n\n"
        f"Respond with `agree` / `disagree: [reason]` / `alternative: [proposal]`.\n"
        f"Timeout: {disc.auto_close_timeout_seconds}s. Silence = consent.",
    )

    await db.flush()
    logger.info("Auto-triggered continuous round %d for project %d", next_number, project_id)
    return True


async def maybe_auto_close_continuous(project_id: int, db: AsyncSession) -> bool:
    """Auto-close a continuous review round if timeout has elapsed.

    Called periodically or after new submissions.
    Returns True if a round was closed.
    """
    result = await db.execute(
        select(Discussion).where(
            Discussion.project_id == project_id,
            Discussion.is_active == True,  # noqa: E712
            Discussion.mode == DiscussionMode.CONTINUOUS,
            Discussion.phase == DiscussionPhase.SUBMITTING,
        )
    )
    disc = result.scalar_one_or_none()
    if not disc or disc.auto_close_timeout_seconds <= 0:
        return False

    # Find current open round
    current_round = None
    for r in disc.rounds:
        if r.number == disc.current_round and r.closed_at is None:
            current_round = r
            break
    if not current_round:
        return False

    elapsed = (datetime.now(timezone.utc) - current_round.opened_at).total_seconds()
    if elapsed < disc.auto_close_timeout_seconds:
        return False

    # Timeout elapsed — auto-close with tally
    body, aggregate = await _generate_continuous_aggregate(db, current_round, disc)

    agg_msg = await _post_system_message(
        db, project_id,
        f"Round {current_round.number} auto-closed (timeout)",
        body,
    )

    current_round.closed_at = datetime.now(timezone.utc)
    current_round.aggregate = aggregate
    current_round.aggregate_message_id = agg_msg.id
    disc.phase = DiscussionPhase.REVIEWING

    await db.flush()
    logger.info("Auto-closed continuous round %d for project %d (timeout)", current_round.number, project_id)
    return True
