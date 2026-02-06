"""ElevenLabs Text-to-Speech service with chunking for long text and streaming support."""

import re
from typing import AsyncIterator
import httpx
from app.core.config import settings

# ElevenLabs has a ~5000 character limit per request
# We chunk at 4000 to leave room for safety margin
CHUNK_SIZE = 4000
REQUEST_TIMEOUT = 90.0  # 90 seconds for long text


def chunk_text(text: str, max_chars: int = CHUNK_SIZE) -> list[str]:
    """Split text into chunks at sentence boundaries.

    Tries to split at sentence endings (. ! ?) to maintain natural speech flow.
    Falls back to splitting at word boundaries if sentences are too long.
    """
    if len(text) <= max_chars:
        return [text]

    chunks = []
    remaining = text

    while remaining:
        if len(remaining) <= max_chars:
            chunks.append(remaining)
            break

        # Find the last sentence boundary within the limit
        chunk = remaining[:max_chars]

        # Look for sentence endings (. ! ?) followed by space or end
        sentence_end = -1
        for match in re.finditer(r'[.!?]+[\s]', chunk):
            sentence_end = match.end()

        if sentence_end > max_chars // 2:
            # Found a good sentence boundary in the latter half
            chunks.append(remaining[:sentence_end].strip())
            remaining = remaining[sentence_end:].strip()
        else:
            # No good sentence boundary, try to split at word boundary
            last_space = chunk.rfind(' ')
            if last_space > max_chars // 2:
                chunks.append(remaining[:last_space].strip())
                remaining = remaining[last_space:].strip()
            else:
                # Worst case: hard split
                chunks.append(chunk)
                remaining = remaining[max_chars:].strip()

    return chunks


async def synthesize_chunk(text: str, voice_id: str, headers: dict) -> bytes | None:
    """Synthesize a single chunk of text."""
    url = f"https://api.elevenlabs.io/v1/text-to-speech/{voice_id}"

    data = {
        "text": text,
        "model_id": "eleven_turbo_v2_5",
        "voice_settings": {
            "stability": 0.5,
            "similarity_boost": 0.75,
        }
    }

    try:
        async with httpx.AsyncClient() as client:
            response = await client.post(url, json=data, headers=headers, timeout=REQUEST_TIMEOUT)

            if response.status_code == 200:
                return response.content
            else:
                print(f"ElevenLabs API error: {response.status_code} - {response.text}")
                return None
    except Exception as e:
        print(f"ElevenLabs TTS error: {e}")
        return None


async def synthesize_stream(text: str, voice_id: str | None = None) -> AsyncIterator[bytes]:
    """Stream TTS audio chunks for faster time-to-first-audio.

    Yields audio bytes as they arrive from ElevenLabs streaming API.
    """
    if not settings.elevenlabs_api_key:
        return

    voice = voice_id or settings.elevenlabs_voice_id

    url = f"https://api.elevenlabs.io/v1/text-to-speech/{voice}/stream"

    headers = {
        "Accept": "audio/mpeg",
        "Content-Type": "application/json",
        "xi-api-key": settings.elevenlabs_api_key,
    }

    data = {
        "text": text,
        "model_id": "eleven_turbo_v2_5",
        "voice_settings": {
            "stability": 0.5,
            "similarity_boost": 0.75,
        },
        "optimize_streaming_latency": 3,
    }

    try:
        async with httpx.AsyncClient() as client:
            async with client.stream("POST", url, json=data, headers=headers, timeout=REQUEST_TIMEOUT) as response:
                if response.status_code != 200:
                    print(f"ElevenLabs streaming error: {response.status_code}")
                    return

                async for chunk in response.aiter_bytes(chunk_size=4096):
                    if chunk:
                        yield chunk
    except Exception as e:
        print(f"ElevenLabs streaming error: {e}")


async def synthesize(text: str, voice_id: str | None = None) -> bytes | None:
    """Convert text to speech using ElevenLabs API.

    Automatically chunks long text and concatenates the audio.
    Returns MP3 audio bytes or None if failed.
    """
    if not settings.elevenlabs_api_key:
        return None

    voice = voice_id or settings.elevenlabs_voice_id

    headers = {
        "Accept": "audio/mpeg",
        "Content-Type": "application/json",
        "xi-api-key": settings.elevenlabs_api_key,
    }

    # Chunk the text if needed
    chunks = chunk_text(text)

    if len(chunks) == 1:
        # Single chunk - simple case
        return await synthesize_chunk(text, voice, headers)

    # Multiple chunks - synthesize each and concatenate
    print(f"[TTS] Chunking long text into {len(chunks)} parts")
    audio_parts = []

    for i, chunk in enumerate(chunks):
        print(f"[TTS] Synthesizing chunk {i+1}/{len(chunks)} ({len(chunk)} chars)")
        audio = await synthesize_chunk(chunk, voice, headers)
        if audio is None:
            print(f"[TTS] Chunk {i+1} failed, aborting")
            return None
        audio_parts.append(audio)

    # Concatenate MP3 audio (MP3 files can be simply concatenated)
    print(f"[TTS] Concatenating {len(audio_parts)} audio chunks")
    return b''.join(audio_parts)


async def get_available_voices() -> list[dict]:
    """Fetch available voices from ElevenLabs API."""
    if not settings.elevenlabs_api_key:
        return []

    headers = {
        "xi-api-key": settings.elevenlabs_api_key,
    }

    try:
        async with httpx.AsyncClient() as client:
            response = await client.get(
                "https://api.elevenlabs.io/v1/voices",
                headers=headers,
                timeout=30.0,
            )
            if response.status_code == 200:
                data = response.json()
                voices = []
                for v in data.get("voices", []):
                    voices.append({
                        "voice_id": v["voice_id"],
                        "name": v["name"],
                        "category": v.get("category", ""),
                        "labels": v.get("labels", {}),
                    })
                return voices
            else:
                print(f"Failed to fetch voices: {response.status_code}")
                return []
    except Exception as e:
        print(f"Failed to fetch voices: {e}")
        return []
