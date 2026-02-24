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
from app.models import Message, Project, WebUser

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


# --- REST endpoints ---

@router.get("/{project_id}")
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

    return {
        "messages": [_msg_to_dict(m) for m in messages],
        "total": len(messages),
    }


@router.post("/{project_id}")
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
    response = _msg_to_dict(msg)
    await manager.broadcast(project_id, response)

    # Auto-trigger continuous review round if status message
    if request.type == "status":
        await _maybe_trigger_continuous_round(db, project_id, msg)

    return response


@router.delete("/{project_id}/{message_id}", status_code=204)
async def delete_message(
    project_id: int,
    message_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Delete a message from the board. Only project owner can delete."""
    # Verify project ownership
    result = await db.execute(
        select(Project).where(Project.id == project_id, Project.owner_id == user.id)
    )
    if not result.scalar_one_or_none():
        raise HTTPException(status_code=404, detail="Project not found")

    result = await db.execute(
        select(Message).where(Message.id == message_id, Message.project_id == project_id)
    )
    msg = result.scalar_one_or_none()
    if not msg:
        raise HTTPException(status_code=404, detail="Message not found")

    await db.delete(msg)
    await db.commit()


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

    # Verify user owns this project (prevents IDOR — any user accessing any project)
    async with async_session() as db:
        result = await db.execute(
            select(Project).where(Project.id == project_id, Project.owner_id == user_id)
        )
        if not result.scalar_one_or_none():
            await websocket.close(code=4003, reason="Not authorized for this project")
            return

    # Register the authenticated + authorized connection with the manager
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
                # Validate input (same limits as REST SendMessageRequest)
                ws_body = str(data.get("body", ""))[:10000]
                ws_subject = str(data.get("subject", ""))[:500]
                ws_to = str(data.get("to", "all"))[:100]
                ws_msg_type = str(data.get("msg_type", "message"))[:50]

                async with async_session() as db:
                    msg = Message(
                        project_id=project_id,
                        from_role=f"human:{user_id}",
                        to_role=ws_to,
                        msg_type=ws_msg_type,
                        subject=ws_subject,
                        body=ws_body,
                    )
                    db.add(msg)
                    await db.commit()
                    await db.refresh(msg)
                    response = _msg_to_dict(msg)
                    await manager.broadcast(project_id, response)

                    # Auto-trigger continuous review for status messages
                    if data.get("msg_type") == "status":
                        await _maybe_trigger_continuous_round(db, project_id, msg)

    except WebSocketDisconnect:
        manager.disconnect(project_id, websocket)
        logger.info("WS disconnected: project=%d user=%d", project_id, user_id)
    except Exception as e:
        logger.error("WS error: project=%d user=%d error=%s", project_id, user_id, e)
        manager.disconnect(project_id, websocket)


def _msg_to_dict(msg: Message) -> dict:
    """Convert a Message ORM object to a dict matching the frontend BoardMessage interface."""
    return {
        "id": msg.id,
        "from": msg.from_role,
        "to": msg.to_role,
        "type": msg.msg_type,
        "subject": msg.subject,
        "body": msg.body,
        "timestamp": msg.created_at.isoformat(),
        "metadata": {},
    }


async def _maybe_trigger_continuous_round(
    db: AsyncSession, project_id: int, trigger_msg: Message
) -> None:
    """Delegate to discussions.py for continuous review auto-trigger.

    Uses lazy import to avoid circular dependency (discussions imports manager from here).
    """
    from app.api.discussions import maybe_auto_trigger_continuous

    await maybe_auto_trigger_continuous(
        project_id=project_id,
        from_role=trigger_msg.from_role,
        msg_type=trigger_msg.msg_type,
        message_id=trigger_msg.id,
        subject=trigger_msg.subject,
        db=db,
    )
