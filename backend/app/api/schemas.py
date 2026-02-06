from typing import Any

from pydantic import BaseModel, Field


class TranscribeResponse(BaseModel):
    """Response from the transcription endpoint."""

    raw_text: str = Field(description="Raw transcription from Whisper")
    duration: float | None = Field(default=None, description="Audio duration in seconds")
    language: str | None = Field(default=None, description="Detected or specified language")


class PolishRequest(BaseModel):
    """Request body for the polish endpoint."""

    text: str = Field(description="Raw text to polish")
    context: str | None = Field(default=None, description="Context like 'email', 'slack', 'code'")
    formality: str = Field(default="neutral", description="'casual', 'neutral', or 'formal'")
    custom_words: list[str] | None = Field(default=None, description="Custom vocabulary to preserve")


class PolishResponse(BaseModel):
    """Response from the polish endpoint."""

    text: str = Field(description="Polished text")
    input_tokens: int = Field(description="Tokens used for input")
    output_tokens: int = Field(description="Tokens used for output")


class TranscribeAndPolishResponse(BaseModel):
    """Response from the combined transcribe-and-polish endpoint."""

    raw_text: str = Field(description="Raw transcription from Whisper")
    polished_text: str = Field(description="Polished text from Claude")
    duration: float | None = Field(default=None, description="Audio duration in seconds")
    language: str | None = Field(default=None, description="Detected or specified language")
    usage: dict = Field(description="Token usage from Claude")
    saved: bool = Field(default=False, description="Whether the transcript was saved (user authenticated)")


class HealthResponse(BaseModel):
    """Response from the health check endpoint."""

    status: str
    version: str
    groq_configured: bool
    anthropic_configured: bool


class DescribeScreenRequest(BaseModel):
    """Request body for screen description."""

    image_base64: str = Field(description="Base64-encoded PNG screenshot")
    blind_mode: bool = Field(default=False, description="If true, provide exhaustive visual detail")
    detail: int = Field(default=3, ge=1, le=5, description="Detail level 1-5")
    model: str | None = Field(default=None, description="Vision model override")
    focus: str | None = Field(default=None, description="Focus mode: general, errors, code, text")
    uia_tree: str | None = Field(default=None, description="Windows UI Automation tree text (from Rust)")


class DescribeScreenResponse(BaseModel):
    """Response from screen description."""

    description: str = Field(description="Text description of the screen")
    input_tokens: int = Field(description="Tokens used for input (includes image)")
    output_tokens: int = Field(description="Tokens used for output")


class TranscribeBase64Request(BaseModel):
    """Request body for base64-encoded audio transcription."""

    audio_base64: str = Field(description="Base64-encoded WAV audio")
    language: str | None = Field(default=None, description="Optional language code")


class ScreenReaderMessage(BaseModel):
    """A single message in a screen reader conversation."""

    role: str = Field(description="'user' or 'assistant'")
    content: str = Field(description="Message text")


class ScreenReaderChatRequest(BaseModel):
    """Request body for screen reader follow-up chat."""

    image_base64: str = Field(description="Base64-encoded PNG screenshot")
    messages: list[ScreenReaderMessage] = Field(description="Conversation history")
    blind_mode: bool = Field(default=False, description="If true, provide exhaustive visual detail")
    detail: int = Field(default=3, ge=1, le=5, description="Detail level 1-5")
    model: str | None = Field(default=None, description="Vision model override")
    focus: str | None = Field(default=None, description="Focus mode: general, errors, code, text")
    uia_tree: str | None = Field(default=None, description="Windows UI Automation tree text")


class ScreenReaderChatResponse(BaseModel):
    """Response from screen reader chat."""

    response: str = Field(description="Assistant response text")
    input_tokens: int = Field(description="Tokens used for input")
    output_tokens: int = Field(description="Tokens used for output")


class ComputerUseMessage(BaseModel):
    """A single message in a computer use conversation."""

    role: str = Field(description="'user', 'assistant', or 'tool'")
    content: Any = Field(description="String, list of content blocks, or tool_result")


class ComputerUseRequest(BaseModel):
    """Request body for computer use endpoint."""

    messages: list[ComputerUseMessage] = Field(description="Conversation messages")
    display_width: int = Field(default=1920, description="Display width in pixels")
    display_height: int = Field(default=1080, description="Display height in pixels")
    model: str | None = Field(default=None, description="Model override")
    uia_tree: str | None = Field(default=None, description="Windows UI Automation tree text")


class ComputerUseResponse(BaseModel):
    """Response from computer use endpoint."""

    content: list[dict] = Field(description="Raw content blocks from Anthropic")
    stop_reason: str = Field(description="Stop reason: 'tool_use' or 'end_turn'")
    input_tokens: int = Field(description="Tokens used for input")
    output_tokens: int = Field(description="Tokens used for output")


class ErrorResponse(BaseModel):
    """Standard error response."""

    error: str
    detail: str | None = None
