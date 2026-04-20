# Vaak - Architectural Vision

**Last Updated:** 2026-04-19
**Architect:** Claude (Architect role, instance 0)

---

## 1. Project Purpose & Goals

Vaak is an AI-powered voice transcription desktop application designed as an **accessibility-first** tool. It records speech, transcribes via Groq Whisper, polishes text with Claude Haiku, and injects it into the user's active window. A key secondary purpose is serving as a **voice interface for Claude Code** — allowing spoken output from AI coding sessions to be organized, queued, and read aloud via ElevenLabs TTS.

The application targets users who:
- Have physical disabilities that make typing difficult
- Are visually impaired (screen reader + blind mode features)
- Want voice-first interaction with AI coding tools
- Need fast, context-aware transcription (email, slack, code, documents)

## 2. High-Level Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Tauri Desktop Shell (Rust)            │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐  │
│  │  Main    │ │Transcript│ │ Screen   │ │ Overlay   │  │
│  │  Window  │ │  Window  │ │ Reader   │ │ (Float)   │  │
│  │ (React)  │ │ (React)  │ │ (React)  │ │ (React)   │  │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └─────┬─────┘  │
│       │             │            │              │        │
│  ┌────┴─────────────┴────────────┴──────────────┴────┐  │
│  │        Tauri IPC (events + commands)               │  │
│  └──────────────────────┬────────────────────────────┘  │
│                         │                                │
│  ┌──────────────────────┴────────────────────────────┐  │
│  │  Native Rust: Audio (CPAL), Keyboard (enigo),     │  │
│  │  Screen capture, UIA/AX, Focus tracker, SQLite,   │  │
│  │  HTTP server (:7865), MCP sidecar management      │  │
│  └──────────────────────┬────────────────────────────┘  │
└─────────────────────────┼───────────────────────────────┘
                          │
            ┌─────────────┴─────────────┐
            │   FastAPI Backend (:19836) │
            │   ┌─────────────────────┐ │
            │   │  API Routes (30+)   │ │
            │   │  Services (12+)     │ │
            │   │  Models (ORM)       │ │
            │   │  Gamification       │ │
            │   │  Learning/Feedback  │ │
            │   └─────────┬───────────┘ │
            └─────────────┼─────────────┘
                          │
     ┌────────────┬───────┼────────┬──────────────┐
     │            │       │        │              │
  ┌──┴──┐   ┌───┴──┐ ┌──┴───┐ ┌──┴──────┐  ┌───┴────┐
  │Groq │   │Claude│ │Eleven│ │PostgreSQL│  │pgvector│
  │Whisper│  │Haiku │ │Labs  │ │   DB    │  │embeddings│
  └──────┘  └──────┘ └──────┘ └─────────┘  └────────┘

              ┌─────────────────┐
              │  MCP Sidecar    │
              │  (vaak-mcp.rs)  │
              │  Claude Code <->│
              │  Vaak speak +   │
              │  Team Collab    │
              └─────────────────┘
```

### Component Boundaries

| Layer | Boundary | Protocol |
|-------|----------|----------|
| Frontend <-> Native | Tauri IPC commands + events | JSON serialized |
| Frontend <-> Backend | HTTP REST + SSE | JSON, FormData, SSE |
| Native <-> Backend | HTTP (ureq) | JSON |
| Claude Code <-> Vaak | MCP stdio + HTTP :7865 | JSON-RPC + REST |
| Backend <-> External AI | HTTPS API calls | Groq, Anthropic, ElevenLabs SDKs |
| Backend <-> Database | async SQLAlchemy | asyncpg driver |

### Web Service Architecture (Added 2026-02-24)

A new **web service** (`web-service/`) mirrors the desktop collab tab for browser-based access. Separate from the desktop backend.

```
┌─────────────────────────────────────────────────┐
│        Web Client SPA (web-client/)              │
│  React 18 + Zustand + WebSocket                 │
│  Pages: Login, Dashboard, Project, Billing       │
│  23 files, 4546 lines, Vite build               │
└──────────────────┬──────────────────────────────┘
                   │ HTTPS + WebSocket
┌──────────────────┴──────────────────────────────┐
│      Vaak Web Service (web-service/app/)         │
│  FastAPI + async SQLAlchemy 2.0 + asyncpg        │
│  40 routes: auth, projects, messages, billing,   │
│  providers, discussions                          │
│  8 DB tables, Alembic migrations                 │
│  JWT auth, Stripe billing, rate limiting         │
└──────┬──────────┬──────────┬────────────────────┘
       │          │          │
  ┌────┴───┐ ┌───┴───┐ ┌───┴────┐
  │LiteLLM │ │Stripe │ │Postgres│
  │(proxy) │ │billing│ │  + PG  │
  │Claude/ │ │       │ │        │
  │GPT/    │ │       │ │        │
  │Gemini  │ │       │ │        │
  └────────┘ └───────┘ └────────┘
```

**Key differences from desktop backend:**
- PostgreSQL (not SQLite) for horizontal scaling
- LiteLLM multi-provider proxy (not direct Groq/Anthropic calls)
- WebSocket real-time messaging (not file-based JSONL polling)
- Stripe subscription billing (Free 50K/Pro 2M/BYOK unlimited tokens/month)
- Server-side agent runtime (async task loops per role, not MCP sidecar)
- Discussion system via DB (not .vaak/ files): Delphi, Oxford, Continuous with auto-trigger
- Fernet encryption for BYOK API keys at rest
- Per-session ($50/day) and per-message ($5) cost ceilings
- Atomic SQL usage counter updates (no TOCTOU race)

**Web client** (web-client/): 26 files, 5000 lines, React 18 + Zustand + Vite.
6 pages, 20+ ProjectPage features, WebSocket real-time, dark/light theme, WCAG AA.

**Test suite:** 90+ tests, all passing (~17s runtime). Covers auth, projects, messages,
discussions, billing, providers, agent runtime, and end-to-end integration flows.

**API routes:** 51 total (46 API + health + docs/openapi).

**Agent runtime:** Fully operational — poll DB for messages, build context with history,
call LiteLLM via metered proxy, parse structured responses (TO/TYPE/SUBJECT headers),
post to DB + broadcast via WebSocket. Supports multi-message output (===MSG=== separator).
Enforces monthly token limits, per-session budgets, and BYOK key lookup per agent completion.

## 3. Design Principles

### 3.1 Accessibility-First
The entire system is designed around voice input/output. The no-censorship policy in the polish service system prompt reflects its role as an assistive tool — it must faithfully transcribe regardless of content.

### 3.2 Graceful Degradation
- Native Tauri CPAL audio recording with browser MediaRecorder fallback
- Local fine-tuned Whisper with Groq API fallback
- ML corrections disabled by default, rule-based always available
- TTS returns empty if ElevenLabs unconfigured
- Auth optional on core transcription endpoints

### 3.3 Multi-Window Isolation
Each window is an independent React app routed by URL hash. Cross-window communication uses Tauri events (not shared state), keeping windows decoupled.

### 3.4 Non-Blocking Side Effects
Gamification (XP, achievements) runs as non-blocking background tasks after transcription. Failures are logged but never break the core pipeline.

### 3.5 Platform-Aware Native Layer
Rust code uses conditional compilation (`#[cfg(target_os)]`) throughout for cross-platform support: keyboard simulation, screen capture, accessibility APIs, focus tracking, error dialogs.

## 4. Key Architectural Decisions

| Decision | Rationale | Trade-off |
|----------|-----------|-----------|
| Tauri over Electron | Smaller binary, native performance, Rust safety | Smaller ecosystem, harder native integrations |
| FastAPI (Python) over Node.js | ML ecosystem (PyTorch, transformers, pgvector) | Separate process from desktop shell |
| Hash-based window routing | Simplest way to serve multiple independent React apps from one build | No shared router, each window bootstraps independently |
| SQLite for queue, PostgreSQL for data | Queue needs local-first offline persistence; user data needs server | Two database systems to maintain |
| Speaker lock (5s timeout) | Prevents overlapping TTS from multiple sessions | Could miss legitimate rapid-fire messages |
| CPAL audio over browser API | Lower latency, better device control, WAV format | Platform-specific builds, more complex |
| pgvector for corrections | Semantic similarity search without external vector DB | Requires PostgreSQL extension, adds schema complexity |
| File-based collab (JSONL) | Simple, inspectable, version-controllable team state | No ACID, advisory locking only, race conditions |
| MCP sidecar as separate binary | Process isolation from desktop, stdio JSON-RPC | Two code paths for same state (discussion writes) |

## 5. macOS Parity Assessment (2026-02-23 Full Audit)

### Current State: Windows works well, macOS has 8 critical/high gaps.

### CRITICAL (Blocking macOS Usage)

| # | Gap | Layer | Files | Impact |
|---|-----|-------|-------|--------|
| 0 | **Missing macOS MCP sidecar binaries** | Build | `desktop/src-tauri/binaries/` | Only Windows `.exe` exists. Missing `vaak-mcp-aarch64-apple-darwin` and `vaak-mcp-x86_64-apple-darwin`. Without these: no Claude Code integration, no speak, no collab, no hooks on macOS. Blocks ALL macOS testing. |
| 1 | **Focus tracking uses legacy stub on macOS** | Rust | `main.rs:1511`, `focus_tracker.rs` | `set_focus_tracking` calls `focus_tracker::start_focus_tracking()` which STUBS on macOS. `a11y::start_focus_tracking()` has working AXObserver. One-line fix. |
| 2 | **Frontend blocks macOS from a11y features** | Frontend | `ScreenReaderApp.tsx:170-209` | Both UI Automation Tree and Focus Tracking buttons hardcode `isWindows()` guards. Backend supports macOS via `capture_macos.rs`. Change to `isWindows() or isMacOS()`. |
| 3 | **AX coordinate flip not implemented** | Rust | `a11y/capture_macos.rs`, `a11y/types.rs` | Screen reader reports inverted Y positions. types.rs documents `y = screen_height - ax_y - height` as MUST but capture_macos.rs doesn't implement it. |
| 4 | **Recording overlay disabled on macOS** | Frontend+Rust | `App.tsx:828-861`, `OverlayApp.tsx` | `isMacOS() return` skips overlay entirely. No visual recording indicator. |
| 5 | **GPU training fails on macOS** | Python | `whisper_finetuner.py:117`, `correction_trainer.py` | CUDA-only check raises RuntimeError. No Metal (MPS) fallback for Apple Silicon. |
| 6 | **Launch button fix pending** | Frontend | `CollabTab.tsx:3003,3243` | UX-engineer fixed — CLI Missing banner + Open Terminal button. Pending commit. |

### HIGH (Degraded macOS Experience)

| # | Gap | Layer | Files | Impact |
|---|-----|-------|-------|--------|
| 7 | **Agent kill/buzz via osascript fragile** | Rust | `launcher.rs:643-964` | Requires Automation permission. TTY detection can fail. |
| 8 | **UIA toggle label confusing on macOS** | Frontend | `ScreenReaderApp.tsx:170` | Shows "(Windows only)" but macOS AX tree capture works. Should say "Accessibility Tree". |
| 9 | **Legacy focus_tracker.rs should be deleted** | Rust | `focus_tracker.rs` | After Fix 1, this module is dead code. Delete it and `a11y/focus_windows.rs` delegation. |
| 10 | **File permissions not set explicitly** | Python | `audio_collector.py`, training modules | Created dirs may be world-readable on macOS (default umask 022). |

### WORKING WELL (No Action Needed)

- Audio recording — cpal cross-platform abstraction (excellent)
- Keyboard shortcuts — `CommandOrControl` + `getModifierKeyName()` (excellent)
- Clipboard/paste — platform-abstracted to Rust backend (excellent)
- Permission wizard — `MacPermissionWizard.tsx` (excellent macOS-specific)
- Collab system — file-based, platform-agnostic (good)
- Backend API calls — all HTTP-based (no platform code)
- Database paths — `APPDATA` on Windows, `~/.vaak/` on macOS (good)
- Queue persistence — SQLite via rusqlite (cross-platform)
- Hotkey display — dynamic Cmd/Ctrl formatting (excellent)
- Platform detection — centralized `platform.ts` module (gold standard)

### Fix Priority (Consolidated from 4-auditor review, 2026-02-23)

**Can Ship NOW (no Mac hardware needed):**
1. Rewire `main.rs:1511` → `a11y::start_focus_tracking()` (5 min, zero risk)
2. Change `ScreenReaderApp.tsx:170-209` guards to `isWindows() || isMacOS()` (10 min, zero risk)
3. Commit UX-engineer's launch button fix (1 min)
4. GPU/MPS fallback in `whisper_finetuner.py` — add MPS detection, keep fp16 CUDA-only (15 min)
5. Delete legacy `focus_tracker.rs` after fix #1 (1 min)

**Needs Mac Hardware:**
0. **Cross-compile vaak-mcp sidecar** for `aarch64-apple-darwin` + `x86_64-apple-darwin` — BLOCKER for all Mac testing
6. Verify AX coordinate origin on physical Mac, implement flip if needed
7. NSPanel non-activating overlay window (proper macOS floating indicator)
8. Full integration testing

**Polish (Any Time):**
9. Update UIA label to "Accessibility Tree" on macOS
10. File permissions `mode=0o700` in Python mkdir calls

## 6. Collab System Architecture

### File Layout
```
.vaak/
├── project.json           # Project config, roles, roster, settings
├── sessions.json          # Active session bindings (role:instance:session_id)
├── board.jsonl            # Append-only message stream (default section)
├── board.lock             # Exclusive lock file for concurrent writes
├── discussion.json        # Active discussion state
├── claims.json            # File claims per developer
├── spawned.json           # Launched agent PIDs
├── roles/{slug}.md        # Briefing files per role
├── sections/{slug}/       # Per-section boards + discussions
└── last-seen/{session}.json  # Per-session read pointer
```

### Communication Flow
```
Claude Code agent                    Desktop App (Tauri)
     │                                      │
     ├── MCP sidecar (stdin/stdout) ──────┐ │
     │   └── project_send()               │ │
     │       └── append to board.jsonl    │ │
     │       └── POST :7865/collab/notify ─┤ │
     │                                     │ │
     │                                     ▼ │
     │                            emit("project-file-changed")
     │                                     │ │
     │                                     ▼ │
     │                            Frontend re-reads board.jsonl
     │                                       │
     │  ◄── POST :7865/speak ─── Frontend ───┘
     │       (voice output)
```

## 7. Security Assessment (Feb 2026 Audit)

### CRITICAL — FIXED
| Issue | Location | Fixed By | Validated |
|-------|----------|----------|-----------|
| Hardcoded admin passwords ("AALB") | backend/app/api/admin.py:648-652 | Dev:1 | Tester |
| Default JWT secret fallback | backend/app/core/config.py:24 | Dev:1 | Tester |
| CORS wildcard `allow_origins=["*"]` | backend/app/main.py:49 | Dev:1 | Tester |

### FIXED (Feb 24) — Desktop backend
| Issue | Location | Fixed By |
|-------|----------|----------|
| Rate limiter trusted X-User-Id header | web-service/app/middleware/rate_limiter.py | Architect |
| Unauthenticated paid endpoints (6) | backend/app/api/routes.py, audience.py | Architect |
| No admin rate limiting | backend/app/api/admin.py | Architect (3 attempts/5min) |
| Unbounded TTS input length | backend/app/api/routes.py | Architect (5000 char limit) |
| No graceful degradation on Vision/Chat | backend/app/api/routes.py, roles.py | Architect |

### FIXED (Feb 24) — Web service (from Evil Architect audit)
| Issue | Severity | Fixed By |
|-------|----------|----------|
| WebSocket IDOR: any user reads/writes any project | CRITICAL | Evil Architect |
| Rate limiter never registered in main.py | CRITICAL | Evil Architect |
| Free→BYOK self-upgrade + platform key fallback | CRITICAL | Evil Architect + UX Engineer |
| BYOK platform key fallback blocked (402) | HIGH | Evil Architect |
| TOCTOU race on usage counter (atomic SQL) | HIGH | Evil Architect |
| Monthly usage counters never reset (lazy reset) | HIGH | Evil Architect |
| WebSocket input validation | HIGH | Evil Architect |
| Agent runtime stubs → fully functional | HIGH | Architect |
| Per-session budget enforcement ($50/day) | HIGH | Architect |
| BYOK keys encrypted at rest (Fernet) | HIGH | Linter (key_encryption.py) |
| ON DELETE CASCADE on all relationships | MEDIUM | Architect |
| WS reconnect timer leak + parallel loops | MEDIUM | UX Engineer |
| Discussion refresh API spam (debounced) | MEDIUM | UX Engineer |
| Focus management on route changes (a11y) | MEDIUM | UX Engineer |
| Message deduplication (REST + WS race) | LOW | UX Engineer |
| Toast ARIA role mismatch | LOW | UX Engineer |

### REMAINING — UNFIXED
| Issue | Location | Status |
|-------|----------|--------|
| HTTP endpoints no auth (:7865) | desktop/src-tauri/src/main.rs | Local-only, low risk |
| XSS in admin dashboard (innerHTML) | backend/app/api/admin.py:1394-1629 | UNFIXED |
| 6 live API keys committed in .env | backend/.env | Needs human key rotation |
| Rate-limit tests fail in test mode | web-service/tests/test_auth.py | 2 tests need refactor |
| Tags/permissions silently dropped on save | web-service/app/api/projects.py | MEDIUM |

## 8. Known Issues & Technical Debt (Prioritized)

### Tier 1: Stop-Ship
1. **Lock file permanent freeze** — No lock timeout; crash during write permanently freezes collab system
2. **Computer use scaling bug** — Alt+A handler divides by base64 string length instead of pixel dimensions
3. **Race conditions in discussion system** — 6 unprotected write sites in vaak-mcp.rs
4. **Asymmetric notification bug** — Tauri write commands don't emit notify events
5. **Board.jsonl crash corruption** — No WAL, no backup, no checksums
6. Remaining security items (rate limiting, HTTP auth, XSS)

### Tier 2: Structural Debt
7. Collab backend migration to SQLite (eliminates 7 failure modes)
8. Error handling: 276 `Result<T, String>`, 60+ `unwrap()` calls, 11 unsafe blocks
9. Frontend monoliths: CollabTab (4,261 lines), App.tsx (1,583 lines)
10. Rust monolith: main.rs (3,923 lines), 62 `#[tauri::command]` functions
11. Zero test coverage across all layers

### Tier 3: Polish & Parity
12. Queue bugs (2 remaining of 13)
13. Accessibility gaps (no ARIA labels, no semantic HTML)
14. CSS: collab.css at 117KB, no component scoping

## 9. Active Decisions

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-02-23 | macOS parity is top priority | Human directive: all features must work on Mac |
| 2026-02-23 | Phase 1: coordinate flip, overlay, GPU, launch button | Critical blockers first |
| 2026-02-23 | Phase 2: UIA toggle, agent mgmt, focus migration, permissions | Polish after unblocking |
| 2026-02-17 | All discussion writes must use file locking | 6 unprotected write sites cause data corruption |
| 2026-02-17 | Tauri write commands must emit notify events | Asymmetric notifications cause 55s stale state |
| 2026-02-17 | JWT secret must be env-var-only, fail on missing | Default fallback allows trivial token forgery |
| 2026-02-17 | Frontend monoliths must be decomposed | 4,261-line CollabTab is unmaintainable |
| 2026-02-17 | Migrate collab backend from JSONL to SQLite (Tier 2) | Eliminates 7 failure modes |

## 10. Conventions & Standards

### Backend (Python)
- **Framework**: FastAPI with async/await throughout
- **ORM**: SQLAlchemy 2.0 with `Mapped` types
- **Validation**: Pydantic models for all request/response schemas
- **Auth**: `get_current_user` / `get_optional_user` dependencies
- **Naming**: snake_case throughout

### Frontend (TypeScript/React)
- **State**: React hooks (useState/useCallback/useRef), no global state library
- **Styling**: Pure CSS with custom properties, dark theme only, no Tailwind
- **API client**: Centralized in `lib/api.ts`
- **Settings**: localStorage with `loadSetting`/`saveSetting` pattern
- **Platform detection**: Always use `lib/platform.ts` — never `navigator.platform` directly

### Rust/Tauri
- **Commands**: `#[tauri::command]`, `Result<T, String>` returns
- **State**: `tauri::State<T>` with `parking_lot::Mutex`
- **Cross-platform**: `#[cfg(target_os = "...")]` for all platform-specific code
- **Locking**: `with_board_lock()` for all concurrent file writes

---

## 11. Session Mode Architecture (2026-04-16)

Originated in the pipeline-mode meta-discussion (board msgs 1–190). This section captures the architectural decisions that survived 5 rounds of adversarial review and produced consensus across all seven active roles. Each decision carries a **Why:** line documenting the constraint that forced it — future readers should apply that constraint before relitigating.

### 11.1 Session = container, Format = behavior

A **Session** holds state (id, participants, start/end time, decisions, moderator). A **Format** describes how turns are structured within a session (`pipeline` | `delphi` | `oxford` | `continuous`).

- **Why:** Mixing these two concepts in the old `DiscussionState` struct made format-specific bugs leak across formats (e.g., pipeline's turn-order guard breaking Continuous mode's parallel submission). Clean split lets each format's logic live behind a single interface.
- **How to apply:** New code keys persistent state on `session_id`. Format-specific logic receives `&Session` + `Format` as inputs, never constructs its own session identity.

### 11.2 Rename: "discussion" → "Session"

Terminology change shipping in PR R. All user-facing strings, MCP tool names, and persistent file names migrate. Backend struct renames: `DiscussionState` → `SessionState`, `discussion.json` → `session.json`, `discussion_control` → `session_control`.

- **Why:** "Discussion" implied talking; the thing also contains voting, deciding, and building. The human correctly flagged this as a misnomer (board msg 162). "Run" was proposed but rejected for noun/verb ambiguity in UI contexts. "Session" is warm, format-neutral, and doesn't collide with existing repo language.
- **How to apply:** New features use "Session" from day 1. Legacy field `discussion_mode` stays readable for 1 release via dual-key parsing, then removed.

### 11.3 Moderator and Manager: distinct privileged roles (existing split)

**`moderator`** and **`manager`** are two separate roles with overlapping-but-distinct capability sets. This split is already encoded in `desktop/src-tauri/src/bin/vaak-mcp.rs` and was there before this vision update. An earlier draft of this section proposed merging them (YAGNI argument); tech-leader retracted that arbitration (board msg 200) after reading the code. Capture what exists, do not "unify" what is already intentionally separate.

- **`moderator`**: the **format moderator**. Runs Delphi/Oxford/Pipeline rounds. Owns `moderator_only_actions` (defined at `vaak-mcp.rs:2170–2175`): `close_round`, `open_next_round`, `pause`, `resume`, `pipeline_next`, `end_discussion`, `gate_audience`, `inject_summary`, `skip_participant`, `reorder_pipeline`, `toggle_pipeline_mode`, `update_settings`. Auto-assigned to `moderator:0` when a session begins (preferred) or to caller otherwise (`vaak-mcp.rs:2232–2263`).
- **`manager`**: the **project coordinator**. Owns `@human` direct-message (`vaak-mcp.rs:3533`), pipeline-turn-lock bypass (`vaak-mcp.rs:3473`), consecutive-mode-turn bypass (`vaak-mcp.rs:3550`). Does not own format moderation actions.
- **Human msg 153 "manager acting as a moderator"**: interpret as *"both roles are privileged and share out-of-turn-speech capability within their charters,"* not as a merge directive. Manager speaks out of turn because manager has `@human` + bypass; moderator speaks out of turn because moderator owns format actions. Different authorities, same surface privilege.

- **Why:** The code is the source of truth. A vision doc that contradicts existing code creates drift, and every future reader has to reconcile the two. The merge proposal was an architectural move that would have undone a deliberate design — the kind of error the narrative-comment standard (§ 11.11) exists to prevent in the first place.
- **How to apply:** Capabilities extended to either role must specify which role and why. Do not add a capability to both unless the privilege is genuinely shared (e.g., `SpeakOutOfTurn` — both roles need it, for different reasons).

### 11.4 Moderator Fallback Invariant

When no `moderator` session is active, `human:0` inherits moderator capabilities for format actions. When a `moderator` session is claimed, the moderator role is authoritative; `human:0` falls back to normal broadcaster for format actions.

This invariant is already encoded at `vaak-mcp.rs:2176–2191` ("allow if no moderator set") — tester's msg 181 refinement matched what the code already did. Document the existing behavior; do not re-implement it.

- **Why:** The permission model collapses without a moderator. Perpetual human:0 capability removes the off-switch — if the human wants a hands-off test or to demote themselves, there's no mechanism. Authoritative-when-claimed + fallback-when-vacant gives both control paths.
- **How to apply:** Every format-action capability check respects this precedence. Tests must cover both branches (moderator claimed → moderator authoritative; moderator vacant → human:0 authoritative).

#### 11.4a Paused moderator retains authority

"Moderator claimed" includes "moderator session is paused." A paused moderator does NOT become effectively-vacant for the fallback rule. `human:0` does not acquire bypass capability while the moderator is paused.

- **Why:** Pause is a session-state action the moderator took deliberately. Promoting human:0 to moderator authority during a moderator-initiated pause inverts the intent of pausing — the moderator paused to prevent further action, not to hand authority to someone else. Dev-challenger flagged this gap in board msg 313 sharpening #2.
- **How to apply:** `active_moderator_session()` returns `Some(...)` for both live and paused moderator sessions. Only explicit moderator departure (session termination, `project_leave`) returns `None`. Resume authority belongs to the `manager` seat per § 11.7; a paused moderator does not auto-release their claim.

### 11.4b Manager Invariant

Manager's capabilities (`@human` direct-message, pipeline/consecutive turn-lock bypass) exist independently of moderator state. A vacant moderator seat does not promote human:0 to manager privileges. A vacant manager seat does not redirect @human routing.

- **Why:** Conflating the two fallback paths was tempting during round 1 arbitration and produced the merge error that tech-leader retracted. They are separate for a reason: a session can have a moderator but no manager (e.g., ad-hoc team debate where no one's tracking project coordination), and vice versa (manager coordinating across sessions without running a specific one). Conflated fallback creates a privilege-escalation path — a `human:0` vacancy-takeover intended for format moderation would quietly inherit direct-message-human privilege.
- **How to apply:** Manager capability checks consult `active_manager_session()` only. Moderator capability checks consult `active_moderator_session()` with `human:0` fallback. Never share a single `privileged_authority()` helper across both — the semantics diverge.

### 11.5 Format-gated capabilities

Moderator capabilities declare `allowed_formats: &[SessionFormat]`. A call to a capability not valid for the active session's format returns `ModeratorError::CapabilityNotSupportedForFormat { capability, format }`. UI consumes the error variant to render a disabled-with-tooltip state; non-manager roles never see the capability surface.

| Capability | Allowed formats |
|---|---|
| `ReorderPipeline` | pipeline |
| `JumpToStage` | pipeline |
| `PauseSession` | all |
| `ResumeSession` | all |
| `EndSession` | all |
| `SpeakOutOfTurn` | all |
| `DirectMessageHuman` | all |

- **Why:** "Jump to stage" is meaningless in Delphi (parallel, no stages). "Reorder" is meaningless in Continuous (ambient, no order). Without gating, a manager in Delphi sees a Reorder button that silently does nothing — the exact failure mode that doesn't surface in testing because no one runs Delphi with a manager yet. Dev-challenger attack 2 (board msg 172).
- **How to apply:** Every new format declares which capabilities apply. Every new capability declares its format allowlist. UI disables (not hides) unsupported capabilities with explicit tooltip text.

### 11.6 Tiered second-factors for destructive moderator actions

Capabilities fall into three tiers based on reversibility:

- **Reversible, low-risk**: `ReorderPipeline`, `PauseSession`, `ResumeSession`, `SpeakOutOfTurn`, `DirectMessageHuman` — no second-factor beyond UI confirm
- **Disruptive but recoverable**: `JumpToStage` — mandatory non-empty `reason` field, min 3 chars after whitespace strip
- **Irreversible**: `EndSession` — mandatory `reason` field + UI confirmation modal

Audit trail captured as metadata `moderator_action: {action, reason, timestamp, affected_role?}` on every privileged message.

- **Why:** A moderator that can end or skip stages without justification degrades format integrity within weeks — the same drift that made auto-advance invisible-moderation problematic in the first place. Audit is the accountability mechanism; reason-required gates turn policy into record. Dev-challenger attack 1 (board msg 172).
- **How to apply:** Tier escalations require architect + tech-leader sign-off. Never downgrade a tier without evidence from postmortem data.

### 11.7 Moderator-exit auto-pause

When the moderator session terminates mid-session (process dies, network drop, explicit leave), the system auto-pauses with banner `Session paused — moderator offline`. PR A's auto-termination rule does NOT fire while paused. Resume requires someone to claim the `manager` seat.

- **Why:** Without this, an active session with a dead moderator runs indefinitely under auto-advance rules, defeating the msg-153 directive that only the moderator can advance. Auto-pause preserves the moderator-authority invariant across unexpected moderator loss. Dev-challenger attack 3 (board msg 172).
- **How to apply:** Moderator session liveness checked on every `advance()`. If stale, emit `session_paused_moderator_offline` event before advancing. Unpause is a manager-claim action, not a regular broadcast.

### 11.8 Auto-termination semantics (PR A)

Pipeline auto-terminates when every participant in the most recently completed round has posted a message tagged `metadata.vote: accept` on the same `on: <msg_id>`. Snapshot semantics: most-recent-vote-per-role wins; prior votes from the same role are superseded.

- **Why:** Latched (any-historical-accept) voting prevents roles from changing their minds across rounds — contradicts the retraction pattern that round 2/3 of the originating pipeline itself demonstrated was the format working correctly. Round-boundary consensus avoids the sliding-window footgun. Developer's msg 155 + dev-challenger's msg 151 semantics.
- **How to apply:** Consensus check scans the last completed round only. Synthesizer self-votes count (no exclusion) — per developer msg 155, "synthesizer-exclusion" is a convention nicety, not load-bearing for termination. Revisit only if a concrete case demonstrates the need.

### 11.9 Decisions ↔ voting bridge: DEFERRED

The msg-56 plan proposed unifying pipeline `decisions` with the workflow-voting system. Deferred per dev-challenger (msg 151) and developer (msg 161) attacks: both systems are unstable, coupling now is architectural fantasy.

- **Why:** Unify after ≥5 real pipeline decisions have shipped and workflow-voting has ≥5 users. Without evidence, the coupling is speculative and the cost (schema rework) is real.
- **How to apply:** Track pipeline `decisions` and workflow votes as separate concerns in PR 4. Revisit unification after observational data accrues.

### 11.10 Format taxonomy: observational, not prescriptive

A "when to use pipeline vs delphi vs oxford vs continuous" matrix was considered and rejected (dev-challenger msg 26, architect retraction msg 44). Instead, ship `format_postmortem` primitive — after each session, moderator fills `{format, topic_class, turns_used, decision_reached: bool, regret: text}`. Generate the matrix from ≥20 postmortems.

- **Why:** A taxonomy written today codifies architect intuition, not observed patterns. Codified intuition ossifies into dogma. Evidence-driven taxonomy waits for data.
- **How to apply:** Format postmortems are required on session end (PR 4 scope). Review aggregate data quarterly. Do not author a prescriptive matrix until data supports it.

### 11.11 Narrative-comment standard for session code

Every capability definition, every format gate, every auto-termination branch, every migration fallback carries a `// Why:` comment naming the constraint that forced the choice. Tech-leader rejects PRs that omit.

- **Why:** Three months from now, `PidReuseGuard` or `MoveFileExW` looks like overkill unless the reader knows the specific platform incident that forced it. The comment is the only mechanism preserving the constraint after the people who remembered it move on.
- **How to apply:** Comment describes the constraint, not the code. "This function validates the session ID format" is not a Why. "Session IDs must match this regex because custom-role-creation flow (2026-02-10) allows non-ASCII slugs which NFC-normalize differently on APFS vs NTFS" is a Why.

### 11.12 Execution sequencing (2026-04-16)

PRs land in this order:
1. **PR M** (Moderator privileges) — developer, ~3 days
2. **PR H** (Human channel filter, three-tab UI) — UX, ~1 day, parallel with PR M
3. **PR A** (Auto-termination on consensus) — developer, ~2 days, after PR M
4. **PR R** (Rename discussion→Session) — developer, ~1-2 days, after PR A
5. **PR H2** (Full moderator toolbar: reorder/jump/pause/end/speak-OOT/human-bridge) — UX, ~1-2 days, after PR M

Parity contract (`.vaak/specs/pipeline-parity.md`) scoped to the surface of these 5 PRs only. Wider msg-56 ledger items redrafted after this cycle ships.

### 11.13 Gate parity across process boundaries (2026-04-18)

When the same data file is writable from both the Tauri main process and the MCP sidecar, every validation rule MUST be enforced at both entry points. Asymmetric gating is a latent footgun: any UI, CLI, or agent path inherits the weaker rule. Preferred shape is a shared validator (e.g., `collab::validate_discussion_mode`) that both binaries call; duplicated enum arrays are the drift pattern that ships holes.

- **Why:** `fbf5db9 pr-seq-7-pipeline-removal` patched only `vaak-mcp.rs` and declared DONE. Runtime evidence (human started a pipeline discussion through the UI on 2026-04-18 via `QuickLaunchBar` default → Tauri `start_discussion` → `valid_modes` still accepts `"pipeline"`) proved the UI-origin path was ungated. The sign-off narrative (tech-leader:0 msg 623) reasoned "the human always bypasses the gate anyway" — that's the opposite of what a gate is for. The primary entry point is exactly what must be gated.
- **How to apply:** Before approving any removal/restriction PR that touches a data file reachable from both processes, trace the call graph from *every* user-visible entry point (button, quick-launch pill, keybind, agent-invoked MCP command) to the final handler and confirm each passes through the validator. Removal ≠ done until every layer enforces. Regression tests live at the Tauri-command level, not only at the MCP-sidecar level.

### 11.14 Pipeline / Sequence convergence — unified surface (2026-04-18, human-ratified)

Pipeline and Sequence coexisted as two overlapping turn-taking features after tier 3 shipped. Human ratified convergence direction in msg 818 ("A" in response to code-interpreter:1 msg 817's A/B question). One state machine, two entry flavors, unified controls.

- **Why:** Redundant UI surfaces confuse users and double the maintenance surface. Pipeline's 30s destructive auto-skip cost work in the "Working yet?" test sequence (msg 741); Sequence's non-destructive stall model (msg 753, 757) is strictly safer. But Pipeline's QuickLaunchBar pill + 1-click muscle memory is real UX value the human defaulted to (msgs 644, 767, 773). The ratified shape: keep Pipeline's entry UX, run Sequence's mechanics underneath, show the same controls regardless of launch path.
- **How to apply:** Implementation plan spans three PRs in the next session:
  1. `pr-seq-multi-round-support` — extend `active_sequence` schema with `rounds_remaining` / `current_round` + restart-queue logic + per-round aggregation (dev-challenger:1 msg 807 blocker). ~150 LOC + tests.
  2. `pr-seq-quicklaunch-pipeline-rewire` — QuickLaunchBar Pipeline pill invokes `discussion_control(action: start_sequence, auto_advance_on_stall: true, participants: <all-active>, ...)`. Delete `mode=pipeline` write paths in MCP + Tauri with a redirect error. DiscussionPanel's defensive pipeline render branches stay (platform-engineer:0 msg 813 Option A: accept staleness, 0 LOC migration). ~100 LOC + tests.
  3. `pr-seq-unified-surface` — ensure HumanSequenceOverrideBar + ModeratorSequencePanel + SequenceSessionCard render for every active sequence regardless of launch path. No conditional hiding based on `auto_advance_on_stall` or entry cookie-trail. ~50 LOC + vitests.
- **Non-negotiable constraints** (from the session debate):
  - `auto_advance_on_stall = true` must use sequence's non-destructive 300s notification model, NOT pipeline's 30s destructive skip. "Auto-advance" means "notify-then-skip after warning window," not "silently drop turn." Evil-architect:0 msg 816 foot-gun #2.
  - SequenceSessionCard displays the current stall mode visibly ("Auto-skip: ON / 300s" vs "Stall: non-destructive / 300s"). Evil-architect:0 msg 816 foot-gun #2 mitigation.
  - "Pipeline" survives only as a UI label and a `discussion_control` preset flag. Internal code + state all uses `sequence` / `active_sequence`. One comment in CollabTab.tsx + collab.rs spelling out the aliasing so future contributors don't regrow `pipeline` as a real concept. Evil-architect:0 msg 816 foot-gun #3 mitigation.
- **Live-click smoke gate** applies per PR (tech-leader:1 msg 732 rule): after each PR lands, someone clicks through on a rebuilt binary before DONE.

### 11.15 Pipeline stage-advance signal (2026-04-19)

Pipeline advance logic at `desktop/src-tauri/src/bin/vaak-mcp.rs:5789-5795` currently advances the stage on **any** broadcast (`to == "all"`) from the current-stage agent. This collides with the team protocol documented in `feedback_ack_is_one_sentence.md` ("Pipeline ack is 1 sentence fired first; substance goes in a separate second send"). The bare ack counts as stage completion, the pipeline advances, and the agent's real substance broadcast is either blocked by the gate or lands after the next stage has already fired. Observed in the 2026-04-19 pipeline run (board msgs 1104–1124): 11 stages elapsed, zero substantive outputs landed on-turn, human ended the discussion.

- **Invariant:** Stage advance MUST be triggered by an explicit signal from the current-stage agent, NOT by mere presence of a broadcast.
- **Ratified signal (shipped in commit `d759666`):** `is_end_of_stage(metadata)` is true only when `metadata.end_of_stage == bool(true)` — string `"true"`, number `1`, or missing key all evaluate false. A broadcast from the current-stage agent advances the pipeline only when this predicate returns true. Every other broadcast (acks, interim status, reviews) is allowed but does not advance the stage.
- **Why metadata flag, not `msg_type == "handoff"`:** `handoff` type is permission-guarded (cross-role work assignment); 6 of 11 pipeline-participating roles lack the permission (tech-leader:0 msg 1151 verification). Semantic mismatch — pipeline advance is mechanical turn-passing, not work reassignment. Metadata flag is permission-free and orthogonal to message type, letting agents end their stage with any type (`status`, `review`, `approval`) plus the flag.
- **Single authoritative signal — no fallbacks.** The shipped implementation (d759666) dropped the 500-char length heuristic that appeared in the superseded 8ae4141. Rationale: length thresholds are brittle (terse substantive messages like "APPROVED — ship it" at <500 chars would have incorrectly stayed open; verbose "still investigating" acks at >500 chars would have incorrectly advanced). Metadata-only is cleaner and teachable.
- **How to apply:**
  - Gate the advance block (`vaak-mcp.rs:5824`) on `from_label == current_agent && is_end_of_stage(metadata.as_ref())`.
  - Update agent briefings via `desktop/src/utils/briefingGenerator.ts` + pipeline announce/wake messages + MCP hook injection (d759666 touches all four surfaces): every pipeline stage ends with a broadcast that includes `metadata: {end_of_stage: true}`. Earlier broadcasts (ack, investigation status, intermediate findings) do not set this flag and therefore do not terminate the stage.
  - Test coverage (shipped in `8ae4141`): unit tests for (a) short-ack-without-flag no-advance, (b) long-body advance, (c) explicit `metadata.ack: true` opt-out forces no-advance even for long body, (d) explicit `metadata.end_of_stage: true` advances even for short body. Plus watchdog-suppression tests.
  - **Deploy invariant:** source changes to `vaak-mcp.rs` require a rebuilt binary commit to `desktop/src-tauri/binaries/vaak-mcp-x86_64-pc-windows-msvc.exe` in the same PR. Without this, the fix is source-only and does not run. See § 11.16.
- **Secondary gate-tightening deferred:** the escape hatches in the non-current-stage gate (manager bypass, to=human, question-to-current by role-slug, answer-to-current-question without uniqueness) are real but did not cause the 2026-04-19 collapse. Close them in a follow-up PR after the advance-signal fix lands and is observed stable. See board msg 1130 (original proposal, superseded) and msg 1133 (correction).

### 11.16 MCP binary deploy invariant (2026-04-19)

The MCP sidecar binary at `desktop/src-tauri/binaries/vaak-mcp-x86_64-pc-windows-msvc.exe` is a **committed artifact**, not a build output. Source changes to `desktop/src-tauri/src/bin/vaak-mcp.rs` (or anything the vaak-mcp binary depends on) do NOT take effect on running Claude Code sessions until the binary is rebuilt AND the new file replaces the committed copy.

- **Problem observed 2026-04-19 (tech-leader:0 msg 1141):** 7 pipeline PRs landed in source between binary builds. Agents and reviewers assumed fixes were running; they were not. The team debated adding an 8th PR on top of stale runtime state.
- **Invariant:** Every PR that modifies `desktop/src-tauri/src/bin/vaak-mcp.rs` (or non-test source it depends on) MUST include a rebuilt binary commit.
  - Build: `cd desktop/src-tauri && cargo build --release --bin vaak-mcp`
  - Deploy: `cp ../../target/release/vaak-mcp.exe binaries/vaak-mcp-x86_64-pc-windows-msvc.exe`
  - Commit: the updated binary alongside source in the same PR, message prefix `deploy:` or similar. No separate rebuild-only PRs.
- **Ship discipline:** `git show --stat <sha>` of the ship broadcast must show the binary changed. Peers reviewing a ship claim where the binary is absent should require the binary commit before approving.
- **Follow-up (separate PR):** add a pre-commit hook or CI step that fails the build if `vaak-mcp.rs` changed without a corresponding binary update. Filename: `pr-mcp-deploy-discipline`.

---

*This document is maintained by the Architect role and updated when architectural decisions are made or the codebase evolves significantly.*
