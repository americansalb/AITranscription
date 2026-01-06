"""MCP server providing a speak tool for text-to-speech."""

import asyncio
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import Tool, TextContent

from .tts import speak as tts_speak, get_engine

# Create the MCP server
server = Server("mcp-speak")


@server.list_tools()
async def list_tools() -> list[Tool]:
    """List available tools."""
    return [
        Tool(
            name="speak",
            description=(
                "Speak text aloud using text-to-speech. Use this to announce what you're doing, "
                "explain code changes, or provide audio feedback to the user. "
                "Keep messages concise (1-2 sentences) for best results."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text to speak aloud. Keep it concise and conversational.",
                    },
                    "voice": {
                        "type": "string",
                        "description": "Voice to use (optional). Options: Aria-PlayAI, Atlas-PlayAI, Indigo-PlayAI",
                        "default": "Aria-PlayAI",
                    },
                },
                "required": ["text"],
            },
        ),
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[TextContent]:
    """Handle tool calls."""
    if name != "speak":
        return [TextContent(type="text", text=f"Unknown tool: {name}")]

    text = arguments.get("text", "")
    voice = arguments.get("voice", "Aria-PlayAI")

    if not text:
        return [TextContent(type="text", text="No text provided to speak.")]

    # Run TTS in a thread to not block
    loop = asyncio.get_event_loop()
    success = await loop.run_in_executor(None, tts_speak, text, voice)

    if success:
        return [TextContent(type="text", text=f"Spoke: \"{text}\"")]
    else:
        return [TextContent(type="text", text=f"Failed to speak text. Check TTS configuration.")]


def main():
    """Run the MCP server."""
    import sys

    # Check for Groq API key
    engine = get_engine()
    if engine.groq_client:
        print("MCP Speak: Using Groq TTS", file=sys.stderr)
    else:
        print("MCP Speak: Using system TTS (set GROQ_API_KEY for better quality)", file=sys.stderr)

    # Run the server
    asyncio.run(stdio_server(server))


if __name__ == "__main__":
    main()
