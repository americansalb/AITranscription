"""Discussion system — Delphi, Oxford, Red Team, Continuous review."""

import logging
import random
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


# --- Schemas ---

class StartDiscussionRequest(BaseModel):
    mode: str = Field(description="delphi, oxford, red_team, or continuous")
    topic: str = Field(min_length=1, max_length=2000)
    participants: list[str] = Field(default_factory=list, description="role:instance IDs")
    max_rounds: int = Field(default=10, ge=1, le=999)
    timeout_minutes: int = Field(default=15, ge=1)
    auto_close_timeout_seconds: int = Field(default=60, ge=0)


class SubmissionRequest(BaseModel):
    body: str = Field(min_length=1, max_length=10000)


class SetTeamsRequest(BaseModel):
    teams: dict = Field(description='{"for": ["role:0"], "against": ["role:1"]}')


class RoundResponse(BaseModel):
    number: int
    topic: str | None
    auto_triggered: bool
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
    teams: dict | None
    rounds: list[RoundResponse]
    started_at: str


# --- Endpoints ---

@router.post("/{project_id}/discussions", status_code=201)
async def start_discussion(
    project_id: int,
    request: StartDiscussionRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Start a new structured discussion in a project."""
    project = await _get_user_project(db, project_id, user.id)

    # Check no active discussion already
    existing = await db.execute(
        select(Discussion).where(
            Discussion.project_id == project_id,
            Discussion.is_active == True,
        )
    )
    if existing.scalar_one_or_none():
        raise HTTPException(status_code=409, detail="A discussion is already active in this project")

    # Validate mode
    try:
        mode = DiscussionMode(request.mode)
    except ValueError:
        raise HTTPException(status_code=400, detail=f"Invalid mode: {request.mode}")

    # Determine initial phase based on mode
    if mode == DiscussionMode.CONTINUOUS:
        phase = DiscussionPhase.REVIEWING
        initial_round = 0
    elif mode == DiscussionMode.DELPHI:
        phase = DiscussionPhase.PREPARING
        initial_round = 0
    else:  # oxford, red_team
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
        max_rounds=request.max_rounds if mode != DiscussionMode.CONTINUOUS else 999,
        timeout_minutes=request.timeout_minutes,
        auto_close_timeout_seconds=request.auto_close_timeout_seconds,
    )
    db.add(discussion)
    await db.flush()

    # Create first round for oxford/red_team
    if initial_round == 1:
        round_obj = DiscussionRound(
            discussion_id=discussion.id,
            number=1,
            topic=request.topic,
        )
        db.add(round_obj)

    # Post announcement to board
    announcement = Message(
        project_id=project_id,
        from_role="system",
        to_role="all",
        msg_type="broadcast",
        subject=f"Discussion started: {request.topic[:100]}",
        body=f"Mode: {mode.value}, Phase: {phase.value}, "
             f"Participants: {', '.join(request.participants) or 'all'}",
    )
    db.add(announcement)

    await db.commit()
    await db.refresh(discussion)

    # Broadcast via WebSocket
    await ws_manager.broadcast(project_id, {
        "type": "discussion_started",
        "discussion_id": discussion.id,
        "mode": mode.value,
        "topic": request.topic,
    })

    logger.info("Discussion started: id=%d mode=%s project=%d", discussion.id, mode.value, project_id)
    return _discussion_response(discussion)


@router.get("/{project_id}/discussions/active", response_model=DiscussionResponse | None)
async def get_active_discussion(
    project_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Get the currently active discussion for a project."""
    await _get_user_project(db, project_id, user.id)

    result = await db.execute(
        select(Discussion).where(
            Discussion.project_id == project_id,
            Discussion.is_active == True,
        )
    )
    discussion = result.scalar_one_or_none()
    if not discussion:
        return None
    return _discussion_response(discussion)


@router.post("/{project_id}/discussions/{discussion_id}/open-round")
async def open_next_round(
    project_id: int,
    discussion_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Open the next round of a discussion."""
    discussion = await _get_discussion(db, project_id, discussion_id, user.id)

    if discussion.phase not in (DiscussionPhase.REVIEWING, DiscussionPhase.PREPARING):
        raise HTTPException(status_code=409, detail=f"Cannot open round in phase '{discussion.phase.value}'")

    if discussion.current_round >= discussion.max_rounds:
        raise HTTPException(status_code=409, detail="Maximum rounds reached")

    discussion.current_round += 1
    discussion.phase = DiscussionPhase.SUBMITTING

    new_round = DiscussionRound(
        discussion_id=discussion.id,
        number=discussion.current_round,
        topic=discussion.topic,
    )
    db.add(new_round)

    # Announce
    msg = Message(
        project_id=project_id,
        from_role="system",
        to_role="all",
        msg_type="broadcast",
        subject=f"Round {discussion.current_round} opened",
        body=f"Topic: {discussion.topic[:200]}. Submit your responses.",
    )
    db.add(msg)

    await db.commit()

    await ws_manager.broadcast(project_id, {
        "type": "round_opened",
        "discussion_id": discussion.id,
        "round": discussion.current_round,
    })

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
    """Submit a response to the current discussion round."""
    discussion = await _get_discussion(db, project_id, discussion_id, user.id)

    if discussion.phase != DiscussionPhase.SUBMITTING:
        raise HTTPException(status_code=409, detail="Not accepting submissions right now")

    # Get current round
    result = await db.execute(
        select(DiscussionRound).where(
            DiscussionRound.discussion_id == discussion.id,
            DiscussionRound.number == discussion.current_round,
        )
    )
    current_round = result.scalar_one_or_none()
    if not current_round:
        raise HTTPException(status_code=404, detail="Current round not found")

    # Post submission as a board message
    from_label = f"{role_slug}:{user.id}"
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

    # Record submission
    submission = DiscussionSubmission(
        round_id=current_round.id,
        from_role=from_label,
        message_id=msg.id,
    )
    db.add(submission)
    await db.commit()

    return {"status": "submitted", "round": discussion.current_round}


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

    # Get current round with submissions
    result = await db.execute(
        select(DiscussionRound).where(
            DiscussionRound.discussion_id == discussion.id,
            DiscussionRound.number == discussion.current_round,
        )
    )
    current_round = result.scalar_one_or_none()
    if not current_round:
        raise HTTPException(status_code=404, detail="Current round not found")

    current_round.closed_at = datetime.now(timezone.utc)

    # Generate aggregate based on mode
    if discussion.mode == DiscussionMode.CONTINUOUS:
        aggregate = await _generate_tally(db, current_round, discussion)
    else:
        aggregate = await _generate_anonymized_aggregate(db, current_round)

    current_round.aggregate = aggregate

    # Post aggregate as board message
    agg_msg = Message(
        project_id=project_id,
        from_role="system",
        to_role="all",
        msg_type="broadcast",
        subject=f"Round {discussion.current_round} aggregate",
        body=_format_aggregate(aggregate, discussion.mode),
    )
    db.add(agg_msg)
    await db.flush()
    current_round.aggregate_message_id = agg_msg.id

    discussion.phase = DiscussionPhase.REVIEWING
    await db.commit()

    await ws_manager.broadcast(project_id, {
        "type": "round_closed",
        "discussion_id": discussion.id,
        "round": discussion.current_round,
        "aggregate": aggregate,
    })

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

    msg = Message(
        project_id=project_id,
        from_role="system",
        to_role="all",
        msg_type="broadcast",
        subject="Discussion ended",
        body=f"Discussion '{discussion.topic[:100]}' has been concluded after {discussion.current_round} rounds.",
    )
    db.add(msg)

    await db.commit()

    await ws_manager.broadcast(project_id, {
        "type": "discussion_ended",
        "discussion_id": discussion.id,
    })

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


# --- Helpers ---

async def _get_user_project(db: AsyncSession, project_id: int, user_id: int) -> Project:
    result = await db.execute(
        select(Project).where(Project.id == project_id, Project.owner_id == user_id)
    )
    project = result.scalar_one_or_none()
    if not project:
        raise HTTPException(status_code=404, detail="Project not found")
    return project


async def _get_discussion(
    db: AsyncSession, project_id: int, discussion_id: int, user_id: int
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
    if not discussion.is_active:
        raise HTTPException(status_code=409, detail="Discussion is no longer active")
    return discussion


async def _generate_tally(
    db: AsyncSession, round_obj: DiscussionRound, discussion: Discussion
) -> dict:
    """Generate a lightweight tally for continuous review (agree/disagree/alternative)."""
    submissions = round_obj.submissions
    tally = {"agree": 0, "disagree": 0, "alternative": 0, "total": len(submissions)}

    for sub in submissions:
        # Fetch the message body to classify
        result = await db.execute(select(Message).where(Message.id == sub.message_id))
        msg = result.scalar_one_or_none()
        if not msg:
            tally["agree"] += 1  # Silence = consent
            continue

        classification = _classify_response(msg.body)
        tally[classification] += 1

    # Count non-submitters as "agree" (silence = consent)
    participants = discussion.participants or []
    author = round_obj.trigger_from
    non_author_participants = [p for p in participants if p != author]
    submitted_roles = {sub.from_role for sub in submissions}
    silent = [p for p in non_author_participants if p not in submitted_roles]
    tally["agree"] += len(silent)
    tally["total"] += len(silent)
    tally["silent_count"] = len(silent)

    return tally


async def _generate_anonymized_aggregate(
    db: AsyncSession, round_obj: DiscussionRound
) -> dict:
    """Generate a Fisher-Yates shuffled anonymized aggregate for Delphi/Oxford."""
    submissions = round_obj.submissions
    entries = []

    for sub in submissions:
        result = await db.execute(select(Message).where(Message.id == sub.message_id))
        msg = result.scalar_one_or_none()
        if msg:
            entries.append({"body": msg.body, "submitted_at": sub.submitted_at.isoformat()})

    # Fisher-Yates shuffle with UUID seed for reproducibility
    seed = uuid.uuid4().int
    rng = random.Random(seed)
    for i in range(len(entries) - 1, 0, -1):
        j = rng.randint(0, i)
        entries[i], entries[j] = entries[j], entries[i]

    return {
        "submission_count": len(entries),
        "anonymized_entries": entries,
        "shuffle_seed": str(seed),
    }


def _classify_response(body: str) -> str:
    """Classify a submission as agree/disagree/alternative."""
    lower = body.lower().strip()
    agree_signals = ["agree", "lgtm", "+1", "looks good", "approved", "no objection", "ship it"]
    disagree_signals = ["disagree", "-1", "object", "reject", "nack", "block"]
    alternative_signals = ["suggest", "alternative", "instead", "what if", "how about", "counter-proposal"]

    for signal in disagree_signals:
        if signal in lower:
            return "disagree"
    for signal in alternative_signals:
        if signal in lower:
            return "alternative"
    for signal in agree_signals:
        if signal in lower:
            return "agree"

    return "agree"  # Default: unclassified = agree


def _format_aggregate(aggregate: dict, mode: DiscussionMode) -> str:
    """Format aggregate as human-readable text for the board message."""
    if "tally" in aggregate or "agree" in aggregate:
        # Continuous tally
        return (
            f"Tally: {aggregate.get('agree', 0)} agree, "
            f"{aggregate.get('disagree', 0)} disagree, "
            f"{aggregate.get('alternative', 0)} alternative "
            f"({aggregate.get('silent_count', 0)} silent=consent)"
        )
    else:
        # Anonymized aggregate
        count = aggregate.get("submission_count", 0)
        entries = aggregate.get("anonymized_entries", [])
        parts = [f"Round aggregate ({count} submissions, anonymized):"]
        for i, entry in enumerate(entries, 1):
            preview = entry["body"][:200]
            parts.append(f"\n---\nSubmission {i}:\n{preview}")
        return "\n".join(parts)


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
        teams=discussion.teams,
        rounds=[
            RoundResponse(
                number=r.number,
                topic=r.topic,
                auto_triggered=r.auto_triggered,
                opened_at=r.opened_at.isoformat(),
                closed_at=r.closed_at.isoformat() if r.closed_at else None,
                submission_count=len(r.submissions),
                aggregate=r.aggregate,
            )
            for r in discussion.rounds
        ],
        started_at=discussion.started_at.isoformat(),
    )
