"""ElevenLabs Text-to-Speech service."""

import httpx
from app.core.config import settings


async def synthesize(text: str, voice_id: str | None = None) -> bytes | None:
    """Convert text to speech using ElevenLabs API.

    Returns MP3 audio bytes or None if failed.
    """
    if not settings.elevenlabs_api_key:
        return None

    voice = voice_id or settings.elevenlabs_voice_id
    url = f"https://api.elevenlabs.io/v1/text-to-speech/{voice}"

    headers = {
        "Accept": "audio/mpeg",
        "Content-Type": "application/json",
        "xi-api-key": settings.elevenlabs_api_key,
    }

    data = {
        "text": text,
        "model_id": "eleven_monolingual_v1",
        "voice_settings": {
            "stability": 0.5,
            "similarity_boost": 0.75,
        }
    }

    try:
        async with httpx.AsyncClient() as client:
            response = await client.post(url, json=data, headers=headers, timeout=30.0)

            if response.status_code == 200:
                return response.content
            else:
                print(f"ElevenLabs API error: {response.status_code} - {response.text}")
                return None
    except Exception as e:
        print(f"ElevenLabs TTS error: {e}")
        return None
