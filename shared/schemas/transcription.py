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
    saved: bool = Field(default=False, description="Whether the transcript was saved")
    polish_failed: bool = Field(default=False, description="Whether polish failed and raw text was used as fallback")
