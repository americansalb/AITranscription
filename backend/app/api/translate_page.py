"""Standalone Spanish translation page.

Serves a self-contained HTML page at GET /translate plus two JSON endpoints:
- GET  /translate/models  -> list of Groq chat models available with the
                             configured GROQ_API_KEY (excludes whisper/tts).
- POST /translate/api     -> { text, model } -> { translated_text, model }

No authentication: this is a public utility page, but it never exposes the
API key to the browser — Groq calls happen server-side using settings.groq_api_key.
"""

from __future__ import annotations

import logging

import httpx
from fastapi import APIRouter, HTTPException
from fastapi.responses import HTMLResponse
from pydantic import BaseModel, Field

from app.core.config import settings

logger = logging.getLogger(__name__)

router = APIRouter()


SYSTEM_PROMPT = (
    "You are a professional Spanish translator. Translate the user's text "
    "into natural, fluent Spanish. Preserve meaning, tone, names, and "
    "technical terms. Output ONLY the Spanish translation — no preamble, "
    "no commentary, no quotation marks around the result."
)


class TranslateRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=20000)
    model: str = Field(..., min_length=1, max_length=200)


class TranslateResponse(BaseModel):
    translated_text: str
    model: str


@router.get("/translate/models")
async def list_groq_models():
    """Return Groq chat models suitable for translation.

    Filters out audio (whisper) and TTS models since they can't do chat completions.
    """
    if not settings.groq_api_key:
        raise HTTPException(status_code=503, detail="GROQ_API_KEY is not configured")

    try:
        async with httpx.AsyncClient(timeout=10.0) as client:
            resp = await client.get(
                "https://api.groq.com/openai/v1/models",
                headers={"Authorization": f"Bearer {settings.groq_api_key}"},
            )
            resp.raise_for_status()
            payload = resp.json()
    except httpx.HTTPError as e:
        logger.error("Groq /models fetch failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=502, detail="Could not reach Groq API")

    raw = payload.get("data", []) or []
    chat_models = []
    for m in raw:
        mid = m.get("id", "")
        if not mid:
            continue
        lower = mid.lower()
        if "whisper" in lower or "tts" in lower or "guard" in lower:
            continue
        chat_models.append({"id": mid, "owned_by": m.get("owned_by", "")})

    chat_models.sort(key=lambda m: m["id"])
    return {"models": chat_models}


@router.post("/translate/api", response_model=TranslateResponse)
async def translate_to_spanish(req: TranslateRequest) -> TranslateResponse:
    """Translate text to Spanish using the chosen Groq model."""
    if not settings.groq_api_key:
        raise HTTPException(status_code=503, detail="GROQ_API_KEY is not configured")

    text = req.text.strip()
    if not text:
        raise HTTPException(status_code=400, detail="Text cannot be empty")

    try:
        from groq import AsyncGroq

        client = AsyncGroq(api_key=settings.groq_api_key)
        response = await client.chat.completions.create(
            model=req.model,
            messages=[
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": text},
            ],
            max_tokens=4096,
            temperature=0.2,
        )
        translated = (response.choices[0].message.content or "").strip()
        return TranslateResponse(translated_text=translated, model=req.model)
    except Exception as e:
        logger.error("Groq translation failed: %s: %s", type(e).__name__, e)
        msg = str(e)
        if "model" in msg.lower() and ("not found" in msg.lower() or "decommissioned" in msg.lower()):
            raise HTTPException(status_code=400, detail=f"Model not available: {req.model}")
        raise HTTPException(status_code=502, detail="Translation service unavailable")


# ---------------------------------------------------------------------------
# Self-contained HTML page
# ---------------------------------------------------------------------------

_PAGE_HTML = r"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Spanish Translator (Groq)</title>
<style>
  :root {
    --bg: #0f1115;
    --panel: #181b22;
    --border: #2a2e38;
    --text: #e6e7eb;
    --muted: #9aa0aa;
    --accent: #4f8cff;
    --accent-hover: #3a78ee;
    --error: #ff6b6b;
    --success: #4ade80;
  }
  * { box-sizing: border-box; }
  html, body {
    margin: 0; padding: 0;
    background: var(--bg); color: var(--text);
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    line-height: 1.5;
  }
  .wrap {
    max-width: 1100px;
    margin: 0 auto;
    padding: 32px 24px 64px;
  }
  h1 {
    margin: 0 0 4px; font-size: 28px; font-weight: 600;
  }
  .subtitle {
    margin: 0 0 24px; color: var(--muted); font-size: 14px;
  }
  .controls {
    display: flex; gap: 12px; align-items: center;
    margin-bottom: 16px; flex-wrap: wrap;
  }
  label { font-size: 14px; color: var(--muted); }
  select, button, input[type="text"] {
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 14px;
    font-family: inherit;
  }
  select { min-width: 280px; }
  input[type="text"] { min-width: 220px; }
  input[type="text"]:focus, select:focus {
    outline: 2px solid var(--accent); outline-offset: -1px;
  }
  .custom-label { margin-left: 8px; }
  .hint {
    margin: 0 0 16px; color: var(--muted); font-size: 12px; max-width: 760px;
  }
  button {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    cursor: pointer;
    font-weight: 500;
    padding: 10px 20px;
  }
  button:hover:not(:disabled) { background: var(--accent-hover); }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }
  @media (max-width: 720px) {
    .grid { grid-template-columns: 1fr; }
  }
  .panel {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 16px;
    display: flex; flex-direction: column;
  }
  .panel h2 {
    margin: 0 0 8px; font-size: 14px;
    color: var(--muted); font-weight: 500;
    text-transform: uppercase; letter-spacing: 0.04em;
  }
  textarea {
    width: 100%;
    min-height: 280px;
    background: #0b0d12;
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 12px;
    font-size: 15px;
    font-family: inherit;
    resize: vertical;
  }
  textarea:focus { outline: 2px solid var(--accent); outline-offset: -1px; }
  textarea[readonly] { background: #0b0d12; cursor: text; }
  .status {
    margin-top: 12px; min-height: 20px; font-size: 13px;
  }
  .status.error { color: var(--error); }
  .status.success { color: var(--success); }
  .meta { font-size: 12px; color: var(--muted); margin-top: 8px; }
  .copy-btn {
    background: transparent;
    color: var(--muted);
    border: 1px solid var(--border);
    padding: 4px 10px;
    font-size: 12px;
    align-self: flex-start;
    margin-top: 8px;
  }
  .copy-btn:hover:not(:disabled) {
    background: var(--border); color: var(--text);
  }
</style>
</head>
<body>
<div class="wrap">
  <h1>Translate to Spanish</h1>
  <p class="subtitle">Powered by Groq. Pick any chat model your API key supports.</p>

  <div class="controls">
    <label for="model">Model:</label>
    <select id="model" disabled>
      <option>Loading models…</option>
    </select>
    <label for="custom-model" class="custom-label">Or custom ID:</label>
    <input type="text" id="custom-model" placeholder="e.g. llama3-70b-8192" autocomplete="off" />
    <button id="translate-btn" disabled>Translate</button>
  </div>
  <p class="hint">
    The dropdown lists every chat model your Groq API key currently supports
    (whisper / TTS / guard models are excluded since they can't do chat).
    To use a model not in the list, type its exact Groq ID into the
    "custom ID" field — it overrides the dropdown.
  </p>

  <div class="grid">
    <div class="panel">
      <h2>English (or any source language)</h2>
      <textarea id="input" placeholder="Paste or type the text you want translated to Spanish…"></textarea>
    </div>
    <div class="panel">
      <h2>Spanish</h2>
      <textarea id="output" readonly placeholder="Translation will appear here…"></textarea>
      <button class="copy-btn" id="copy-btn" disabled>Copy</button>
      <div class="meta" id="meta"></div>
    </div>
  </div>

  <div class="status" id="status"></div>
</div>

<script>
(function () {
  const modelSelect = document.getElementById("model");
  const customModelEl = document.getElementById("custom-model");
  const inputEl = document.getElementById("input");
  const outputEl = document.getElementById("output");
  const btn = document.getElementById("translate-btn");
  const copyBtn = document.getElementById("copy-btn");
  const statusEl = document.getElementById("status");
  const metaEl = document.getElementById("meta");

  function chosenModel() {
    const custom = (customModelEl.value || "").trim();
    return custom || modelSelect.value;
  }

  // Preferred default models in priority order.
  const PREFERRED = [
    "llama-3.3-70b-versatile",
    "llama-3.1-70b-versatile",
    "llama-3.1-8b-instant",
    "llama3-70b-8192",
  ];

  function setStatus(msg, kind) {
    statusEl.textContent = msg || "";
    statusEl.className = "status" + (kind ? " " + kind : "");
  }

  async function loadModels() {
    try {
      const resp = await fetch("/translate/models");
      if (!resp.ok) {
        const err = await resp.json().catch(() => ({}));
        throw new Error(err.detail || ("HTTP " + resp.status));
      }
      const data = await resp.json();
      const models = data.models || [];
      if (models.length === 0) {
        modelSelect.innerHTML = '<option value="">No models available</option>';
        setStatus("No chat models returned by Groq.", "error");
        return;
      }
      modelSelect.innerHTML = "";
      for (const m of models) {
        const opt = document.createElement("option");
        opt.value = m.id;
        opt.textContent = m.id + (m.owned_by ? "  (" + m.owned_by + ")" : "");
        modelSelect.appendChild(opt);
      }
      // Pick a sensible default.
      const ids = models.map(m => m.id);
      const preferred = PREFERRED.find(p => ids.includes(p));
      modelSelect.value = preferred || ids[0];
      modelSelect.disabled = false;
      btn.disabled = false;
      setStatus("");
    } catch (e) {
      modelSelect.innerHTML = '<option value="">Failed to load</option>';
      setStatus("Could not load models: " + e.message, "error");
    }
  }

  async function translate() {
    const text = inputEl.value.trim();
    if (!text) {
      setStatus("Please enter some text to translate.", "error");
      return;
    }
    const model = chosenModel();
    if (!model) {
      setStatus("No model selected.", "error");
      return;
    }
    btn.disabled = true;
    copyBtn.disabled = true;
    outputEl.value = "";
    metaEl.textContent = "";
    setStatus("Translating with " + model + "…");
    const t0 = performance.now();
    try {
      const resp = await fetch("/translate/api", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text, model }),
      });
      if (!resp.ok) {
        const err = await resp.json().catch(() => ({}));
        throw new Error(err.detail || ("HTTP " + resp.status));
      }
      const data = await resp.json();
      outputEl.value = data.translated_text || "";
      const ms = Math.round(performance.now() - t0);
      metaEl.textContent = "Model: " + data.model + " · " + ms + " ms";
      setStatus("Done.", "success");
      copyBtn.disabled = !outputEl.value;
    } catch (e) {
      setStatus("Translation failed: " + e.message, "error");
    } finally {
      btn.disabled = false;
    }
  }

  btn.addEventListener("click", translate);
  inputEl.addEventListener("keydown", function (e) {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      translate();
    }
  });
  copyBtn.addEventListener("click", async function () {
    if (!outputEl.value) return;
    try {
      await navigator.clipboard.writeText(outputEl.value);
      copyBtn.textContent = "Copied!";
      setTimeout(() => { copyBtn.textContent = "Copy"; }, 1500);
    } catch (e) {
      outputEl.select();
    }
  });

  loadModels();
})();
</script>
</body>
</html>
"""


@router.get("/translate", response_class=HTMLResponse)
async def translate_page() -> HTMLResponse:
    """Serve the standalone Spanish translator HTML page."""
    return HTMLResponse(content=_PAGE_HTML)
