"""Serves the self-contained Transcription Studio web UI at GET /studio.

Single HTML document with inline CSS/JS (no build step), talking to the JSON API under
/api/v1/studio. Mirrors the pattern used by translate_page.py.
"""

from fastapi import APIRouter
from fastapi.responses import HTMLResponse

router = APIRouter()

STUDIO_HTML = r"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Transcription Studio</title>
<style>
  :root {
    --bg: #f5f3ee; --panel: #ffffff; --ink: #1f2421; --muted: #6b7280;
    --accent: #b5562f; --accent-d: #8f421f; --line: #e3ded3; --ok: #1f7a4d;
    --warn: #9a6700; --err: #b3261e; --chip: #efece4;
  }
  * { box-sizing: border-box; }
  body { margin: 0; background: var(--bg); color: var(--ink);
    font: 16px/1.5 system-ui, -apple-system, Segoe UI, Roboto, sans-serif; }
  header { padding: 22px 24px; border-bottom: 1px solid var(--line); background: var(--panel); }
  header h1 { margin: 0; font-size: 22px; }
  header p { margin: 4px 0 0; color: var(--muted); font-size: 14px; }
  main { max-width: 980px; margin: 0 auto; padding: 24px 16px 80px; }
  .panel { background: var(--panel); border: 1px solid var(--line); border-radius: 10px;
    padding: 18px; margin-bottom: 22px; }
  h2 { font-size: 17px; margin: 0 0 12px; }
  button { font: inherit; cursor: pointer; border: 1px solid var(--accent); background: var(--accent);
    color: #fff; padding: 8px 14px; border-radius: 7px; }
  button:hover { background: var(--accent-d); border-color: var(--accent-d); }
  button.secondary { background: transparent; color: var(--accent); }
  button:disabled { opacity: .5; cursor: not-allowed; }
  input[type=text], input[type=password], textarea {
    font: inherit; width: 100%; padding: 9px 11px; border: 1px solid var(--line);
    border-radius: 7px; background: #fff; color: var(--ink); }
  textarea { resize: vertical; min-height: 64px; }
  label { font-size: 13px; color: var(--muted); display: block; margin-bottom: 4px; }
  .row { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }
  .grow { flex: 1; min-width: 180px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: 9px 8px; border-bottom: 1px solid var(--line);
    font-size: 14px; vertical-align: top; }
  th { color: var(--muted); font-weight: 600; }
  .chip { display: inline-block; padding: 2px 9px; border-radius: 999px; font-size: 12px;
    background: var(--chip); }
  .chip.queued { background: #eef; color: #334; }
  .chip.processing { background: #fdf0d8; color: var(--warn); }
  .chip.completed { background: #e3f3ea; color: var(--ok); }
  .chip.failed { background: #fde7e6; color: var(--err); }
  .drop { border: 2px dashed var(--line); border-radius: 10px; padding: 22px; text-align: center;
    color: var(--muted); transition: border-color .15s, background .15s; }
  .drop.over { border-color: var(--accent); background: #faf6f1; }
  .muted { color: var(--muted); font-size: 13px; }
  .src { border-left: 3px solid var(--line); padding: 6px 10px; margin: 8px 0; background: #faf8f3; }
  .src .meta { font-size: 12px; color: var(--muted); }
  .banner { padding: 10px 12px; border-radius: 8px; margin-bottom: 12px; font-size: 14px; }
  .banner.warn { background: #fdf0d8; color: var(--warn); }
  .answer { white-space: pre-wrap; }
  .actions button { padding: 4px 9px; font-size: 13px; margin-right: 4px; }
  pre.transcript { white-space: pre-wrap; max-height: 360px; overflow: auto; background: #faf8f3;
    padding: 12px; border-radius: 8px; border: 1px solid var(--line); }
  a.link { color: var(--accent); cursor: pointer; }
</style>
</head>
<body>
<header>
  <h1>Transcription Studio</h1>
  <p>Bulk-transcribe audio &amp; video with Groq Whisper, then search and ask questions with Claude Haiku.</p>
</header>
<main>
  <div id="banners"></div>

  <div class="panel" id="tokenPanel" style="display:none">
    <h2>Access token</h2>
    <div class="row">
      <div class="grow"><input type="password" id="token" placeholder="Enter access token" /></div>
      <button onclick="saveToken()">Save</button>
    </div>
  </div>

  <div class="panel">
    <h2>Upload media</h2>
    <div class="drop" id="drop">
      Drag &amp; drop files here, or
      <label for="fileInput" class="link" style="display:inline">browse</label>.
      <input type="file" id="fileInput" multiple accept="audio/*,video/*" style="display:none" />
      <div class="muted" id="dropHint" style="margin-top:8px"></div>
    </div>
    <div class="row" style="margin-top:12px">
      <button id="uploadBtn" onclick="uploadFiles()" disabled>Upload &amp; transcribe</button>
      <span class="muted" id="selectedInfo"></span>
    </div>
    <div id="uploadMsg" class="muted" style="margin-top:8px" role="status" aria-live="polite"></div>
  </div>

  <div class="panel">
    <h2>Library</h2>
    <div id="jobs"><p class="muted">Loading…</p></div>
  </div>

  <div class="panel">
    <h2>Search transcripts</h2>
    <div class="row">
      <div class="grow"><input type="text" id="searchInput" placeholder="Keyword search across all transcripts"
        onkeydown="if(event.key==='Enter')runSearch()" /></div>
      <button onclick="runSearch()">Search</button>
    </div>
    <div id="searchResults" style="margin-top:12px"></div>
  </div>

  <div class="panel">
    <h2>Ask a question</h2>
    <label for="askInput">Claude Haiku answers from your transcripts, with citations.</label>
    <textarea id="askInput" placeholder="e.g. What did the speaker say about pricing?"></textarea>
    <div class="row" style="margin-top:10px">
      <button id="askBtn" onclick="runAsk()">Ask</button>
      <span class="muted" id="askStatus" role="status" aria-live="polite"></span>
    </div>
    <div id="answer" style="margin-top:14px"></div>
  </div>

  <div class="panel" id="viewerPanel" style="display:none">
    <h2 id="viewerTitle">Transcript</h2>
    <pre class="transcript" id="viewerBody"></pre>
  </div>
</main>

<script>
const API = "/api/v1/studio";
let selectedFiles = [];
let config = {};

function getToken() { return localStorage.getItem("studioToken") || ""; }
function saveToken() {
  localStorage.setItem("studioToken", document.getElementById("token").value.trim());
  loadConfig(); refreshJobs();
}
function headers(extra) {
  const h = extra || {};
  const t = getToken();
  if (t) h["X-Studio-Token"] = t;
  return h;
}
function withToken(url) {
  const t = getToken();
  if (!t) return url;
  return url + (url.includes("?") ? "&" : "?") + "token=" + encodeURIComponent(t);
}
function esc(s) {
  return (s == null ? "" : String(s)).replace(/[&<>"]/g, c =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
}
function fmtDur(s) {
  if (s == null) return "—";
  s = Math.round(s); const m = Math.floor(s / 60), sec = s % 60;
  return m + ":" + String(sec).padStart(2, "0");
}

async function loadConfig() {
  try {
    const r = await fetch(API + "/config");
    config = await r.json();
  } catch (e) { config = {}; }
  document.getElementById("tokenPanel").style.display = config.access_required ? "" : "none";
  const b = [];
  if (config.access_required && !getToken())
    b.push(['warn', 'This studio requires an access token. Enter it above to continue.']);
  if (config.groq_configured === false)
    b.push(['warn', 'GROQ_API_KEY is not configured — transcription will fail until it is set.']);
  if (config.anthropic_configured === false)
    b.push(['warn', 'ANTHROPIC_API_KEY is not configured — question answering is disabled.']);
  if (config.ffmpeg_available === false)
    b.push(['warn', 'ffmpeg unavailable — only files under ' + (config.max_upload_mb||'') + 'MB in compatible formats can be transcribed (no large-file/video chunking).']);
  document.getElementById("banners").innerHTML =
    b.map(x => '<div class="banner ' + x[0] + '">' + esc(x[1]) + '</div>').join("");
  document.getElementById("dropHint").textContent =
    "Accepted: " + (config.allowed_extensions || []).join(" ");
}

// ---- upload ----
const drop = document.getElementById("drop");
const fileInput = document.getElementById("fileInput");
["dragover", "dragenter"].forEach(ev => drop.addEventListener(ev, e => {
  e.preventDefault(); drop.classList.add("over"); }));
["dragleave", "drop"].forEach(ev => drop.addEventListener(ev, e => {
  e.preventDefault(); drop.classList.remove("over"); }));
drop.addEventListener("drop", e => setFiles(e.dataTransfer.files));
fileInput.addEventListener("change", () => setFiles(fileInput.files));

function setFiles(list) {
  selectedFiles = Array.from(list || []);
  document.getElementById("uploadBtn").disabled = selectedFiles.length === 0;
  document.getElementById("selectedInfo").textContent =
    selectedFiles.length ? (selectedFiles.length + " file(s) selected") : "";
}

async function uploadFiles() {
  if (!selectedFiles.length) return;
  const fd = new FormData();
  selectedFiles.forEach(f => fd.append("files", f));
  const btn = document.getElementById("uploadBtn");
  const msg = document.getElementById("uploadMsg");
  btn.disabled = true; msg.textContent = "Uploading…";
  try {
    const r = await fetch(API + "/jobs", { method: "POST", headers: headers(), body: fd });
    const data = await r.json();
    if (!r.ok) { msg.textContent = "Error: " + (data.detail ? JSON.stringify(data.detail) : r.status); }
    else {
      const made = (data.created || []).length;
      const errs = (data.errors || []).map(e => e.filename + ": " + e.error);
      msg.textContent = "Queued " + made + " file(s)." + (errs.length ? " Skipped — " + errs.join("; ") : "");
      selectedFiles = []; fileInput.value = ""; setFiles([]);
    }
  } catch (e) { msg.textContent = "Upload failed: " + e; }
  btn.disabled = selectedFiles.length === 0;
  refreshJobs();
}

// ---- jobs ----
async function refreshJobs() {
  let data;
  try {
    const r = await fetch(API + "/jobs", { headers: headers() });
    if (!r.ok) { document.getElementById("jobs").innerHTML =
      '<p class="muted">Could not load library (' + r.status + ').</p>'; return; }
    data = await r.json();
  } catch (e) { return; }
  const jobs = data.jobs || [];
  if (!jobs.length) { document.getElementById("jobs").innerHTML =
    '<p class="muted">No media yet. Upload some files to get started.</p>'; return; }
  let html = '<table><thead><tr><th>File</th><th>Status</th><th>Length</th><th>Words</th><th>Actions</th></tr></thead><tbody>';
  for (const j of jobs) {
    const st = '<span class="chip ' + j.status + '">' + j.status + '</span>'
      + (j.status === 'failed' && j.error ? '<div class="muted">' + esc(j.error) + '</div>' : '');
    let actions = '';
    if (j.has_transcript) {
      actions += '<button class="secondary" onclick="viewJob(' + j.id + ')">View</button>';
      actions += '<a class="link" href="' + withToken(API + "/jobs/" + j.id + "/transcript?format=txt") + '">TXT</a> ';
      actions += '<a class="link" href="' + withToken(API + "/jobs/" + j.id + "/transcript?format=srt") + '">SRT</a> ';
    }
    actions += '<button class="secondary" onclick="deleteJob(' + j.id + ')">Delete</button>';
    html += '<tr><td>' + esc(j.filename) + '</td><td>' + st + '</td><td>' + fmtDur(j.duration_seconds)
      + '</td><td>' + (j.word_count || 0) + '</td><td class="actions">' + actions + '</td></tr>';
  }
  html += '</tbody></table>';
  document.getElementById("jobs").innerHTML = html;
}

async function viewJob(id) {
  const r = await fetch(API + "/jobs/" + id, { headers: headers() });
  const j = await r.json();
  document.getElementById("viewerPanel").style.display = "";
  document.getElementById("viewerTitle").textContent = "Transcript — " + j.filename;
  document.getElementById("viewerBody").textContent = j.transcript || "(empty)";
  document.getElementById("viewerPanel").scrollIntoView({ behavior: "smooth" });
}

async function deleteJob(id) {
  if (!confirm("Delete this transcript?")) return;
  await fetch(API + "/jobs/" + id, { method: "DELETE", headers: headers() });
  refreshJobs();
}

// ---- search ----
async function runSearch() {
  const q = document.getElementById("searchInput").value.trim();
  const box = document.getElementById("searchResults");
  if (!q) { box.innerHTML = ""; return; }
  box.innerHTML = '<p class="muted">Searching…</p>';
  const r = await fetch(API + "/search?q=" + encodeURIComponent(q), { headers: headers() });
  const data = await r.json();
  const res = data.results || [];
  if (!res.length) { box.innerHTML = '<p class="muted">No matches.</p>'; return; }
  box.innerHTML = res.map(s =>
    '<div class="src"><div class="meta">' + esc(s.filename) + ' @ ' + esc(s.timestamp)
    + ' · <a class="link" onclick="viewJob(' + s.media_id + ')">view</a></div>'
    + esc(s.text) + '</div>').join("");
}

// ---- ask ----
async function runAsk() {
  const q = document.getElementById("askInput").value.trim();
  if (!q) return;
  const btn = document.getElementById("askBtn");
  const status = document.getElementById("askStatus");
  const out = document.getElementById("answer");
  btn.disabled = true; status.textContent = "Thinking…"; out.innerHTML = "";
  try {
    const r = await fetch(API + "/ask", {
      method: "POST", headers: headers({ "Content-Type": "application/json" }),
      body: JSON.stringify({ question: q }) });
    const data = await r.json();
    if (!r.ok) { out.innerHTML = '<div class="banner warn">' + esc(data.detail || r.status) + '</div>'; }
    else {
      let html = '<div class="answer">' + esc(data.answer) + '</div>';
      if ((data.sources || []).length) {
        html += '<h2 style="margin-top:16px;font-size:15px">Sources</h2>';
        html += data.sources.map((s, i) =>
          '<div class="src"><div class="meta">[' + (i + 1) + '] ' + esc(s.filename) + ' @ ' + esc(s.timestamp)
          + ' · <a class="link" onclick="viewJob(' + s.media_id + ')">view</a></div>'
          + esc(s.text) + '</div>').join("");
      }
      out.innerHTML = html;
    }
  } catch (e) { out.innerHTML = '<div class="banner warn">Request failed: ' + esc(e) + '</div>'; }
  status.textContent = ""; btn.disabled = false;
}

// ---- init + polling ----
loadConfig();
refreshJobs();
setInterval(() => { if (!document.hidden) refreshJobs(); }, 5000);
const savedToken = getToken();
if (savedToken) { const el = document.getElementById("token"); if (el) el.value = savedToken; }
</script>
</body>
</html>
"""


@router.get("/studio", include_in_schema=False)
async def studio_page() -> HTMLResponse:
    return HTMLResponse(content=STUDIO_HTML)
