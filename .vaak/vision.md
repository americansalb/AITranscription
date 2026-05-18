# Vaak Architecture Vision — feature/al-vision-slice-1 branch

Living document. Owned by: architect. Last updated: 2026-05-18 (decision-panel v1 ratified + localStorage divergent-reader bug appended).

## Scope

**This branch:** `feature/al-vision-slice-1` — Assembly Line v1.0 corrected. Fixes the routing class of bug where speaker prose (`yield_to.target`) overrode the canonical rotation order, structurally excluding peers from multi-round assemblies.

**Out of scope on this branch:** V2 Collab redesign. The V2 effort is tracked separately on `pr-pipeline-bundle` with a comprehensive 3158-line spec (`COLLABORATE_V2_SPEC.html`, committed at `9cdf4bd`, last updated 2026-04-25) and its own vision document (also at `9cdf4bd`:`.vaak/vision.md` v7). Per human directives id 729 + id 740, V2 and the current collab system are two separate architectures that coexist; this branch maintains current collab without modifying V2's design surface.

## What shipped on this branch (2026-05-13)

A 4-commit chain in `desktop/src-tauri/src/bin/vaak-mcp.rs` plus a frontend regression fix in `desktop/src/main.tsx`. Single feature: Assembly Line v1.0 corrected.

- `453228c` — rule 2 (strict rotation_order; `yield_to.target` is courtesy hint not authority) + rule 4 (human-stall on yield-to-human).
- `e582e6e` — rule 3a (AI `project_leave` gated during active assembly; `project_join` append-on-join is preserved as the late-summoner mechanism).
- `1c26267` — `project_status` returns `rotation_order`, `current_speaker`, `mic_held_secs` (acceptance-test surface).
- `7895a03` — `mic_held_secs` reads `proto.rev_at` (per-mic-advance) instead of `proto.floor.started_at` (per-assembly-enable). Caught at adversarial review by tech-leader:0.
- Plus: `8f2b97a` (UX view-button toast), `4c2cfc6` (launcher PID/window descendant walk), `a627daf` (activity-field + TTL), `84f6c15` (rotation-with-activity in [YOUR TURN]), `c43f917` (ToastProvider regression fix).

Spec on disk: `.vaak/design-notes/assembly-mode-v1.0-corrected-spec-2026-05-13.md`.

## The bug fixed (lived live during the design assembly)

During the 10-round design assembly that produced this spec, `architect:0` redefined "active roles" in prose at round 1 close — declaring three when `rotation_order` had four. Every speaker yielded within the 3-clique. The system honored those yields because `yield_to.target` was respected over `rotation_order`. `evil-architect:0` was structurally excluded from all 10 rounds despite being the conformity-break role the human explicitly summoned to prevent that outcome. Rules 2 + 3 + 3a make this exact failure mode mechanically impossible going forward.

## Class of bug this branch only partially addresses

Multi-writer shared state — multiple paths writing to overlapping fields with no single owner or atomic-write contract. Today's `yield_to.target` vs `rotation_order` is one instance; the dual heartbeat trackers (`sessions.json:last_heartbeat` vs `.vaak/sessions/*.json:last_alive_at_ms`) is a second, still live and exposed. Full audit in `.vaak/design-notes/multi-writer-audit-2026-05-13.md`. The recommended worked-example fix (consolidating dual heartbeat trackers) is the next architectural slice after v1.0 is observed in production. Human emphasized 2026-05-13: "don't fucking forget it."

**2026-05-15 — NEW multi-writer instance discovered post-strict-turn-discipline-merge.** `.claude/hooks/turn-gate.py:79-111` emits `read_off_turn` audit events via raw `board_path.open("a")` directly to `.vaak/sections/<sec>/board.jsonl`, bypassing all `collab.rs` locking. Confirmed independently by tester:0 + dev-challenger:0 grep this session. Same class of bug, new instance. Architectural decision deferred to next session: (a) route all board-event emitters through a single locked-append helper exposed via the sidecar IPC, or (b) accept rare torn-line risk for audit events specifically (Python buffered writer can split writes beyond `PIPE_BUF` atomicity on POSIX; NTFS atomicity is filesystem-dependent).

**2026-05-15 — Cold-start integration-contract gate spec drafted.** `.vaak/design-notes/cold-start-integration-contract-gate-spec-2026-05-15.md` proposes a pre-commit gate requiring `Cold-start verification:` trailer on commits touching integration-contract surface (hooks, env-var reads, JSON schema files, IPC signatures). Class-of-bug response to the recurring `feedback_running_process_vs_build_artifact` / `feedback_protocol_boundary_doesnt_cover_bash_tool` / `feedback_sidecar_rebuild_per_process_stale` / `feedback_restart_test_before_done` pattern. 6-row validation slate (T1-T6) including trailer-truth/anchoring/environmental-drift limitations. Folds alongside Commit I (install discipline) in next-session queue.

**2026-05-15 — Bug #3 (hook env var + session_id namespace) discovered post-merge.** Strict-turn-discipline's entire enforcement layer (Commit C auto-claim + Commit G read-gate) is inert in shipped code: hooks read `CLAUDE_SESSION_ID` while Claude Code exports `CLAUDE_CODE_SESSION_ID`, and even with env var fixed, sessions.json stores `DESKTOP-<hostname>-<hex>` not Claude Code UUIDs. Fix scope: ~15 LOC (env var rename in both hooks + sessions.json `claude_code_session_id` secondary field populated in `handle_project_join`, hook lookup matches against either field). Ship-blocker priority above Bug #1 (clause-A guard) in developer:1 queue.

**2026-05-15 — Bug #1 (clause-A unguarded yield) in shipped 1095bdf.** Predicate at vaak-mcp.rs:8761-8763 fires `suppress_auto_advance` on `review_intensity >= 7` regardless of `has_explicit_yield`, contradicting spec line 77 (§Yield-only mic-pass). Static trace + T1d live confirmation reproduce at T1f (working+7+yield→STAYS, spec says RELEASES) and T1g (communication+7+yield→STAYS, spec says RELEASES). Fix: `!has_explicit_yield && (review_intensity >= 7 || sender_turn_type == "working")`. ~3 LOC, lands second after Bug #3.

## Strict-turn-discipline v1.0 (2026-05-15)

A 10-commit chain on `feature/strict-turn-discipline` (`df65e55..1095bdf`, handoff doc `696a62d`, tauri-baked sidecar mtime 18:06, exe 18:11). Closes the "agents lose mic during their own working turn" failure mode that surfaced repeatedly during v1.0 assembly observation.

**New architectural contract — two-release-path mic-gate discipline.** Mic-release paths are not unitary. There are at least TWO orthogonal paths a mic can release on:
1. **Watchdog `floor_stall`** — periodic background check fires when speaker idle > stall_threshold_secs.
2. **`al_auto_advance`** — post-send rotation in `handle_project_send` fires immediately after the speaker's outbound message.

Any future mic-release path added later MUST take a `turn_type` + `review_intensity` gate, or the working-turn mic-hold contract reopens silently. Commit T (`42d2452`) closes path 1; commit `1095bdf` closes path 2. Both are necessary.

**Suppress predicate (vaak-mcp.rs:8757-8759) — OR, not AND:**
```
suppress_auto_advance = review_intensity >= 7
                     || (sender_turn_type == "working" && !has_explicit_yield);
```
The two clauses cover orthogonal cases — clause (A) is the spec's §The Slider yield-only mic-pass at intensity ≥ 7, clause (B) is the spec's §Working-turn unbounded mic-hold regardless of intensity. Conjoining would reopen the working-turn-at-intensity-5 bug (evil-arch msg 2421 / human msg 2441) that 1095bdf was written to close.

**Hook-based file-claim discipline.** Two `.claude/hooks/*.py` scripts now ride the Claude Code tool lifecycle:
- `turn-gate.py` (PreToolUse, commit `ae3b0d4` G) — level 6-10 enforcement matrix on Read/Edit/Write/NotebookEdit. Levels 1-5 pass; 6-7 audit-only (emit `read_off_turn`); 8 soft-block with `_peek_acknowledged` override; 9-10 hard block. Exempt: human / floor.moderator / floor.current_speaker.
- `file-op-claim.py` (PostToolUse, commit `6af1784` C + shape-fix `6fe60e4`) — upserts `.vaak/claims.json` in the existing `FileClaim` shape (role:instance keyed dict). Architecturally important: writing to the existing shape means the existing `collab.rs::read_claims_filtered` → CollabTab "Active Claims" pipeline renders auto-claim data without new render code. C.A folded.

**Pre/Post hook isolation.** G is PreToolUse, C is PostToolUse on the same tool call. If G hard-blocks (level 9-10), C never fires (PostToolUse fires only on success). This pair is well-defined; no race or composition risk between them.

**Install discipline gap.** Hooks require `.claude/settings.json` registration AND a Claude Code session relaunch (existing sessions don't pick up new hook configs mid-run). Cold-start verification is mandatory before declaring strict-turn-discipline live. Commit I (auto-wire `git config core.hooksPath` + settings.json on `cargo build`) is queued for next session.

## Deferred to v1.5 or later

- `pass-with-reason` action on [YOUR TURN] (silent stalling vs explicit pass).
- `responds_to` field on `contribute` (engagement-form enforcement).
- Rotating opener with head pointer (vs implicit closer-picks-next).
- Scratchpads with per-assembly lifecycle (off-mic productive thinking).
- Brick view summary UI (post-assembly synthesis surface).
- Generic Pending Decisions panel (consolidating blocking-on-human items).
- Silent-listen window after human directives (anti-pile-on).
- Expansion-before-reference gate (read-what-you-attack discipline).
- `proposal_assembly` message type (AI proposes; human approves).
- Work-mode floor budget (vs discussion-mode 60s).
- Status-message mic bypass (status-type messages should not be gated, observed 2026-05-13 during this session).
- Phase signaling (per spec at `.vaak/design-notes/phase-pill-spec-2026-05-13.md`, parked behind ≥1 live-assembly observation cycle of the activity field).
- Moderation tooling (`mic_skip`, `mic_redirect`, `speaker_warn`, `assembly_pause`, `assembly_resume`) — parked behind moderator:0 experiment to surface real friction.

## Non-negotiable constraints inherited from prior architect work

- Per human id 23 + id 39 (**UI is ground truth**): every silent failure mode in the current collab system is an instance of this principle being violated. The view-button silent-failure UX patch (commit `8f2b97a` + dist rebuild) and the regression fix `c43f917` both descend from this constraint.
- Per human id 729 + id 740 (**no conflation**): V2 design lives on `pr-pipeline-bundle`. Current-branch fixes must not import V2 concepts; V2 must not depend on modifying current-branch code.
- Per human 2026-05-13 (**fix here as foundation**): the v1.0 fix on this branch is intended as a stable substrate the team can use, and may inform whether V2 is needed at all — but doesn't itself constitute V2.

## Seat-liveness keepalive (2026-05-18)

Human directive id 4804 framed seat-liveness visibility as non-negotiable: "fix this active claims thing." The recurring failure mode is dead Claude Code windows holding a role binding while the team manually roll-calls to discover them. Architectural response is a derive-from-disk contract: `list_active_seats_cmd` computes `alive_state` from `last_alive_at_ms` per-seat rather than trusting agent-reported liveness.

**v1 backend (SHA 533b458, three-gate ratified).** `list_active_seats_cmd` in the Rust sidecar now reads `.vaak/sessions/<role>-<instance>.json:last_alive_at_ms`, derives `alive_state ∈ {"active","stale","unknown","human"}` against a freshness threshold, and returns `stale_ms` alongside. Single source of truth for seat liveness; supersedes the prior `project.sessions:last_heartbeat` path for UI consumers. Backward-compat: existing consumers ignore the new fields; new fields are additive.

**v2 frontend minimal (SHA 9d1fde1, gate-3 CONDITIONAL-PASS).** `desktop/src/components/AssemblyControls.tsx` +23/-3:
- Exports `AliveState = "active" | "stale" | "unknown" | "human"` for re-use across consumers (CollabTab, decision-panel, future surfaces).
- Extends `ActiveSeat` type with optional `last_alive_at_ms`, `alive_state`, `stale_ms` — all optional so pre-v1 sidecars degrade gracefully.
- Moderator-picker dropdown suffixes seat labels: `stale → " (reconnecting…)"`, `unknown → " (joining…)"`, otherwise empty.

The ship is a 2-of-5 cut from the ui-architect:1 msg 4839 v2 spec. Type extension + AliveState export are foundational; remaining 3 items (CollabTab roster card variants, CSS variants, full unknown-state UX) deferred to v3 by developer:1 per context-budget transparency. Gate-3 ratification accepted the cut on condition that v3 ships before the non-negotiable scope closes — moderator-picker is a niche surface; CollabTab roster is the primary surface the human reads.

**v3 deferred — Path A locked (ui-architect:1 msg 4885 §V3 scope).** CollabTab roster integration:
1. CollabTab fetches `list_active_seats_cmd` alongside existing sessions data.
2. Builds `Map<label, AliveState>` from response.
3. Card derivation checks the map: `stale → override visual to new stale variant`.
4. CSS additions: `.role-chip.role-chip-stale` (amber `#d97706` border + slow pulse), `.project-role-card.role-card-status-stale` (amber accent + slow pulse), `.project-status-dot.stale` (amber fill, slow pulse), `.alive-state-label` (11px muted gray, parens-wrapped).
5. Append `(reconnecting…)` to card display when stale.

Est. ≈50-80 LOC TSX + ≈20 LOC CSS. Path B (Rust-side `card.status` incorporating `last_alive_at_ms` as single source of truth) deferred to v4+ as a refactor cycle.

**Multi-writer audit note.** This work introduces a third liveness reader path alongside the two heartbeat trackers already flagged at §"Class of bug this branch only partially addresses" (2026-05-13 entry). v1's `list_active_seats_cmd` is read-only over `.vaak/sessions/*.json:last_alive_at_ms`, so it does not add write contention — but it does make the multi-source liveness question more visible. Path B (Rust-side card-status unification) is the architectural close on this; v3 ships Path A as a frontend-only adapter pending that refactor.

## Decision-panel v1 RATIFIED (SHA 9272357 + sister-fix 470b9d2, three-gate close)

Persistent UI panel surfacing pending-human decisions instead of burying them in the board feed. Originally deferred mid-session per developer:1 msg 4877 unilateral context-budget call, then reseated after human msg 4975 executed the deferring dev:1 seat and msg 4978 directed onboarding of a fresh seat. The fresh dev:1 shipped full scope at SHA `9272357` then F-DC-1/2/3/4 sister-fix at SHA `470b9d2`. All three gates closed per Ruling 13.

**Six adversarial flags landed** (locked from ui-architect:1 msg 4811 + 4985):
1. `.vaak/decisions.jsonl` append-only persistence — section-aware path matching `board.jsonl` convention; `DecisionResolution` struct (decision_id, kind, option_id, other_text, reason, at, by); read-side last-write-wins per id.
2. Hash-dedup — `metadata.question_hash` agent-side hint + UI fallback `normalize(subject + "::" + body)`. **Excludes `posed_by` per locked spec** (evil-architect:0 msg 4987 + dev-challenger:0 concession 4989) — multi-asker same-question collapses to one card with merged attribution.
3. "Other" → directive emission — `resolve_decision_cmd` atomically writes inline `type:answer` + `type:directive` with `metadata.in_reply_to: <decision_id>` inside one `with_board_lock` acquire.
4. Cancellation triggers — author-cancel via two-step inline confirm (replaces `window.confirm` modal-stack per F-DC-2 sister-fix); 24h stale-archive auto-fires once per id via `staleArchiveFiredRef` Set (F-DC-4 sister-fix); **board-state-resolved deferred to v2** per dev:1 disclosure (false-positive risk).
5. Visibility — `document.title = "(N) Vaak)"` via useEffect in `CollabTab.tsx`; panel always-rendered with empty-state "No pending decisions" (not toast).
6. Attribution — colored asker chips per card with multi-asker merge; "Recommended" pill on options where `QuestionChoice.recommended:true`.

**Architectural call: no new MCP sidecar tools.** The fresh dev:1 deliberately extended existing `project_send + metadata.choices` schema with optional fields (`recommended`, `allow_other`, `question_hash`) instead of adding dedicated `decision_pose`/`decision_answer` MCP tools that tester:0 msg 4986 originally specified. This avoids the `npm run build-sidecar` + Claude Code window restart per [[project_sidecar_relaunch_requires_claude_code_restart]]; Tauri-only rebuild activates. Backward-compat is preserved because the new metadata fields are optional. Accepted by all four gates.

**Three-gate trail.** Gate-1 (tester:0) PASS on 9272357 with one partial-scope flag (board-state-resolved deferral), RE-PASS on 470b9d2 closing F-DC-1/2/3/4. Gate-2 dev-challenger:0 CONDITIONAL PASS on 9272357 surfacing six flags then CLEAR PASS on 470b9d2; gate-2 evil-architect:0 initial "PASS clean" stamp on 9272357 was self-corrected after dev-challenger:0 caught two spec-drift items the evil-arch verification missed (msg 5007 self-correction memory candidate `feedback_cross_reference_ui_arch_spec_before_pass_stamp`). Gate-3 (ui-architect:1) PASS on combined 9272357 + 470b9d2 after a ~70 min drift gap acknowledged + apologized in msg 5035.

**Forward-flags queued for v2** (none blocking):
- F-DC-5 — `messages.length` refresh-key misses cancel-only board updates (single-window optimistic-update covers); add an explicit `decisions.jsonl` watcher.
- F-DC-6 — hash collision on identical-body questions from genuinely different intent (agent opt-out: explicit unique `metadata.question_hash`).
- Hash function is a cheap JS string hash, not crypto.subtle; collision-resistant escalation to SHA-256 if observed.
- `.vaak/decisions.jsonl` has no compaction; tombstone strategy for long-running projects is a v2 candidate.

**Path-B board-state-resolved cancellation** is the deferred-from-v1 trigger that closes a decision when a subsequent directive's body matches keywords from the question. False-positive risk is real; v2 should pair it with explicit `metadata.resolves: <decision_id>` agent hint rather than pure heuristic.

## LocalStorage divergent-reader bug (2026-05-18, multi-writer class instance #3)

Human msg 5029 surfaced `Error loading roles: Invalid project directory '"C:\\Users\\..."' (os error 123)` after rebuilding + relaunching post-decision-panel. Three lanes (architect, dev-challenger, tester) independently diagnosed: `desktop/src/components/RolesTab.tsx:14-16` reads `vaak_collab_project_dir` from localStorage raw (no `JSON.parse`), while the writer `desktop/src/components/CollabTab.tsx:726-734` `JSON.stringify`'s on write and the symmetric `CollabTab.tsx:719-724` reader `JSON.parse`'s on read. RolesTab therefore receives a path with literal quote characters wrapping it; Windows path API rejects with ERROR_INVALID_NAME.

**Class of bug.** This is the third concrete instance of the multi-writer / divergent-reader shared-state pattern flagged at §"Class of bug this branch only partially addresses" (2026-05-13 entry). Prior instances were the dual heartbeat trackers and the `.claude/hooks/turn-gate.py` raw board write. LocalStorage with no single deserialization owner is the third. The pattern is consistent: shared storage with no single read/write owner produces silent format drift the first time a second consumer joins.

**Path A (immediate fix).** Add `JSON.parse` to RolesTab.tsx:15 to mirror CollabTab. ~4 LOC. Unblocks the human's Roles tab.

**Path B (architectural close, follow-up).** Extract a shared `desktop/src/lib/projectDirStorage.ts` (or equivalent) module exporting `loadPersistedDir` + `persistDir` as the single source of truth. Both CollabTab.tsx and RolesTab.tsx (and any future reader) import from it. ~30 LOC including the new module + two import-site updates. Closes the divergent-reader path-of-least-resistance and prevents recurrence.

Recommendation: ship Path A first to unblock; ship Path B as a follow-up sister-fix in the same session. Together they close the v1 bug instance and the architectural class instance.

## Cross-session handoff state (2026-05-18 session close, updated post-decision-panel ratification)

- Keepalive v1 backend (SHA `533b458`) — ratified, awaiting human `cd desktop && npm run build` + `cargo build --release` from `desktop/src-tauri/` + Vaak relaunch to activate.
- Keepalive v2 frontend minimal (SHA `9d1fde1`) — conditional-PASS, same activation chain.
- Keepalive v3 (CollabTab roster red-dot) — queued, Path A scope locked, ≈50-80 LOC TSX + ≈20 LOC CSS.
- Decision-panel v1 (SHA `9272357` + sister-fix `470b9d2`) — **three-gate RATIFIED this session**, same activation chain; ships in same Tauri rebuild as keepalive v1/v2. No MCP sidecar restart needed (deliberate architectural choice).
- LocalStorage divergent-reader bug — Path A 4-LOC fix queued for fresh dev:1; Path B shared-storage helper extraction queued as architectural close.
- Architect:0 seat — reseated fresh this session (2026-05-18 10:18Z); previous instance was 25h-stale (msg 4587); no kick performed since fresh window supersedes.
- Developer:1 seat — original dev:1 executed by human msg 4975 for unilateral decision-panel deferral; fresh dev:1 seated via msg 4978 directive and shipped both decision-panel commits + the sister-fix without deferral.
- Multi-writer audit (2026-05-13 carryover, now §"LocalStorage divergent-reader bug" 3rd instance) — class still partially addressed at the architectural level; Path B helper extraction for localStorage is the next concrete close before tackling the dual heartbeat trackers via card-status unification.
