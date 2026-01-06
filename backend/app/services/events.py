"""
Server-Sent Events manager for real-time voice notifications.
"""
import asyncio
import json
from dataclasses import asdict, dataclass
from datetime import datetime
from typing import AsyncGenerator


@dataclass
class VoiceEvent:
    """Voice event to send to clients."""

    type: str  # "voice" | "status" | "error" | "connected"
    audio_base64: str | None = None
    explanation: str | None = None
    file_path: str | None = None
    timestamp: str | None = None


class EventManager:
    """Manages SSE connections and broadcasts voice events to clients."""

    def __init__(self):
        self._clients: list[asyncio.Queue] = []

    async def subscribe(self) -> AsyncGenerator[str, None]:
        """
        Subscribe to voice events.

        Returns an async generator of SSE-formatted data strings.
        """
        queue: asyncio.Queue = asyncio.Queue()
        self._clients.append(queue)

        try:
            # Send initial connection event
            yield self._format_sse(
                VoiceEvent(
                    type="connected",
                    timestamp=datetime.utcnow().isoformat(),
                )
            )

            while True:
                # Wait for next event
                event = await queue.get()
                yield self._format_sse(event)

        except asyncio.CancelledError:
            # Client disconnected
            pass
        finally:
            self._clients.remove(queue)

    async def broadcast(self, event: VoiceEvent) -> None:
        """
        Send event to all connected clients.

        Args:
            event: VoiceEvent to broadcast
        """
        if not self._clients:
            return

        for queue in self._clients:
            try:
                await queue.put(event)
            except Exception:
                # Client queue might be full or closed
                pass

    def _format_sse(self, event: VoiceEvent) -> str:
        """Format event as SSE data string."""
        data = json.dumps(asdict(event))
        return f"data: {data}\n\n"

    @property
    def client_count(self) -> int:
        """Number of currently connected clients."""
        return len(self._clients)


# Singleton instance
event_manager = EventManager()
