# Team Handoff Document — March 6, 2026

## Session Summary

This session focused on **Mac/Windows platform parity**, **bug fixes**, and **codebase health**. Four commits were pushed to the `dev-local` branch on GitHub. None have been merged to `main` yet.

---

## Commits Pushed to dev-local (in order)

### 1. `65e2ffb` — Fix CollabTab scroll jumping to middle on message send/receive
**File**: `desktop/src/components/CollabTab.tsx`
**Problem**: Every time a message was sent or received, the collab message timeline would scroll to the middle of the conversation instead of staying in place.
**Root cause**: The `project-file-changed` and `project-update` event handlers called `setProject(result)` which triggered a full React re-render of the message timeline, losing the browser's scroll position.
**Fix**:
- Added `savedScrollRef` to capture `scrollTop` before `setProject()` calls
- Added `useLayoutEffect` that restores scroll position after React commits DOM updates
- Auto-scrolls to bottom when the human sends their own message
- Grows `visibleMsgLimit` on new messages to prevent slice window shifts
- Increased `isAtBottom` threshold from 40px to 150px

### 2. `8443202` — Make tray-recording.png visually distinct (red) from idle icon
**File**: `desktop/src-tauri/icons/tray-recording.png`
**Problem**: On Mac, the tray icon didn't change during recording because `tray-idle.png` and `tray-recording.png` were identical teal images.
**Fix**: Generated a red-tinted version of the icon using PIL. The tray now shows teal (idle) vs red (recording). Requires Rust rebuild since icons are embedded via `include_bytes!`.

### 3. `cda9e7e` — Add fallback for Download Node.js button on Mac
**File**: `desktop/src/components/CollabTab.tsx`
**Problem**: The "Download Node.js" button in the setup wizard did nothing on Mac. The `invoke("open_url_in_browser")` call was failing silently.
**Fix**: Added `window.open("https://nodejs.org", "_blank")` as a fallback when the Tauri invoke fails.

### 4. `cfa06d2` — Fix microphone permission denied on vaak-lite web app
**File**: `backend/app/main.py`
**Problem**: After merging dev-local security hardening to main, the vaak-lite web transcription app started showing "permission denied" for microphone access.
**Root cause**: `SecurityHeadersMiddleware` was sending `Permissions-Policy: microphone=()` on ALL responses, which tells the browser to block microphone access entirely.
**Fix**: Conditionally omit `microphone=()` for `/vaaklite` and `/api/v1/transcri` paths. All other routes still block microphone.

**CRITICAL**: This fix needs to be merged to `main` and redeployed for the live vaak-lite service to work again.

---

## What Needs To Be Done Next

### Immediate (Merge & Deploy)
1. **Merge dev-local to main**: `git checkout main && git merge dev-local && git push origin main`
2. **Redeploy backend**: The microphone fix (cfa06d2) must reach production for vaak-lite to work
3. **Rebuild desktop app on Mac**: `cd desktop && npm run tauri dev` — needed for tray icon, scroll fix, and button fix

### Mac Platform Parity (Open Issues)
4. **No audio feedback when app is in background**: Web Audio API `AudioContext` gets suspended by macOS when the WebView doesn't have focus. Global hotkey presses don't count as user interaction. Fix options: (a) play sounds from Rust using cpal/rodio, (b) use macOS system sounds via NSSound
5. **No floating overlay on Mac**: Intentionally skipped because `show()` steals focus and breaks paste. Only the tray icon (now red) and "REC" menu bar text indicate recording. Fix options: (a) Use a macOS NSPanel (non-activating overlay), (b) Use macOS notifications for recording start/stop
6. **Setup wizard too complex**: Requires Node.js + npm + CLI install. Human wants a one-click experience. Options: (a) Bundle Claude CLI binary in the Tauri app, (b) Ship a pkg/dmg installer that includes Node.js, (c) Auto-download Claude CLI binary without npm

### Codebase Health
7. **Other modified files not committed**: `Cargo.toml`, `package.json`, `Cargo.lock`, `vaak-mcp.rs`, capability schemas were modified by other agents but not committed. Review before committing.
8. **Evil Architect audit items** (from earlier session):
   - DELETE `web-service/app/services/usage_meter.py` (dead code — metering.py is the real implementation)
   - Tighten `infer_provider_from_model()` — `model.startswith("o")` is too broad
   - Extract `SecurityHeadersMiddleware` to shared module (duplicated between backend and web-service)
9. **Role organization**: 28 roles in a flat list is overwhelming. Only 2 role_groups exist (Medical: 5, Curriculum: 11). Core roles need grouping. UX engineer should design this.
10. **Launch scripts for Mac**: `launch-dev.bat`, `launch-dev.ps1`, `launch-team.ps1` exist for Windows but have no Mac/Linux `.sh` equivalents.

### Known Bugs (Not Fixed This Session)
11. **Queue bugs**: 10+ issues documented in `QUEUE_BUGS.md`, none fixed
12. **Zero test coverage**: No tests across any layer (Rust, TypeScript, Python backend)
13. **main.rs monolith**: ~2700 lines, needs decomposition
14. **Security**: Hardcoded admin passwords, default JWT secret (documented in MEMORY.md)

---

## Architecture Quick Reference

- **Tauri desktop** (Rust) + **React/TS frontend** + **FastAPI** (Python) backend
- 5 Tauri windows: main, transcript, screen-reader, overlay, queue
- MCP sidecar (`vaak-mcp.rs`) bridges Claude Code <-> Vaak via stdio JSON-RPC
- HTTP server on `:7865` (Rust) for speak/heartbeat/collab
- Backend on `:19836` (Python FastAPI)
- Vaak-lite web app mounted at `/vaaklite` on the main backend
- Collab system uses `.vaak/` directory with file-based messaging (JSONL)

## Key File Locations

| Component | Path |
|-----------|------|
| Rust main | `desktop/src-tauri/src/main.rs` |
| React main app | `desktop/src/App.tsx` |
| Collab tab | `desktop/src/components/CollabTab.tsx` |
| Platform detection | `desktop/src/lib/platform.ts` |
| Sound feedback | `desktop/src/lib/sounds.ts` |
| Launcher (agent spawning) | `desktop/src-tauri/src/launcher.rs` |
| Collab backend | `desktop/src-tauri/src/collab.rs` |
| MCP sidecar | `desktop/src-tauri/src/bin/vaak-mcp.rs` |
| Python backend | `backend/app/main.py` |
| Vaak-lite app | `backend/app/vaaklite/app.py` |
| Security middleware | `backend/app/main.py` (SecurityHeadersMiddleware) |
| CI/CD | `.github/workflows/build.yml` |
| Tray icons | `desktop/src-tauri/icons/tray-*.png` |

---

*Generated by Developer:1 — March 6, 2026*
