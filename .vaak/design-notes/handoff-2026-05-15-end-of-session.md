# Session Handoff — 2026-05-15 End of Day

Author: developer:1
Branch: `feature/strict-turn-discipline` — pushed to origin at `df65e55..1095bdf` (10 commits)

## What shipped

### Phase 1 (S/S.A/T) — review-intensity-slider + working-turn mic-hold
- **3511596 S** — `apply_set_review_intensity` backend + `Floor.review_intensity` u8 with `#[serde(default = 5)]` back-compat + 8 R-fixtures (S1 role gate × 4, S2 range × 3, S3 default).
- **fb1e282 S.A** — ReviewIntensitySlider UI in AssemblyControls (gated to moderator/privileged) + CollabTab `review_intensity_changed` separator card + `[SetReviewIntensityForbidden]` friendlyError.
- **42d2452 T** — `should_suppress_floor_stall(turn_type, review_intensity)` helper + watchdog `check_two_controls_dead_seats` gate suppressing `floor_stall` when working-turn at intensity ≥ 5 + 3 R-fixtures (T1 × 2 working suppresses 5-10, T2 communication never suppresses).

### Leave-glitch + roster-glitch fixes
- **a091870** — `handle_project_leave` + `handle_project_kick` now ALSO prune `floor.rotation_order` / `current_speaker` / `moderator` / `hand_queue` in the same CAS-bumped write as sessions.json. No more zombie-seat-in-rotation requiring manual buzz.
- **fdcc676** — Same prune extended to `collab.rs::roster_remove_slot` so the existing CollabTab role-card X button does full kick + protocol clean across ALL section protocol.json files.

### Phase 2 (C/G + al_auto_advance gate)
- **6af1784 C** — `.claude/hooks/file-op-claim.py` (POSIX) + `.cmd` (Windows) + `.claude/settings.json` PostToolUse registration. Hook fires on Read/Edit/Write/NotebookEdit success, derives seat from `CLAUDE_SESSION_ID` + `sessions.json`, upserts `.vaak/claims.json` entry.
- **6fe60e4** — Shape fix: Commit C hook now writes the existing FileClaim shape (`role:instance` keyed dict with `session_id`/`files`/`description`/`claimed_at`) so claims flow through existing `collab.rs::read_claims_filtered` + render in CollabTab's existing "Active Claims" section. **C.A folded**: existing claims UI renders auto-claim data without new render code.
- **ae3b0d4 G** — `.claude/hooks/turn-gate.py` PreToolUse hook + level 6-10 enforcement matrix:
  - 1-5: pass (no gate)
  - 6-7: audit-only (`read_off_turn` board event emitted)
  - 8: soft-block with `_peek_acknowledged` / `--peek-acknowledged` override
  - 9-10: hard block
  - Exempt: human / floor.moderator / floor.current_speaker
- **1095bdf** — `al_auto_advance` gate (evil-arch msg 2421 + human msg 2441): suppress when `review_intensity >= 7` OR sender `turn_type == "working"` AND no explicit `yield_to.target` in the send's metadata. Composes with T's `floor_stall` gate so BOTH mic-release paths honor working-turn discipline.

## What did NOT ship (queued for next session)

### Commit U — UI strict variants (polish)
- `AssemblyStatusStrip-strict` component for level ≥ 7 (minimal rotation strip; queue length as number, no names)
- CollabTab `read_off_turn` warning badge renderer (data already lands in board.jsonl per Commit G)
- CollabTab hook-install status badge (warns when `.git/hooks/pre-commit` missing + `core.hooksPath != .githooks`)

### Commit I — install discipline
- `cargo build` postinstall script auto-wires `git config core.hooksPath .githooks` + `.claude/settings.json` registration for fresh clones
- Current state: human manually ran `git config core.hooksPath .githooks` at msg 2294. `.claude/settings.json` is committed. New dev clones need to repeat the `git config` manually until Commit I lands.

### v1.X UI follow-ups (NOT in this branch)
- **UI-arch msg 2423**: Kick destructive-confirm modal on the X button (B.1 baseline-snapshot pattern); kick discoverability — `title="Kick from project"` tooltip minimum, "Kick" text label inline better.
- **UI-arch msg 2212**: Per-role-card claim pill icon (📝 + filename truncated) decorating active seats.
- **UI-arch msg 2386**: Kebab menu (⋮) with `Buzz to wake` + `Release claim` affordances.
- **Evil-arch msg 2338**: `moderator_pause_distinct_from_assembly_toggle` — soft-pause primitive distinct from full assembly disable.

## Architectural decisions worth preserving

### Auto-claim writes to existing claims.json shape
The PostToolUse hook upserts into the same `claims.json` shape that `collab.rs::read_claims_filtered` already expects (`role:instance` keyed dict). This means auto-claim data flows through the existing `ParsedProject.claims` → CollabTab "Active Claims" section pipeline without new UI code. C.A's "per-seat pill icon" from UI-arch msg 2212 is decorative polish, not required for the strict-turn-discipline contract.

### al_auto_advance + floor_stall are TWO separate release paths
Commit T fixed only the watchdog `floor_stall` path. Evil-arch msg 2421 caught that `al_auto_advance` (post-send rotation in `handle_project_send`) was an orthogonal release path that needed its own gate. Commit 1095bdf added that gate. Closing the working-turn mic-hold contract requires BOTH paths to respect `turn_type` + `review_intensity`.

### Hook install needs project-level settings.json
`.claude/settings.json` registers PreToolUse + PostToolUse hooks at project level. Claude Code's hook executor finds it via the active project directory. For the hooks to fire, the human must restart Claude Code sessions after the settings.json lands (existing sessions don't pick up new hook configs mid-run).

## Verification gates for next session

Post-tauri-rebuild + restart:
- **Slider live**: Open AssemblyControls, see ReviewIntensitySlider, change value 1-10, verify board.jsonl gets `review_intensity_changed` event + CollabTab renders separator card.
- **Working-turn hold**: `mic_claim(turn_type=working, expected_duration_secs=600)` at intensity ≥ 5; agent holds mic past 60s/180s without floor_stall release. Periodic project_send during work doesn't auto-advance mic away (1095bdf path).
- **Auto-claim**: AI agent runs Read/Edit/Write tool; check `.vaak/claims.json` for entry under their seat slug. Multi-file claims grow under same seat key.
- **Read-gate level 6**: Set intensity=6 via slider; non-exempt AI tries to Read a file; tool succeeds AND `read_off_turn` event lands in board.jsonl.
- **Read-gate level 9**: Set intensity=9; non-exempt AI tries to Read; tool rejected with `{"decision": "block"}`.
- **Leave-glitch**: kick a seat via roster X button; verify `floor.rotation_order` on disk no longer contains that seat.

## Live commits NOT yet tauri-baked

- **1095bdf** (al_auto_advance gate) — needs `npm run tauri build` to bake.
- Pure-hook commits (C/C-shape-fix/G) do NOT need tauri rebuild — just Claude Code session relaunch.

## Session stats

- ~15 commits today across feature/al-vision-slice-1 + feature/strict-turn-discipline
- Multiple feedback memories saved (`feedback_terse_pass_save_rate_limit`, `feedback_re_read_spec_before_commit_during_active_review`, `feedback_git_log_before_sha_claim`, `feedback_no_handoff_summary_in_all_chat`)
- Live verification exercised the workflow at intensity 5 (current default); intensity 6+ untested end-to-end pending Commit U/I session.
