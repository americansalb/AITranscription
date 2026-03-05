"""Configuration from environment variables."""

import os
from pathlib import Path
from dotenv import load_dotenv

load_dotenv(Path(__file__).parent / ".env")

# Transcription (Groq Whisper)
GROQ_API_KEY: str = os.environ.get("GROQ_API_KEY", "")
WHISPER_MODEL: str = os.environ.get("WHISPER_MODEL", "whisper-large-v3-turbo")

# Translation LLM keys
ANTHROPIC_API_KEY: str = os.environ.get("ANTHROPIC_API_KEY", "")
OPENAI_API_KEY: str = os.environ.get("OPENAI_API_KEY", "")
GOOGLE_API_KEY: str = os.environ.get("GOOGLE_API_KEY", "")
# Groq key is reused for both Whisper and Llama translation

# Default models per provider
ANTHROPIC_MODEL: str = os.environ.get("ANTHROPIC_MODEL", "claude-sonnet-4-20250514")
OPENAI_MODEL: str = os.environ.get("OPENAI_MODEL", "gpt-4o-mini")
GROQ_LLAMA_MODEL: str = os.environ.get("GROQ_LLAMA_MODEL", "llama-3.3-70b-versatile")
GOOGLE_MODEL: str = os.environ.get("GOOGLE_MODEL", "gemini-2.0-flash")

PORT: int = int(os.environ.get("PORT", "19837"))
DEBUG: bool = os.environ.get("DEBUG", "false").lower() == "true"
