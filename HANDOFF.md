# Team Handoff Document — March 6, 2026 (Updated PM Session)

## STATUS: Uncommitted local changes on dev-local. Nothing pushed yet.

## UNCOMMITTED CHANGES (This Session)

### 1. CRITICAL — Vaaklite Microphone Fix
- **File:** backend/app/main.py (lines 19-23)
- **Problem:** Permissions-Policy: microphone=() blocks mic for vaaklite
- **Fix:** Omit microphone=() for /vaaklite and /api/v1/transcri routes
- **Deploy:** Restart backend. Python change, no build.

### 2. HIGH — Collab Tab Scroll Bug
- **File:** desktop/src/components/CollabTab.tsx
- **Problem:** Sending message jumped scroll to mid-conversation
- **Fix:** scrollingToBottomRef flag suppresses scroll saves during smooth animation

### 3. HIGH — Setup Wizard One-Click Install (macOS)
- **Files:** launcher.rs, main.rs, CollabTab.tsx
- **New:** check_homebrew_installed(), install_nodejs() commands
- **Mac+Homebrew:** brew install node, then auto-cascades to Claude CLI
- **Without Homebrew:** Opens nodejs.org (same as before)

### 4. MEDIUM — Global Role Groups Persistence
- **Files:** vaak-mcp.rs, main.rs
- **What:** Auto-imports role-groups.json into new projects on join
- **Backfilled:** 17 missing roles + 2 groups to ~/.vaak/

### 5. LOW — Tauri Opener Plugin
- **Files:** Cargo.toml, main.rs, default.json, package.json
- **What:** tauri-plugin-opener for URL opening

## macOS ISSUES (Not fixed, future work)

- No recording indicator when backgrounded (overlay disabled, steals focus)
- No sound when backgrounded (AudioContext may suspend)
- Tray icon not turning red (template image, use shapes not colors)
- API key step still manual

## PREVIOUS COMMITS (Already on remote dev-local)

1. 65e2ffb - Fix CollabTab scroll jumping
2. 8443202 - Make tray-recording.png red
3. cda9e7e - Add fallback for Node.js button on Mac
4. cfa06d2 - Fix microphone permission denied on vaak-lite

## EVIL ARCHITECT AUDIT (Pre-existing)

1. CRITICAL: usage_meter.py is a no-op stub, delete it
2. HIGH: infer_provider_from_model startswith("o") too broad
3. MEDIUM: SecurityHeadersMiddleware duplicated
4. MEDIUM: usage_meter.py is dead code

## PLATFORM PARITY: EXCELLENT

All Rust functions have Win/Mac/Linux counterparts. Audio, a11y, keyboard, sidecar, bundle all covered. Only missing: .sh launch scripts (convenience).

## DEPLOY COMMANDS

Backend (mic fix): restart server, no build
Desktop: cd desktop && npm install && npm run build && cd src-tauri && cargo build --release

Commit: git add backend/app/main.py desktop/src/components/CollabTab.tsx desktop/src-tauri/src/launcher.rs desktop/src-tauri/src/main.rs desktop/src-tauri/src/bin/vaak-mcp.rs desktop/src-tauri/Cargo.toml desktop/src-tauri/capabilities/default.json desktop/package.json desktop/package-lock.json && git commit -m "Fix vaaklite mic, scroll bug, setup wizard, global role groups"
