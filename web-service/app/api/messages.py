"""Real-time message board — WebSocket + REST."""

import asyncio
import json
import logging
from datetime import datetime, timezone

from fastapi import APIRouter, Depends, HTTPException, WebSocket, WebSocketDisconnect
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import decode_access_token, get_current_user
from app.database import async_session, get_db
from app.models import (
    Discussion,
    DiscussionMode,
    DiscussionPhase,
    DiscussionRound,
    Message,
    Project,
    WebUser,
)

logger = logging.getLogger(__name__)
router = APIRouter()


# --- WebSocket connection manager ---

class ConnectionManager:
    """Manages WebSocket connections per project."""

    def __init__(self):
        self._connections: dict[int, list[WebSocket]] = {}

    async def connect(self, project_id: int, websocket: WebSocket):
        await websocket.accept()
        if project_id not in self._connections:
            self._connections[project_id] = []
        self._connections[project_id].append(websocket)
        logger.info("WS connected: project=%d total=%d", project_id, len(self._connections[project_id]))

    def disconnect(self, project_id: int, websocket: WebSocket):
        if project_id in self._connections:
            self._connections[project_id] = [
                ws for ws in self._connections[project_id] if ws is not websocket
            ]

    async def broadcast(self, project_id: int, data: dict):
        """Send a message to all connected clients for a project."""
        if project_id not in self._connections:
            return
        dead = []
        for ws in self._connections[project_id]:
            try:
                await ws.send_json(data)
            except Exception:
                dead.append(ws)
        for ws in dead:
            self._connections[project_id].remove(ws)


manager = ConnectionManager()


# --- Schemas ---

class SendMessageRequest(BaseModel):
    to: str = Field(description="Target role slug or 'all'")
    type: str = Field(default="message", description="Message type")
    subject: str = Field(default="", max_length=500)
    body: str = Field(max_length=10000)


class MessageResponse(BaseModel):
    id: int
    project_id: int
    from_role: str
    to_role: str
    msg_type: str
    subject: str
    body: str
    created_at: str


class MessageListResponse(BaseModel):
    messages: list[MessageResponse]
    total: int


# --- REST endpoints ---

@router.get("/{project_id}", response_model=MessageListResponse)
async def get_messages(
    project_id: int,
    since_id: int = 0,
    limit: int = 50,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Get messages from the board, optionally filtered by since_id."""
    # Verify project ownership
    result = await db.execute(
        select(Project).where(Project.id == project_id, Project.owner_id == user.id)
    )
    if not result.scalar_one_or_none():
        raise HTTPException(status_code=404, detail="Project not found")

    query = (
        select(Message)
        .where(Message.project_id == project_id)
        .order_by(Message.id.asc())
    )
    if since_id > 0:
        query = query.where(Message.id > since_id)
    query = query.limit(limit)

    result = await db.execute(query)
    messages = result.scalars().all()

    return MessageListResponse(
        messages=[_msg_response(m) for m in messages],
        total=len(messages),
    )


@router.post("/{project_id}", response_model=MessageResponse)
async def send_message(
    project_id: int,
    request: SendMessageRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Post a message to the board (human user sending)."""
    # Verify project ownership
    result = await db.execute(
        select(Project).where(Project.id == project_id, Project.owner_id == user.id)
    )
    if not result.scalar_one_or_none():
        raise HTTPException(status_code=404, detail="Project not found")

    msg = Message(
        project_id=project_id,
        from_role=f"human:{user.id}",
        to_role=request.to,
        msg_type=request.type,
        subject=request.subject,
        body=request.body,
    )
    db.add(msg)
    await db.commit()
    await db.refresh(msg)

    # Broadcast to WebSocket clients
    response = _msg_response(msg)
    await manager.broadcast(project_id, response.model_dump())

    # Auto-trigger continuous review round if status message
    if request.type == "status":
        await _maybe_trigger_continuous_round(db, project_id, msg)

    return response


# --- WebSocket endpoint ---

@router.websocket("/{project_id}/ws")
async def websocket_endpoint(websocket: WebSocket, project_id: int):
    """Real-time message stream for a project.

    Clients must send an auth token as the first message:
    {"type": "auth", "token": "Bearer ..."}
    """
    # Wait for auth message
    await websocket.accept()
    try:
        first_msg = await asyncio.wait_for(websocket.receive_text(), timeout=10)
        data = json.loads(first_msg)
        if data.get("type") != "auth" or not data.get("token"):
            await websocket.close(code=4001, reason="First message must be auth")
            return

        user_id = decode_access_token(data["token"])
        if user_id is None:
            await websocket.close(code=4001, reason="Invalid token")
            return

    except (asyncio.TimeoutError, json.JSONDecodeError):
        await websocket.close(code=4001, reason="Auth timeout or invalid JSON")
        return

    # Register the authenticated connection with the manager
    if project_id not in manager._connections:
        manager._connections[project_id] = []
    manager._connections[project_id].append(websocket)

    logger.info("WS authenticated: project=%d user=%d", project_id, user_id)

    try:
        while True:
            raw = await websocket.receive_text()
            data = json.loads(raw)

            # Handle incoming messages through the socket
            if data.get("type") == "send":
                async with async_session() as db:
                    msg = Message(
                        project_id=project_id,
                        from_role=f"human:{user_id}",
                        to_role=data.get("to", "all"),
                        msg_type=data.get("msg_type", "message"),
                        subject=data.get("subject", ""),
                        body=data.get("body", ""),
                    )
                    db.add(msg)
                    await db.commit()
                    await db.refresh(msg)
                    response = _msg_response(msg)
                    await manager.broadcast(project_id, response.model_dump())

                    # Auto-trigger continuous review for status messages
                    if data.get("msg_type") == "status":
                        await _maybe_trigger_continuous_round(db, project_id, msg)

    except WebSocketDisconnect:
        manager.disconnect(project_id, websocket)
        logger.info("WS disconnected: project=%d user=%d", project_id, user_id)
    except Exception as e:
        logger.error("WS error: project=%d user=%d error=%s", project_id, user_id, e)
        manager.disconnect(project_id, websocket)


def _msg_response(msg: Message) -> MessageResponse:
    return MessageResponse(
        id=msg.id,
        project_id=msg.project_id,
        from_role=msg.from_role,
        to_role=msg.to_role,
        msg_type=msg.msg_type,
        subject=msg.subject,
        body=msg.body,
        created_at=msg.created_at.isoformat(),
    )


async def _maybe_trigger_continuous_round(
    db: AsyncSession, project_id: int, trigger_msg: Message
) -> None:
    """Auto-create a continuous review round when a status message is posted.

    This mirrors the desktop's auto_create_continuous_round behavior:
    - Only triggers if there's an active continuous discussion
    - Creates a new round with trigger_from set to the message author
    - Posts a review-request broadcast so other participants know to respond
    """
    result = await db.execute(
        select(Discussion).where(
            Discussion.project_id == project_id,
            Discussion.is_active == True,
            Discussion.mode == DiscussionMode.CONTINUOUS,
        )
    )
    discussion = result.scalar_one_or_none()
    if not discussion:
        return

    if discussion.phase not in (DiscussionPhase.REVIEWING, DiscussionPhase.SUBMITTING):
        return

    if discussion.current_round >= discussion.max_rounds:
        return

    # Create a new auto-triggered round
    discussion.current_round += 1
    discussion.phase = DiscussionPhase.SUBMITTING

    new_round = DiscussionRound(
        discussion_id=discussion.id,
        number=discussion.current_round,
        topic=trigger_msg.subject or trigger_msg.body[:200],
        auto_triggered=True,
        trigger_from=trigger_msg.from_role,
        trigger_message_id=trigger_msg.id,
    )
    db.add(new_round)

    # Post review-request broadcast
    review_msg = Message(
        project_id=project_id,
        from_role="system",
        to_role="all",
        msg_type="broadcast",
        subject=f"Review round {discussion.current_round} (auto)",
        body=f"Status update from {trigger_msg.from_role}: {trigger_msg.subject or trigger_msg.body[:200]}\n"
             f"Respond with agree/disagree/alternative, or silence = consent "
             f"(timeout: {discussion.auto_close_timeout_seconds}s).",
    )
    db.add(review_msg)
    await db.commit()

    await manager.broadcast(project_id, {
        "type": "continuous_round_triggered",
        "discussion_id": discussion.id,
        "round": discussion.current_round,
        "trigger_from": trigger_msg.from_role,
    })

    logger.info(
        "Continuous round auto-triggered: discussion=%d round=%d trigger=%s",
        discussion.id, discussion.current_round, trigger_msg.from_role,
    )
