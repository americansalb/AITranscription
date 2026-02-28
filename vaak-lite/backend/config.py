"""Minimal configuration loaded from environment variables."""

import os
from pathlib import Path
from dotenv import load_dotenv

# Load .env from this directory
load_dotenv(Path(__file__).parent / ".env")

GROQ_API_KEY: str = os.environ.get("GROQ_API_KEY", "")
WHISPER_MODEL: str = os.environ.get("WHISPER_MODEL", "whisper-large-v3-turbo")
PORT: int = int(os.environ.get("PORT", "19837"))
DEBUG: bool = os.environ.get("DEBUG", "false").lower() == "true"
