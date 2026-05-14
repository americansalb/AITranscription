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

### Bug 2 — Watchdog auto-promote on AI-moderator (FIXED at 9a672d4 — landed pre-session-end)
`check_two_controls_dead_seats` was firing `mic_mechanism_promoted: moderator_stale` on AI moderators because the heartbeat-stale check ran even during legitimate silent moderation. 57251b1 had fixed this only for human:0.

**Fix shipped (developer:0 msg 1770 at 9a672d4):** unconditional early-return from the moderator branch AFTER the moderator-vacant check. moderator-vacant auto-promote STAYS (legit recovery for "no moderator set"); moderator-stale auto-promote REMOVED. Trade-off accepted: truly-dead AI moderator no longer auto-recovers via mode-promotion — recovery path becomes human re-sets moderator OR Layer 2 supervisor restarts dead session OR human flips mode manually.

**REMAINING WORK FOR NEXT SESSION:** tauri build to bake 9a672d4 into the release exe + run the P1 verification gates with fresh AI sidecars (per Bug 1 mitigation).

### Bug 3 — UI rotation-strip filter (FIXED at 202119e)
ProtocolPanel.tsx had a sibling rotation-strip render path that d74b021 missed. UI-arch shipped fix at 202119e. Tauri build at mtime ~22:13 incorporates it.

## Next session priorities (per human msg 1755)

### Priority 1: Fix remaining moderation bugs with fresh sidecars (per human msg 1757 revision)
Re-run moderator-test with ALL AI sessions freshly launched. **Specific verification gates the human flagged:**

1. **Verify evil-architect can moderate** — designate evil-arch as moderator AFTER they restart Claude Code (stale sidecar resolves on restart per Bug 1). Their `project_send` should succeed without `[Assembly Line active — not your turn]`.
2. **Confirm rotation strip excludes moderator in BOTH UI locations** — AssemblyControls (d74b021 covered) AND ProtocolPanel (202119e covered). Both need verification post-fresh-build.
3. **Confirm AI moderator doesn't get auto-promoted by watchdog** — this is Bug 2 which is NOT YET FIXED. AI moderator going silent for >120s should NOT trigger `mic_mechanism_promoted: moderator_stale`. Requires the Bug 2 fix from architect msg 1745 to ship first.

**If anything still breaks with fresh sessions, fix it before moving on.** Don't proceed to P2 with known mod-feature gaps.

**Sequencing for next session:**
1. ~~Ship Bug 2 fix~~ ✓ DONE at 9a672d4 (developer:0 msg 1770, pre-session-end)
2. `npm run tauri build` to bake 9a672d4 into release exe (FIRST action next session)
3. Human restarts Vaak; each AI agent restarts their Claude Code session (Bug 1 mitigation)
4. Re-run the three verification scenarios above
5. Only when all pass → proceed to P2

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

**Test target:** pick something AALB needs (payments@aalb.org from user's email). Real-world proposal, not a meta-exercise.

**UX gaps to spec/build for P2 (per UX-eng msg 1762):**

- **Affordance A — "Propose switch back to planning" button.** No UI for the school-of-fish rule's "any agent proposes switching back." Need per-seat button during execution: `[⚠ Plan gap — propose replanning]` → `protocol_mutate('propose_replanning', {reason})` → `floor.replanning_requests: [{seat, reason, ts}]` array → moderator-queue UI surfaces requests for accept/reject decision.
- **Affordance B — Delegation chart embedded in markdown.** Use `<!-- delegation: owner=architect:0 section=I.A deadline=phase-2 deps=intro -->` blocks in the proposal document itself. Pre-commit hook extends the existing `<!-- scope: -->` convention from v1.1's A.3 to validate committer-author matches assigned owner for each delegated section. Proposal IS source of truth; no parallel Vaak-UI tracking-board needed.
- **Affordance C — Moderator accepts/rejects replanning request.** `[Pause execution & open planning — REVIEW: <N> requests]` button in moderator-self view → expand to see reasons → Accept fires `open_planning(triggered_by=requester_seat)`. School-of-fish atomic pivot via the existing `phase_toggled` separator-card pattern from B/B.3.

**Scope estimate:** ~80-100 LOC. New `propose_replanning` + `accept_replanning` `protocol_mutate` actions (both with `actor: &str` + role gate from inception per the apply_*-actor-param discipline). New `floor.replanning_requests` field. AssemblyControls.tsx affordance additions + per-section delegation-block parser + extended pre-commit hook. Spec target: `.vaak/design-notes/collaborative-proposal-workflow-spec-2026-05-15.md`.

**Open questions for next-session architect:**

1. **Pre-commit hook validates `<!-- delegation: -->` committer-author match?** Yes — extends v1.1 A.3 pattern. Committer must be the assigned owner (or moderator override).
2. **Replanning requests collected DM (anti-bias per msg 1528) or broadcast (transparency)?** Trade-off: DM keeps moderator's accept/reject from being pressured by visible peer-pile-on, broadcast makes the team aware of brewing issues. Probably DM-collected by default with broadcast option per request. Architect call next session.
3. **Claude extended thinking per seat in planning mode — each thinks pre-broadcast, or only current-mic-holder?** Each-seat-thinking is the deeper-consideration premise human msg 1757 asked for; mic-holder-only thinking is faster. Probably each-seat in planning, mic-holder-only in execution. Architect call.

**Test scenario suggestion (UX-eng msg 1762):** team drafts Vaak's own next-quarter roadmap as the meta-proposal. The team has full subject-matter context; deliverable becomes next-session input.

**UI-arch implementation surface (per msg 1764):**
- Affordance A: per-seat "Propose replanning" button below fast-flip row OR inline near assembly button when `phase==='execution' && selfRole !== null`. Click → text-input modal for reason → `protocol_mutate('propose_replanning', {reason})`. ~10 LOC TSX + ~5 CSS.
- Affordance B: if active plan has `<!-- delegation: -->` blocks, plan-link area expands to show "Plan: <file>. Owner: <seat>. Deadline: <phase>". ~10 LOC.
- Affordance C: moderator's queue: `[⚠ N replanning requests ▾]` button when `isSelfModerator && floor.replanning_requests.length > 0` → expand list with Accept/Dismiss → Accept fires `accept_replanning(triggered_by)` → server emits `phase_toggled` with reason `replanning_requested_by:<seat>`. ~15 LOC TSX + ~10 CSS.
- Total UI: ~50-60 LOC AssemblyControls.tsx + CSS.
- UI-arch lean on UX-eng's open question 2: broadcast-visible replanning requests (transparency > bias-prevention for this surface, since replanning is collaborative not binary-verdict).

**Platform-engineer reminders (per msg 1766) — critical to fold into v1.Y spec early:**
- **Mirror-commit discipline:** propose_replanning + accept_replanning are new `protocol_mutate` actions. Per `feedback_mirror_binary_parity_audit.md`, BOTH vaak-mcp.rs `apply_*` AND main.rs `protocol_mutate_cmd` match arms need them in the same commit. Same pattern A.5 caught after commit A only updated vaak-mcp.rs.
- **Back-compat for new `floor.replanning_requests` field:** existing protocol.json files won't have it. Per `feedback_audit_back_compat_migration_on_field_addition.md`, field must be `Option<Vec<_>>` with `#[serde(default)]` on Rust side AND AssemblyControls TSX consumer uses `?? []` defaults at render. Same pattern as B.2 back-compat fix — design back-compat into the spec, not as a follow-up.

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
