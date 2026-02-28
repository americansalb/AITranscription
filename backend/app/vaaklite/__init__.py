"""Vaak Lite — Live interpretation & translation sub-app.

Mounted at /vaaklite on the main FastAPI application.
"""

import os

# Config — reads from same env vars as the parent app
GROQ_API_KEY: str = os.environ.get("GROQ_API_KEY", "")
WHISPER_MODEL: str = os.environ.get("WHISPER_MODEL", "whisper-large-v3-turbo")

ANTHROPIC_API_KEY: str = os.environ.get("ANTHROPIC_API_KEY", "")
OPENAI_API_KEY: str = os.environ.get("OPENAI_API_KEY", "")
GOOGLE_API_KEY: str = os.environ.get("GOOGLE_API_KEY", "")

ANTHROPIC_MODEL: str = os.environ.get("ANTHROPIC_MODEL", "claude-sonnet-4-20250514")
OPENAI_MODEL: str = os.environ.get("OPENAI_MODEL", "gpt-4o-mini")
GROQ_LLAMA_MODEL: str = os.environ.get("GROQ_LLAMA_MODEL", "llama-3.3-70b-versatile")
GOOGLE_MODEL: str = os.environ.get("GOOGLE_MODEL", "gemini-2.0-flash")
