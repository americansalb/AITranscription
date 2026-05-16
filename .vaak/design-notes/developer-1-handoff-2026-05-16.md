# developer:1 — End-of-Session Handoff (2026-05-16)

## Lane State at Close

**Branch:** `feature/strict-turn-discipline` at tip `a2e8ad7` on `origin`.

**Commits this session (in order):**
- `07fbd95` — Tier 1.5 stderr breadcrumb at vaak-mcp.rs `get_session_id` entry (Bug #3 diagnostic)
- `3bc377f` — Tier 1.5b file-write breadcrumb to `.vaak/diagnostics/startup-env.jsonl` (Bug #3 diagnostic, breadcrumb_version=1)
- `305f549` — Bug #3 Part B v2 Phase 0: MCP initialize params + per-request `_meta`/`clientInfo` capture
- `09a29dd` — Frontend zombie-seat filter: `ProtocolPanel.tsx CompactMicLine` + `AssemblyControls.tsx renderStatusLine`. Filters `rotation_order` against heartbeats / activeSeats.
- `7f658c3` — Server-side zombie-seat filter at vaak-mcp.rs:8810 mic_landed body construction. `seat_has_binding` closure.
- `0e73ba8` — Status-exclusion tightening on `seat_has_binding` — adds `status NOT IN {"revoked", "left"}` per architect msg 2808 canonical predicate.
- `a2e8ad7` — Zombie-seat mutator enumeration design-note at `.vaak/design-notes/zombie-seat-mutator-enumeration-2026-05-15.md`. 187 LOC, 10 sections.

**Build artifacts:**
- Sidecar `vaak-mcp-x86_64-pc-windows-msvc.exe` — mtime 21:50, size 2522624. Carries 305f549 + 3bc377f + 7f658c3 + 0e73ba8.
- Vaak desktop `vaak-desktop.exe` (build bbe6thjxv complete 6m 08s) — mtime 22:06, size 7470080. Carries the bundled `dist/` from 09a29dd.

## Activation Status: PENDING HUMAN RELAUNCH

All 6 commits PUSHED + binaries on disk. **None of the changes are live in the running app yet.** Per dev:1 msg 2903 consolidated READY sequence:
1. Human closes all Claude Code windows (5 active seats + their shell)
2. Human closes the running Vaak Tauri app
3. Human relaunches Vaak Tauri (picks up new vaak.exe with bundled TS dist)
4. Human relaunches all Claude Code windows (spawns fresh sidecars with new vaak-mcp binary)

Per `[[project_ts_change_needs_tauri_rebuild_and_restart]]` (new memory from architect msg 2915) + `[[project_sidecar_relaunch_requires_claude_code_restart]]` (existing).

## First-Action Checklist For Next developer:1

1. **Verify activation** — read `.vaak/diagnostics/startup-env.jsonl` tail; expect new entries with PPIDs matching post-relaunch claude.exe PIDs. If no new entries → vaak-mcp binary didn't activate (sidecar relaunch gate missed). If breadcrumb_version != 1 → wrong binary running.

2. **Check rotation strip live state** — look at Vaak UI rotation strip + most recent `[mic_landed]` board message body. Both should show 6 active seats (no ghosts). If ghosts visible → 09a29dd / 7f658c3 / 0e73ba8 not active.

3. **Wait for tester:0 Phase 0.5 captures** — `.vaak/diagnostics/startup-env.jsonl` MCP-server-spawn entries (PPID matching claude.exe PIDs, not just hook-mode sidecars) discriminate Bug #3 hypothesis space per architect msg 2718 vs evil-arch msg 2722 open question.

4. **Human (D3) trial result** — Phase 0.5 (D) MCP-config env-field test (per architect msg 2776). If D3 wins → Phase 1 fix is ~3 LOC at main.rs:889 adding `"env": {"CLAUDE_CODE_SESSION_ID": "${...}"}` to the vaak entry json!() block. If D3/D1/D2 all fail → Phase 1 falls back to PPID-cmdline introspection per architect msg 2707 Option B (~30 LOC + platform `#[cfg]` blocks).

5. **Start §13 step 0 audit grep work** — backend-only, no human-relaunch dependency. Pre-emptive partial work done this session:
   - **§13 item 7 (`heartbeats.connected` consumers)** — initial grep returned 4 sites in `desktop/src/`:
     - `SeatChip.tsx:36` — `!heartbeat || !heartbeat.connected) return 'disconnected'` — strict-active interpretation; if filter loosens to accept idle, idle seats no longer show 'disconnected' visual. POSSIBLE behavior change.
     - `ProtocolPanel.tsx:328` — my own 09a29dd filter (the loosening target).
     - `composer/micToDetector.ts:58` — `sameRole.filter((s) => s.connected)` — picks active instances for "Mic to ROLE" routing.
     - `composer/micToDetector.ts:65` — `if (!exact.connected)` — branches on connected; returns classification 'disconnected' if user types "Mic to ROLE:N" against a non-active seat. Behavior change: if loosened, can now mic-to idle seats. Likely correct (idle seat is in rotation, will respond when mic arrives).
   - **§13 item 6 (`bindings.iter()` consumers for §6 leave-mark migration)** — initial grep returned 32 occurrences across 3 Rust files (collab.rs:5, main.rs:2, vaak-mcp.rs:25). Manual audit per consumer NOT complete; queued for next session.

6. **§10 steps 1+2 backend implementation** can start in parallel (no §13 audit dependency per dev-challenger msg 2893):
   - **§10 step 1 (watchdog mic_released rotation_order prune)** — main.rs:5510-5535 needs `arr.retain(seat != speaker)` matching kick/leave handlers. ~5-8 LOC.
   - **§10 step 2 (sweep mechanism)** — primary trigger = watchdog tick per architect msg 2879 v1.3 revert. ~15-20 LOC in main.rs check_assembly_floor_watchdog. Iterate active section's `floor.rotation_order`; drop entries where seat has no presence-passing binding in sessions.json.

7. **§10 steps 3+4** WAIT on §13 step 0 audit + 5-min adversarial review window per architect msg 2901 §(1).

## Caveats / Open Architectural Debt

- **Bug #3 latent bug at main.rs:873-883** — installer's `needs_update` check at vaak-mcp.rs:889 logic only compares `command` field, NOT env-field. If Phase 1 fix is "add env field to ~/.claude.json vaak entry," existing installs will NOT get the new env field on subsequent Vaak launches. Per dev-challenger msg 2830 audit-omission-paths instance: matter for spec v4 amendment delta when Phase 1 lands.
- **Bindings status lifecycle has FOUR values** (active, revoked, disconnected, idle if writer confirmed); my 0e73ba8 only excludes 2 (revoked, left). "Disconnected" inclusion is design-call deferred to spec; "idle" is intentionally accepted. Confirm post-relaunch.
- **TS filter still STRICTER than canonical** — `heartbeats.connected = (status == "active")` at main.rs:3332 + vaak-mcp.rs:3475 rejects "idle" / "disconnected". Spec v1.4 §10 step 4 calls for loosening. Implementation pending audit results.
- **Per-section migration unverified** — section "5-12" today has ~2 days of accumulated drift; switch_section behavior not yet checked.

## Memory Updates This Session

None written by me. Architect committed to write 3 (per msg 2915):
- `project_rapid_debate_class_of_bug_discovery_2026-05-15.md`
- `project_ts_change_needs_tauri_rebuild_and_restart.md`
- `feedback_static_pass_insufficient_without_live_pass_evidence.md`

Plus dev-challenger landed `feedback_audit_omission_paths.md` mid-rotation per msg 2830.

## References

- Activation sequence: dev:1 msg 2903
- Enumeration ground truth: `.vaak/design-notes/zombie-seat-mutator-enumeration-2026-05-15.md`
- Zombie-seats spec v1.4: `.vaak/design-notes/zombie-seats-spec-2026-05-15.md`
- Bug #3 cold-start spec (Phase 1 delta pending): `.vaak/design-notes/cold-start-integration-contract-gate-spec-2026-05-15.md`
- Architect handoff: `.vaak/design-notes/architect-handoff-2026-05-16-session-end.md`

Session over for developer:1.
