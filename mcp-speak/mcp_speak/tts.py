"""Text-to-speech functionality using Groq."""

import os
import tempfile
import subprocess
import platform
from pathlib import Path

# Try to import optional dependencies
try:
    from groq import Groq
    GROQ_AVAILABLE = True
except ImportError:
    GROQ_AVAILABLE = False

try:
    from playsound3 import playsound
    PLAYSOUND_AVAILABLE = True
except ImportError:
    PLAYSOUND_AVAILABLE = False


class TTSEngine:
    """Text-to-speech engine with Groq and system fallbacks."""

    def __init__(self, groq_api_key: str | None = None):
        self.groq_api_key = groq_api_key or os.environ.get("GROQ_API_KEY")
        self.groq_client = None

        if self.groq_api_key and GROQ_AVAILABLE:
            self.groq_client = Groq(api_key=self.groq_api_key)

    def speak(self, text: str, voice: str = "Aria-PlayAI") -> bool:
        """
        Speak the given text.

        Args:
            text: Text to speak
            voice: Voice ID for Groq TTS

        Returns:
            True if successful, False otherwise
        """
        if not text or not text.strip():
            return False

        # Try Groq TTS first (best quality)
        if self.groq_client:
            if self._speak_groq(text, voice):
                return True

        # Fall back to system TTS
        return self._speak_system(text)

    def _speak_groq(self, text: str, voice: str) -> bool:
        """Use Groq TTS API."""
        try:
            # Truncate if too long (Groq has limits)
            if len(text) > 500:
                text = text[:497] + "..."

            response = self.groq_client.audio.speech.create(
                model="playai-tts",
                voice=voice,
                input=text,
                response_format="mp3",
            )

            # Save to temp file and play
            with tempfile.NamedTemporaryFile(suffix=".mp3", delete=False) as f:
                audio_path = f.name
                for chunk in response.iter_bytes():
                    f.write(chunk)

            self._play_audio(audio_path)

            # Clean up
            try:
                os.unlink(audio_path)
            except:
                pass

            return True

        except Exception as e:
            print(f"Groq TTS error: {e}")
            return False

    def _play_audio(self, path: str) -> None:
        """Play an audio file."""
        system = platform.system()

        # Try playsound library first
        if PLAYSOUND_AVAILABLE:
            try:
                playsound(path)
                return
            except:
                pass

        # Fall back to system commands
        try:
            if system == "Darwin":  # macOS
                subprocess.run(["afplay", path], check=True, capture_output=True)
            elif system == "Linux":
                # Try various Linux audio players
                for player in ["mpv", "ffplay", "aplay", "paplay"]:
                    try:
                        if player == "ffplay":
                            subprocess.run([player, "-nodisp", "-autoexit", path],
                                         check=True, capture_output=True)
                        else:
                            subprocess.run([player, path], check=True, capture_output=True)
                        return
                    except FileNotFoundError:
                        continue
            elif system == "Windows":
                # Use PowerShell to play audio
                subprocess.run([
                    "powershell", "-c",
                    f"(New-Object Media.SoundPlayer '{path}').PlaySync()"
                ], check=True, capture_output=True)
        except Exception as e:
            print(f"Audio playback error: {e}")

    def _speak_system(self, text: str) -> bool:
        """Use system TTS as fallback."""
        system = platform.system()

        try:
            if system == "Darwin":  # macOS
                subprocess.run(["say", text], check=True, capture_output=True)
                return True
            elif system == "Linux":
                # Try espeak
                try:
                    subprocess.run(["espeak", text], check=True, capture_output=True)
                    return True
                except FileNotFoundError:
                    # Try festival
                    try:
                        subprocess.run(["festival", "--tts"], input=text.encode(),
                                      check=True, capture_output=True)
                        return True
                    except FileNotFoundError:
                        pass
            elif system == "Windows":
                # Use PowerShell SAPI
                escaped = text.replace("'", "''")
                subprocess.run([
                    "powershell", "-c",
                    f"Add-Type -AssemblyName System.Speech; "
                    f"(New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak('{escaped}')"
                ], check=True, capture_output=True)
                return True
        except Exception as e:
            print(f"System TTS error: {e}")

        return False


# Singleton instance
_engine: TTSEngine | None = None


def get_engine() -> TTSEngine:
    """Get or create the TTS engine singleton."""
    global _engine
    if _engine is None:
        _engine = TTSEngine()
    return _engine


def speak(text: str, voice: str = "Aria-PlayAI") -> bool:
    """Convenience function to speak text."""
    return get_engine().speak(text, voice)
