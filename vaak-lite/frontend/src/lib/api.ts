/**
 * API client for the Vaak Lite interpretation backend.
 */

const API_BASE = import.meta.env.VITE_API_URL || "/vaaklite/api";

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

export interface InterpretResult {
  source_text: string;
  translated_text: string;
  source_lang: string;
  target_lang: string;
  duration: number | null;
  segments: Segment[];
  provider: string;
  model: string;
}

export interface TranslateResult {
  translated_text: string;
  provider: string;
  model: string;
}

export interface ProviderInfo {
  id: string;
  model: string;
}

/** Full pipeline: transcribe + translate in one call. */
export async function interpret(
  blob: Blob,
  targetLang: string,
  provider: string,
  sourceLang?: string,
): Promise<InterpretResult> {
  const fd = new FormData();
  const ext = blob.type.includes("mp4") ? "mp4" : blob.type.includes("wav") ? "wav" : "webm";
  fd.append("audio", blob, `recording.${ext}`);
  fd.append("target_lang", targetLang);
  fd.append("provider", provider);
  if (sourceLang && sourceLang !== "auto") {
    fd.append("source_lang", sourceLang);
  }

  const res = await fetch(`${API_BASE}/interpret`, { method: "POST", body: fd });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: `HTTP ${res.status}` }));
    throw new Error(err.detail || `Interpretation failed (${res.status})`);
  }
  return res.json();
}

/** Transcribe only (no translation). */
export async function transcribe(blob: Blob, language?: string): Promise<TranscribeResult> {
  const fd = new FormData();
  const ext = blob.type.includes("mp4") ? "mp4" : blob.type.includes("wav") ? "wav" : "webm";
  fd.append("audio", blob, `recording.${ext}`);
  if (language && language !== "auto") fd.append("language", language);

  const res = await fetch(`${API_BASE}/transcribe`, { method: "POST", body: fd });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: `HTTP ${res.status}` }));
    throw new Error(err.detail || `Transcription failed (${res.status})`);
  }
  return res.json();
}

/** Translate text only. */
export async function translateText(
  text: string,
  sourceLang: string,
  targetLang: string,
  provider: string,
): Promise<TranslateResult> {
  const res = await fetch(`${API_BASE}/translate`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ text, source_lang: sourceLang, target_lang: targetLang, provider }),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: `HTTP ${res.status}` }));
    throw new Error(err.detail || `Translation failed (${res.status})`);
  }
  return res.json();
}

/** Get available LLM providers. */
export async function getProviders(): Promise<ProviderInfo[]> {
  const res = await fetch(`${API_BASE}/providers`);
  if (!res.ok) return [];
  const data = await res.json();
  return data.providers || [];
}

/** Health check. */
export async function checkHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${API_BASE}/health`, { signal: AbortSignal.timeout(3000) });
    return res.ok;
  } catch {
    return false;
  }
}
