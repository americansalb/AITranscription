"""MCP server that sends speak requests to Scribe desktop app."""

import asyncio
import urllib.request
import urllib.error
import json
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import Tool, TextContent

SCRIBE_URL = "http://127.0.0.1:7865/speak"

server = Server("scribe-speak")


@server.list_tools()
async def list_tools() -> list[Tool]:
    """List available tools."""
    return [
        Tool(
            name="speak",
            description=(
                "Speak text aloud through Scribe. Use this to announce what you're doing, "
                "explain code changes, or provide audio feedback. "
                "Keep messages concise (1-2 sentences). Requires Scribe desktop app to be running."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text to speak aloud. Keep it concise and conversational.",
                    },
                },
                "required": ["text"],
            },
        ),
    ]


def send_to_scribe(text: str) -> bool:
    """Send text to Scribe's local speak endpoint."""
    try:
        data = json.dumps({"text": text}).encode("utf-8")
        req = urllib.request.Request(
            SCRIBE_URL,
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status == 200
    except urllib.error.URLError:
        return False
    except Exception:
        return False


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[TextContent]:
    """Handle tool calls."""
    if name != "speak":
        return [TextContent(type="text", text=f"Unknown tool: {name}")]

    text = arguments.get("text", "")
    if not text:
        return [TextContent(type="text", text="No text provided to speak.")]

    # Send to Scribe in a thread to not block
    loop = asyncio.get_event_loop()
    success = await loop.run_in_executor(None, send_to_scribe, text)

    if success:
        return [TextContent(type="text", text=f"Spoke: \"{text}\"")]
    else:
        return [TextContent(
            type="text",
            text="Could not reach Scribe. Make sure the Scribe desktop app is running."
        )]


def main():
    """Run the MCP server."""
    asyncio.run(stdio_server(server))


if __name__ == "__main__":
    main()
