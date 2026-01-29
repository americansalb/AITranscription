"""MCP server that sends speak requests to Vaak desktop app."""

import asyncio
import urllib.request
import urllib.error
import json
import os
import uuid
import socket
import psutil
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import Tool, TextContent

VAAK_URL = "http://127.0.0.1:7865/speak"
HEARTBEAT_URL = "http://127.0.0.1:7865/heartbeat"
HEARTBEAT_INTERVAL_SECONDS = 120  # Send heartbeat every 2 minutes

server = Server("vaak-speak")

# Shell/terminal process names to look for when walking up the process tree
SHELL_NAMES = {
    # Windows shells
    'powershell.exe', 'pwsh.exe', 'cmd.exe',
    # Windows terminals
    'windowsterminal.exe', 'wt.exe', 'conhost.exe',
    # Unix shells
    'bash', 'zsh', 'fish', 'sh', 'tcsh', 'csh', 'ksh', 'dash',
    # macOS/Linux terminals
    'terminal', 'gnome-terminal', 'konsole', 'xterm',
    'iterm2', 'alacritty', 'kitty', 'hyper', 'warp',
}


def get_session_id() -> str:
    """Get session ID based on terminal/shell process, not immediate parent.

    The MCP server is spawned by Claude Code, which is spawned by the terminal.
    We want the terminal's PID so that all Claude instances in the same terminal
    share the same session, even if Claude Code restarts.

    Process tree:
        Terminal (PowerShell/bash) ─ PID 5000  <-- We want THIS
          └── Claude Code ─ PID 6000
                └── MCP Server ─ PID 7000, ppid = 6000
    """
    # Check environment override first
    if env_session := os.environ.get("CLAUDE_SESSION_ID"):
        return env_session

    hostname = socket.gethostname()

    try:
        proc = psutil.Process()

        # Walk up the process tree looking for a shell/terminal
        while proc.parent():
            parent = proc.parent()
            try:
                parent_name = parent.name().lower()
                if parent_name in SHELL_NAMES:
                    return f"{hostname}-{parent.pid}"
                proc = parent
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                break

        # Fallback: use grandparent PID (skip Claude Code, get its parent)
        # This handles cases where we can't identify the shell by name
        try:
            ppid = os.getppid()  # Claude Code's PID
            grandparent = psutil.Process(ppid).parent()
            if grandparent:
                return f"{hostname}-{grandparent.pid}"
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass

    except Exception:
        pass

    # Last resort fallback to immediate parent
    return f"{hostname}-{os.getppid()}"


SESSION_ID = get_session_id()


@server.list_tools()
async def list_tools() -> list[Tool]:
    """List available tools."""
    return [
        Tool(
            name="speak",
            description=(
                "IMPORTANT: You MUST use this tool to speak your responses aloud to the user. "
                "The user relies on voice output and cannot see the screen. "
                "Call this tool after completing tasks to announce what you did. "
                "Keep messages concise (1-3 sentences). "
                "Example: speak({text: 'I updated the login function to validate email addresses.'})"
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text to speak aloud. Be concise and conversational. Summarize what you did.",
                    },
                },
                "required": ["text"],
            },
        ),
    ]


def send_to_vaak(text: str) -> tuple[bool, str]:
    """Send text to Vaak's local speak endpoint with session ID.

    Returns:
        tuple: (success: bool, instructions: str)
    """
    try:
        data = json.dumps({
            "text": text,
            "session_id": SESSION_ID
        }).encode("utf-8")
        req = urllib.request.Request(
            VAAK_URL,
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            if resp.status == 200:
                response_data = json.loads(resp.read().decode("utf-8"))
                instructions = response_data.get("instructions", "")
                return (True, instructions)
            return (False, "")
    except urllib.error.URLError:
        return (False, "")
    except Exception:
        return (False, "")


def send_heartbeat() -> bool:
    """Send heartbeat to Vaak to indicate this session is still active.

    Returns:
        bool: True if heartbeat was received successfully
    """
    try:
        data = json.dumps({
            "session_id": SESSION_ID
        }).encode("utf-8")
        req = urllib.request.Request(
            HEARTBEAT_URL,
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status == 200
    except Exception:
        return False


async def heartbeat_loop():
    """Background task that sends heartbeats to Vaak periodically."""
    # Send initial heartbeat immediately
    loop = asyncio.get_event_loop()
    await loop.run_in_executor(None, send_heartbeat)

    while True:
        await asyncio.sleep(HEARTBEAT_INTERVAL_SECONDS)
        await loop.run_in_executor(None, send_heartbeat)


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[TextContent]:
    """Handle tool calls."""
    if name != "speak":
        return [TextContent(type="text", text=f"Unknown tool: {name}")]

    text = arguments.get("text", "")
    if not text:
        return [TextContent(type="text", text="No text provided to speak.")]

    # Send to Vaak in a thread to not block
    loop = asyncio.get_event_loop()
    success, instructions = await loop.run_in_executor(None, send_to_vaak, text)

    if success:
        # Return instructions from Vaak (contains voice mode/detail settings)
        # This allows Claude to adjust its communication style based on user preferences
        return [TextContent(type="text", text=instructions if instructions else f"Spoke: \"{text}\"")]
    else:
        return [TextContent(
            type="text",
            text="Could not reach Vaak. Make sure the Vaak desktop app is running."
        )]


async def run_server():
    """Run the MCP server with stdio transport and heartbeat loop."""
    # Start heartbeat loop in background
    heartbeat_task = asyncio.create_task(heartbeat_loop())

    try:
        async with stdio_server() as (read_stream, write_stream):
            await server.run(read_stream, write_stream, server.create_initialization_options())
    finally:
        # Cancel heartbeat when server stops
        heartbeat_task.cancel()
        try:
            await heartbeat_task
        except asyncio.CancelledError:
            pass


def main():
    """Run the MCP server."""
    asyncio.run(run_server())


if __name__ == "__main__":
    main()
