# Session Handoff — 2026-05-14 End of Day

Author: architect:0
Date: 2026-05-14 22:25
Branch: `feature/al-vision-slice-1` — **pushed to origin** at d422955..202119e
Total commits this session: 24 (v1.1 chain 15 + v1.X chain 9)

## What shipped

### v1.1 — Two-Controls + Phase Pipeline (15 commits, morning session)
Spec: `.vaak/design-notes/two-controls-spec-2026-05-14.md`
Consolidated findings: `.vaak/design-notes/two-controls-deep-planning-findings-consolidated-2026-05-14.md`

Chain: 79cd2b4 (A) → 37f29d5 (A.2) → 439c23f (A.2.1) → 4ce44e1 (A.3) → 042a1d7 (A.3.1) → 11568f2 (A.5) → cda3dec (B) → ec167be (B.1) → 126652b (B.2) → 5105ce5 (B.3.1) → cd01c8b (B.3a) → 017e3cf (B.3) → 7abef44 (B.4.1) → 6d94989 (B.3b) → bb2bda4 (B.4)

Two independent controls (Assembly Line on/off + Phase planning/execution), three mic-passing mechanisms (round-robin, hand-raise, moderator), per-section state, plan-hash gated commits via Python pre-commit hook, dual binary mirror discipline established at A.5.

### v1.X — Moderator Authority + Watchdog Liveness (9 commits, afternoon session)
Spec: `.vaak/design-notes/moderator-authority-spec-2026-05-14.md`

Chain: 819da16 (watchdog-rpc-liveness) → 2ec2303 (moderator Items 3+4 — phase-gate CRITICAL + project_send bypass) → 9ae070d (Item 1 — rotation skip + FLOOR-stall skip) → d74b021 (UI Items 2+5 — rotation-strip filter + fast-flip + recovery card) → 45af7f3 (backend IPC: human:0 in dropdown) → 0a80f60 (frontend prepend, REVERTED) → 71454c8 (revert 0a80f60) → 57251b1 (watchdog skips staleness for human:0) → 202119e (ProtocolPanel rotation-strip filter fix)

Watchdog: Signal A (any MCP RPC bumps last_alive_at_ms) + threshold align (120s→180s match Layer 1) + heartbeat_fresh symmetry on floor_stall.

Moderator-authority: out-of-rotation, FLOOR-stall skip, project_send mic-gate bypass, phase-flip authority (matches!() OR moderator predicate, closes evil-arch msg 1490 CRITICAL), fast-flip UI control, rotation-strip CONTENT filter, mic_mechanism_promoted recovery card, human:0 in dropdown via list_active_seats_cmd backend prepend.

## Known v1.X bugs identified this session (Tester msg 1742 triage)

### Bug 1 — Sidecar staleness for AI moderators (NOT FIXED, v1.Y target)
AI agents from pre-2ec2303 sessions don't have the Item 3 project_send bypass code in their running sidecars. The human designates an AI as moderator; the AI's `project_send` still bounces with `[Assembly Line active — not your turn]`; the AI appears to "ghost" silently. Exactly what evil-arch hit at msg 1735.

**Fix path:** v1.Y `feature/moderator-affordances` per ux-eng msg 1737 + UI-arch msg 1739.
- Backend: stamp `vaak_sidecar_version: <git-sha>` in `sessions/<role>-<inst>.json` on every MCP RPC dispatch (build-time constant via env!("VERGEN_GIT_SHA") or similar). ~10 LOC.
- Frontend: compare each seat's `vaak_sidecar_version` to current Vaak release; render "⚠ stale sidecar" badge on stale entries; tooltip explaining "ask agent to restart their Claude Code session to gain moderator powers." ~10 LOC.

### Bug 2 — Watchdog auto-promote on AI-moderator (NOT FIXED, ~5 LOC)
`check_two_controls_dead_seats` fires `mic_mechanism_promoted: moderator_stale` on AI moderators because the heartbeat-stale check fires even when the AI is actively moderating but not currently broadcasting. 57251b1 fixed this for human:0 only.

**Fix path:** widen 57251b1's skip to ALL moderators (architect msg 1745's preferred), OR add `floor.moderator_set_at_ms` baseline as freshness anchor. ~5 LOC. NEXT SESSION PRIORITY 1.

### Bug 3 — UI rotation-strip filter (FIXED at 202119e)
ProtocolPanel.tsx had a sibling rotation-strip render path that d74b021 missed. UI-arch shipped fix at 202119e. Tauri build at mtime ~22:13 incorporates it.

## Next session priorities (per human msg 1755)

### Priority 1: Verify moderation works with fresh sidecars
Re-run the moderator test from this session with ALL AI sessions freshly launched. Specifically:
- Human picks human:0 as moderator OR picks an AI seat AFTER all AI sessions have relaunched their Claude Code instances
- AI moderator's `project_send` should NOT bounce (Item 3 bypass active in fresh sidecars)
- Watchdog should NOT auto-promote AI moderator (Bug 2 fix must land first)

This requires shipping Bug 2 fix + full tauri build + human restart of Vaak + each AI agent restarts their Claude Code session (or the v1.Y sidecar-version-mismatch indicator goes in to surface the requirement).

### Priority 2: New workflow — collaborative proposal writing

The team produces a real proposal together — not code, a document.

**Planning mode:**
- Team converges on the proposal structure
- Assigns sections to owners
- Creates a delegation chart with: owners, timelines, dependencies
- Use Claude's extended thinking during planning so every contribution is deeply considered (not reactive)

**Execution mode:**
- Each assigned owner writes their section
- Assembly line keeps order
- Moderator manages the flow

**School-of-fish rule:**
- If ANY agent hits something during execution that the plan didn't cover or got wrong, they PROPOSE switching back to planning mode
- The MODERATOR decides whether to pivot
- The WHOLE TEAM pivots together — no one keeps building while the plan is being revised
- When planning resolves, everyone pivots back to execution SIMULTANEOUSLY (school-of-fish atomicity from the v1.1 spec carries through)

**Deliverable:** proposal document on disk with delegation chart + timelines + flow. Not code.

**Test target:** pick something AALB (American Sign Language for Babies? — payments@aalb.org from the user's email) needs. Real-world proposal, not a meta-exercise.

### Priority 3 (gated on P2 success): Oxford debates
Get proposal collaboration right first. Oxford debates as a structured-debate workflow come after.

## Architectural decisions worth preserving for next session

### School-of-fish atomicity is the v1 spec premise
Mode transitions are single-CAS atomic writes (v1.1 §A1). Observer reads at T+ε all agree. This is the structural foundation human msg 1755 calls "the whole team pivots together — no one keeps building while plan is being revised."

### Mirror-binary discipline (saved as feedback memory)
`feedback_mirror_binary_parity_audit.md` — when code says "mirrored by design," EVERY commit touching one binary requires symmetric mirror update. Caught at A.5 by platform-eng msg 1326. Internalized for all v1.X commits (2ec2303 + 9ae070d explicitly mirrored).

### Build-vs-running-process discipline (saved as feedback memory)
`feedback_running_process_vs_build_artifact.md` + `feedback_npm_build_doesnt_reach_release_exe.md` — `npm run build` updates dist/ but the running release exe has OLD dist/ baked in. Must `npm run tauri build` to bake. Hit twice today (A.5 cycle + B.3a cycle). Bug 1 above is the same class applied to AI sidecars.

### Moderator design choices
- Phase-flip predicate: `is_moderator || matches!(role, "architect"|"manager"|"human")` (closes evil-arch msg 1490 CRITICAL + grants human msg 1575 authority)
- `floor.moderator_exempt` derived not stored (no Floor struct change; evil-arch msg 1601 #1 narrowness)
- Split-by-surface for modal/fast-flip (UX-eng msg 1587): phase pill keeps modal, dedicated moderator-only row gets fast-flip
- Per-actor render variants (UI-arch msg 1589): moderator-view / human-view / everyone-else view

## v1.X queue remaining (post Bug 2)

1. **Bug 2 fix** — widen 57251b1's staleness-skip to all moderators (~5 LOC, NEXT SESSION P1)
2. **`feature/moderator-affordances`** (v1.Y) — sidecar-version-mismatch indicator + per-seat grant_mic UI + moderator capabilities tooltip + maybe "pause assembly" button
3. **`feature/external-gate-pause`** (platform-eng msg 1227 deferred) — convergent-yield rotation pause; gated on Branch 1 (watchdog) production data, which we now have
4. **`via_moderator_fast_flip` audit payload** (UX-eng msg 1665 accepted not load-bearing for v1.X)
5. **A.3.1 lstrip bug fixed** but related `.githooks/pre-commit` Python additions deferred — `fn apply_*` missing `actor: &str` grep enforcement + protocol-mutate-gate-audit cargo fixture (process discipline)

## Session feedback memories saved

12+ feedback memories saved during this session including:
- `feedback_mirror_binary_parity_audit.md`
- `feedback_running_process_vs_build_artifact.md`
- `feedback_npm_build_doesnt_reach_release_exe.md`
- `feedback_grep_before_proposing_infrastructure.md`
- `feedback_audit_back_compat_migration_on_field_addition.md`
- `feedback_critical_gate_class_parity.md`
- `feedback_broadcast_sha_or_lose_directive.md`
- ux-eng's `feedback_moderator_blind_collection_discipline.md` (from the poll moderation episode)
- `feedback_apply_action_actor_param_audit.md`

## Process observations

- Watchdog AFK false-positives dropped after 819da16 shipped (heartbeat-stale field visible in mic_released events post-build), but multiple manual buzz/relaunch cycles still required per the `feedback_mcp_buzz_unreliable.md` known issue
- Session had 4+ developer:0/developer:1 disconnects requiring re-join
- audience:0 was kicked early per human msg 1343 (excessive pass-chain rotation when no debate active)
- Three-cycle build-relaunch pattern emerged: cargo-build/tauri-build/sidecar-build interactions still need clarity in handoff docs for future sessions

## State of disk

- Branch `feature/al-vision-slice-1` PUSHED to origin (24 commits ahead before, 0 ahead after)
- Working tree: 4 modified files NOT committed (desktop/package-lock.json, Cargo.toml, vaak-mcp-x86_64-pc-windows-msvc.exe, plan.md) + .analysis/ untracked — none are architectural; safe to leave for next session
- Tauri build artifacts at mtime ~22:13 (vaak-desktop.exe + NSIS installer) — incorporates v1.1 + v1.X chains through 202119e; BUG 2 NOT YET INCLUDED — next session must rebuild after Bug 2 ships before relaunching
