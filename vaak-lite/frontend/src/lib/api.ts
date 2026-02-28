/**
 * API client for the Vaak Lite backend.
 */

const API_BASE = import.meta.env.VITE_API_URL || "http://127.0.0.1:19837";

export interface TranscribeResult {
  text: string;
  duration: number | null;
  language: string | null;
  segments: Segment[];
}

export interface Segment {
  start: number;
  end: number;
  text: string;
}

/**
 * Transcribe an audio blob.
 *
 * @param blob  Audio data (webm, wav, mp4, etc.)
 * @param language  ISO language code or "auto" / undefined for auto-detect
 */
export async function transcribe(
  blob: Blob,
  language?: string
): Promise<TranscribeResult> {
  const fd = new FormData();
  const ext = blob.type.includes("mp4")
    ? "mp4"
    : blob.type.includes("wav")
      ? "wav"
      : "webm";
  fd.append("audio", blob, `recording.${ext}`);

  if (language && language !== "auto") {
    fd.append("language", language);
  }

  const res = await fetch(`${API_BASE}/transcribe`, {
    method: "POST",
    body: fd,
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: `HTTP ${res.status}` }));
    throw new Error(err.detail || `Transcription failed (${res.status})`);
  }

  return res.json();
}

/** Quick health check — resolves true if the backend is reachable. */
export async function checkHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${API_BASE}/health`, { signal: AbortSignal.timeout(3000) });
    return res.ok;
  } catch {
    return false;
  }
}
