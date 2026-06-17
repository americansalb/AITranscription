"""Media preparation for transcription.

Groq's Whisper endpoint accepts audio/video but caps each request at ~25 MB, so long
recordings (especially video) must be split before upload. This module uses ffmpeg —
the statically-linked binary bundled by the ``imageio-ffmpeg`` wheel, so no system
ffmpeg / apt / Docker is required on the Render Python runtime — to:

  * strip any video track and downmix to 16 kHz mono (what Whisper wants), and
  * segment the result into fixed-length FLAC chunks small enough for one request.

16 kHz mono FLAC is roughly 1 MB/minute, so a 10-minute chunk is ~10 MB — comfortably
under the limit. Each chunk knows its start offset so segment timestamps can be made
absolute across the whole recording.

If ffmpeg is unavailable for some reason, callers fall back to sending a small,
already-compatible file directly (see studio_transcription).
"""

from __future__ import annotations

import glob
import logging
import os
import shutil
import subprocess
from dataclasses import dataclass

logger = logging.getLogger(__name__)


@dataclass
class AudioChunk:
    """A prepared audio chunk and where it starts within the full recording."""

    path: str
    start_seconds: float


def get_ffmpeg_path() -> str | None:
    """Return a usable ffmpeg executable, preferring the bundled static binary."""
    try:
        import imageio_ffmpeg

        return imageio_ffmpeg.get_ffmpeg_exe()
    except Exception:  # pragma: no cover - only when imageio-ffmpeg is missing
        return shutil.which("ffmpeg")


def ffmpeg_available() -> bool:
    return get_ffmpeg_path() is not None


def split_to_chunks(
    input_path: str,
    out_dir: str,
    chunk_seconds: int = 600,
    sample_rate: int = 16000,
) -> list[AudioChunk]:
    """Transcode ``input_path`` to 16 kHz mono FLAC and split into chunks.

    Returns the chunks in order. Raises RuntimeError if ffmpeg is missing or fails.
    """
    ffmpeg = get_ffmpeg_path()
    if not ffmpeg:
        raise RuntimeError("ffmpeg is not available")

    os.makedirs(out_dir, exist_ok=True)
    pattern = os.path.join(out_dir, "chunk_%05d.flac")

    cmd = [
        ffmpeg,
        "-nostdin",
        "-y",
        "-i", input_path,
        "-vn",                     # drop any video stream
        "-ac", "1",                # mono
        "-ar", str(sample_rate),   # 16 kHz
        "-f", "segment",
        "-segment_time", str(chunk_seconds),
        "-c:a", "flac",
        pattern,
    ]

    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        tail = (proc.stderr or "")[-600:]
        raise RuntimeError(f"ffmpeg failed (exit {proc.returncode}): {tail}")

    files = sorted(glob.glob(os.path.join(out_dir, "chunk_*.flac")))
    if not files:
        raise RuntimeError("ffmpeg produced no audio chunks (unsupported or empty input?)")

    return [
        AudioChunk(path=path, start_seconds=float(i * chunk_seconds))
        for i, path in enumerate(files)
    ]
