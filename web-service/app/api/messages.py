"""Real-time message board — WebSocket + REST fallback."""

from fastapi import APIRouter, WebSocket, WebSocketDisconnect, HTTPException
from pydantic import BaseModel, Field

from shared.schemas.collab import BoardMessage, MessageType

router = APIRouter()


# --- Request schemas ---

class SendMessageRequest(BaseModel):
    to: str = Field(description="Target role slug or 'all'")
    type: MessageType
    subject: str
    body: str
    metadata: dict = {}


class MessageListResponse(BaseModel):
    messages: list[BoardMessage]
    total: int


# --- REST endpoints ---

@router.get("/{project_id}", response_model=MessageListResponse)
async def get_messages(project_id: str, since_id: int = 0, limit: int = 50):
    """Get messages from the board, optionally filtered by since_id."""
    # TODO: query DB for messages
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.post("/{project_id}", response_model=BoardMessage)
async def send_message(project_id: str, request: SendMessageRequest):
    """Post a message to the board (human user sending)."""
    # TODO: insert message into DB, broadcast via WebSocket
    raise HTTPException(status_code=501, detail="Not implemented yet")


# --- WebSocket endpoint ---

@router.websocket("/{project_id}/ws")
async def websocket_endpoint(websocket: WebSocket, project_id: str):
    """Real-time message stream for a project.

    Clients connect here to receive new messages as they're posted.
    Supports sending messages through the socket as well.
    """
    await websocket.accept()
    # TODO: register connection, stream new messages, handle sends
    try:
        while True:
            data = await websocket.receive_text()
            # Parse incoming message and post to board
            await websocket.send_json({"status": "received"})
    except WebSocketDisconnect:
        pass  # Clean up connection
