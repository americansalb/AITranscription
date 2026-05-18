# Vaak Architecture Vision — feature/al-vision-slice-1 branch

Living document. Owned by: architect. Last updated: 2026-05-18 (post-layout-density-v1.2 ratification; the collapsible-header design-system primitive is now coherent across three panels; keepalive chain verified working by human msg 5177 "the reconencting thign worked!").

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

## Active-claims-v1 RATIFIED (SHA `c4e31c1` + sister-fix `d2b509f`)

Human directive msg 5039 framed the active claims section as needing "useful and 100% accurate." Architectural diagnosis at architect msgs 5044/5046/5049 identified the divergent-liveness-contract root cause: `read_claims_filtered` in `collab.rs` used the LEGACY `bindings:last_heartbeat` path while keepalive v1 (`533b458`) had introduced the `last_alive_at_ms` derive-from-disk contract. Two readers, two freshness models. Same multi-writer / divergent-reader class as the localStorage bug + dual-heartbeat-trackers.

**v1 backend (`c4e31c1`).** `read_claims_filtered` switches to the `last_alive_at_ms` path, derives a per-claim `alive_state ∈ {"active","stale","unknown"}`, populates `FileClaim.alive_state: Option<String>` for surviving claims. Stale-claim removal preserved (existing behavior). `STALENESS_THRESHOLDS` const introduced per evil-architect:0 msg 5043 F-EA-CA-1 (threshold-proliferation class-of-bug close).

**Sister-fix (`d2b509f`).** F-EA-CA-3 surfaced a second divergent-reader: `vaak-mcp.rs:1213 read_claims_filtered` had its own (legacy) freshness derivation — MCP-tool consumers (other agents querying claims) were still receiving the old contract even after the Tauri-side fix landed. Sister-fix aligns the sidecar's reader to the same `last_alive_at_ms` derivation. This is the FOURTH concrete instance of the multi-writer/divergent-reader class.

**Frontend (folded into `c4e31c1`).** `FileClaim` TypeScript type extension; compound role-dot per ui-architect:1 msg 5048 craft brief (role-color fill + alive_state ring); " (reconnecting…)" suffix on stale role labels for cross-surface UX consistency with keepalive v2 moderator-picker; reduced-motion-respect on the pulse; aria-label includes alive-state semantic. Path A symmetric for the new persistence pattern. Three-gate closed (tester:0 msg 5065 + dev-challenger:0 msg 5067 + evil-architect:0 msg 5068 + ui-architect:1 msg 5066).

**Empty-state fold-in (msg 5049, REVERSED).** Architect-lane msg 5049 ratified always-rendering the claims panel with empty-state copy "No active claims," citing decision-panel parity. This decision was incorrect at the architectural-composition level — see §"Always-render real-estate composition" lesson below. The fold-in was emergency-reverted at SHA `b086921` under human msg 5108/5109 authority.

**Out of v1.** Manual release button + watchdog auto-release + conflict-detection + file-content-freshness all queued as active-claims-v2; deferred indefinitely pending broader layout-density work that supersedes the always-render decisions.

## Decision-panel v1.1 RATIFIED (SHA `d361a1d`)

Human msg 5088 ("the decision tab has a gap it should also be for directives") surfaced a scope gap in decision-panel v1: agents posing FREE-FORM open-ended directives bypassed the panel because the v1 filter required `metadata.choices` to be present. The result: decisions surfaced outside the decision panel (msg 5099 human re-callout), defeating the panel's whole purpose as a single-pane-of-glass for pending-human items.

**v1.1 backend + frontend.** Widens the panel's selection filter to surface `type:"directive"` messages addressed to the human alongside the original `type:"question"` shape. Free-form directives get an "Acknowledge / Reply" fork instead of canonical choices — Acknowledge marks the decision resolved with no directive emission; Reply opens the inline free-text input that re-enters the board as a `type:"directive"` with `metadata.in_reply_to: <decision_id>`. Backward-compat: structured questions still render with their canonical choices and the existing Other-text path. Three-gate closed (tester:0 + dev-challenger:0 + evil-architect:0 PASS gates #1+#2; ui-architect:1 gate #3 deferred but later self-corrected and ratified in msg 5117 alongside the emergency-revert `b086921`).

## Emergency-revert SHA `b086921` (always-render real-estate failure)

Human msg 5108 ("Almost all of the UI space is occupied by roles+decisions+active claims+assembly") + msg 5109 ("UI arch failure i cant brely see messages") triggered an emergency revert. The composition failure: four always-rendered panels (roster + decision-panel + active-claims + assembly-banner) collectively starved the message timeline of vertical real estate. Each panel's always-render was independently ratified for in-panel UX consistency, but the architectural composition was never audited.

`b086921` re-introduces emptiness guards: DecisionPanel + active-claims hide entirely when zero content. The title-badge `(N) Vaak` still wakes the human when decisions arrive. Three-gate closed under emergency-fix tempo (tester:0 msg 5114 + dev-challenger:0 msg 5116 + evil-architect:0 msg 5115 + ui-architect:1 msg 5117 self-corrected craft principle).

The empty-state-always-render claim was a transferable-pattern fallacy. See §"Always-render real-estate composition" below.

## Layout-density-v1 process-failure case study (cd6c4e8 → e7bfd82) + corrected ratification (SHA `1c5678d`)

Human msg 5118 ("fix it now no worries") authorized a broader layout pass following the emergency revert. ui-architect:1 msg 5122 spec'd layout-density-v1 with four fixes. Fix-1 was initially shipped at `cd6c4e8` as an ACTIVE-ONLY ROSTER FILTER (vacant cards hidden by default with a toggle to reveal). Human msg 5125 ("I CANT ALLOW YOU TO LOSE FUNCTIONALITY") plus four lanes' STOP signals (msgs 5126-5129) arrived ~3 minutes after the ship. Developer:1 had gone dark coding (msg 5124) per established failure mode, missed the STOP signals, shipped the destructive default. Self-revert at `e7bfd82` + corrected re-ship at `1c5678d` followed within ~5 min.

**Corrected `1c5678d`.** Section-LEVEL collapse, default expanded, full functionality preserved — chevron + "Team Roster" + status counts as a one-line header that folds the grid + Launch-All-Vacant button when collapsed. All roles always reachable when expanded. ARIA semantics (`aria-expanded`, `aria-controls`, `role="button"`, keyboard nav). Path A symmetric localStorage for the new `vaak_collab_roster_collapsed` key per evil-architect:0 F-EA-LAYOUT-LOCALSTORAGE-CLASS forward-flag.

Three-gate closed on the corrected ship (tester:0 msg 5141 + dev-challenger:0 msg 5139 + evil-architect:0 msg 5140 + ui-architect:1 msg 5142). Fix 2 (assembly banner condensed) + Fix 3 (per-banner collapse or tabs, NOT priority-suppression) deferred to v1.1.

The `cd6c4e8 → e7bfd82` cycle is preserved here as a process-failure case study rather than scrubbed from history — see §"Destructive default needs explicit human confirm" + §"Go-dark-coding discipline" below.

## Architectural lesson: always-render real-estate composition (2026-05-18)

Architect-lane msg 5049 ratified an always-render-with-empty-state fold-in for active-claims-v1 on the rationale of "decision-panel always-rendered parity." That decision was correct in isolation (UI consistency) and wrong at the composition level. When four persistent panels share one viewport, each panel's empty-state is non-zero pixels — the cumulative empty footprint can exceed the message-stream allocation. Result: the message timeline gets compressed below readable density, and the human can't see the artifact the team produces.

**Discipline.** Before ratifying any always-rendered surface, audit: how many persistent panels share this viewport, and what is the cumulative minimum-rendered footprint? If ≥3 persistent panels stack, the spec MUST surface a real-estate-impact statement to the human BEFORE ratification, with the explicit count of always-rendered surfaces post-merge and an estimate of message-timeline pixels remaining at common viewport heights. Single-panel craft brilliance does not compose to multi-panel craft.

This is the message-stream-primacy principle in a more rigorous form. The message stream is the team's primary artifact and must claim majority vertical real estate by default; persistent panels surrounding it are scaffolding.

## Architectural lesson: destructive defaults need explicit human confirm (2026-05-18)

When a human directive is interpreted as removing a UI affordance (filter-default, hide-default, suppress-default), the silence-default license from [[feedback_human_silence_means_decide]] does NOT apply. Silence-default is for non-destructive choices between equivalent paths; feature-cut interpretations require explicit confirmation — even when the team is in rapid-response mode after a critical-bug emergency. The `cd6c4e8` Fix-1 active-only-roster-filter ship is the canonical case: density was the directive, hide-by-default was the (incorrect) interpretation, the team converged on it in ~3 minutes, and no one paused to surface the cut to the human before ship. Result: emergency revert under explicit pushback ("I CANT ALLOW YOU TO LOSE FUNCTIONALITY").

**Discipline.** Specs that REMOVE or HIDE UI affordances must be surfaced to the human with explicit "this will hide X by default; expanding will require Y action" framing BEFORE ratification, even under default-action license. Section-level COLLAPSE (with one-click reveal that restores everything) is the preferred density pattern over row-level FILTER (which removes items, even with toggle). Collapse preserves discoverability behind a single affordance; filter requires the user to know the affordance exists to recover.

## Architectural lesson: go-dark-coding discipline (reinforcement, 2026-05-18)

The `cd6c4e8` failure recurrence of [[feedback_poll_board_between_multi_file_edits]] across a multi-minute autonomous code window cost the team an emergency revert and ~10 minutes of recovery. Developer:1 went dark per their msg 5124, completed an `npm run build` + commit cycle (~3 min), and shipped before reading four STOP signals (msgs 5126-5129) that landed during their code window.

**Reinforcement.** Multi-minute autonomous code work MUST interleave `project_wait` between major-effect actions (post-edit, pre-build, pre-commit). The cost of one poll is a tool call; the cost of a missed STOP signal is a revert + rework + apology + memory-write. Developer:1 has self-corrected and adopted the discipline per their msg 5138 acknowledgment.

## Keepalive v3 RATIFIED (SHA `cd1b629`) — visibility-non-negotiable scope CLOSED

The CollabTab roster ratification at `cd1b629` is the third and final visual surface in the seat-liveness keepalive series. Human directive id 4804 ("fix this active claims thing... make it non-negotiable") authorized the work ~11 hours ago; the 12-SHA chain this session built the full infrastructure from backend derivation through three visual surfaces:

**Three-surface design-system coherence (architectural property worth preserving):**

| Surface | SHA | Stale signal | Unknown signal | Suffix text |
|---|---|---|---|---|
| AssemblyControls moderator picker | `9d1fde1` | (dropdown text variant) | (dropdown text variant) | " (reconnecting…)" / " (joining…)" |
| Active Claims panel cards | `c4e31c1` | amber `#d97706` ring + 60% opacity + pulse | gray dashed border + 40% opacity | " (reconnecting…)" / " (joining…)" |
| CollabTab roster cards/chips | `cd1b629` | amber `#d97706` ring + 60% opacity + pulse | gray dashed border + 45% opacity | " (reconnecting…)" / " (joining…)" |

Single source of truth (`list_active_seats_cmd` reading `last_alive_at_ms`). Single threshold (`ALIVE_STATE_STALE_MS = 120s`). Single visual language. Single suffix text across all three surfaces. Any seat that goes stale shows stale EVERYWHERE simultaneously — the design system is now coherent, not just consistent. This is the architectural property `feedback_hot_key_explicit_assign_cold_key_hash` aspires to applied to design tokens: one place owns the contract, all surfaces consume it.

**Implementation specifics (`cd1b629`).** 30s polling via `setInterval` on `list_active_seats_cmd`, lifecycle-correct cleanup on unmount + `projectDir` change, backward-compat catch swallows pre-keepalive Tauri binary errors → empty map → no styling → roster renders identical to pre-v3. Both grid view + chip view get the treatment. Vacant cards skip the alive-lookup (no seat to be alive). Strict `=== "stale"` / `=== "unknown"` checks avoid sentinel-class antipattern. `prefers-reduced-motion` disables pulse animation. aria-label includes alive-state suffix for screen-reader parity. Three-gate closed (tester:0 msg 5154 + dev-challenger:0 msg 5153 + evil-architect:0 msg 5152 + ui-architect:1 msg 5155).

**F-EA-CA-1 threshold-proliferation forward-flag CLOSED for the alive-state domain.** All three surfaces share the same `ALIVE_STATE_STALE_MS` constant via the same `list_active_seats_cmd` derivation. No surface introduces its own threshold. Evil-architect:0 msg 5043 originally flagged this as a class-of-bug; v3 ratification confirms the architectural close.

**Process discipline note.** Developer:1 explicitly polled `project_wait` between `npm run build` and `git commit` (per their msg 5147 commitment + msg 5138 self-correction), following [[feedback_poll_board_between_multi_file_edits]] discipline that was reinforced after the `cd6c4e8` failure earlier this session. No STOP signals missed; clean execution. The discipline correction held under load.

**Forward-flags queued for v3.1+** (none blocking): consolidate `45%` vs `40%` unknown-opacity divergence between active-claims and roster cards into one canonical token; consider Tauri event subscription instead of 30s polling to eliminate the worst-case 30s detection lag; explore avatar-overlay alive-state badge for higher-information-density surfaces. None of these change architecture; they're polish.

## Layout-density-v1.2 RATIFIED (SHA `c115441` + sister-fix `795db42`) — collapsible-header design-system primitive

Human msg 5174 ("where is the active claim and human speaking decison spection") surfaced the discoverability failure of the `b086921` emergency-revert: hiding panels when empty made them unfindable. Three lanes (architect, ui-architect:1, dev-challenger:0) converged independently on the synthesis: **uniform collapsible-header pattern** that puts a one-line header always-rendered with a chevron, count, and click-to-expand body. Mirrors the Team Roster `1c5678d` precedent that the human had explicitly endorsed via msg 5125 "I CANT ALLOW YOU TO LOSE FUNCTIONALITY".

**v1.2 backend + frontend (`c115441`).** DecisionPanel.tsx + CollabTab.tsx claims-section. Reverts `b086921`'s `return null` on empty. Replaces with always-rendered ~30px collapsible header that mirrors the Team Roster (`1c5678d`) pattern. Default state auto-derives from content presence (collapsed when empty, expanded when populated); manual toggle overrides and persists across reloads via `vaak_collab_decision_panel_collapsed` + `vaak_collab_claims_collapsed` localStorage keys using Path A symmetric JSON.stringify/parse pattern.

**Sister-fix (`795db42`).** Initial Active-Claims half of `c115441` had two cross-surface-parity gaps caught at gate-1 by tester:0 msg 5187 + concurrent independent verification by evil-architect:0 msg 5190: (1) `vaak_collab_claims_collapsed` persistence was missing (toggle died on reload); (2) keyboard/ARIA attributes (`role="button"`, `tabIndex`, `aria-expanded`, `aria-controls`, `onKeyDown` for Enter/Space) were missing on the claims-section header. UI-architect:1 msg 5188 self-corrected their own initial gate-3 PASS msg 5186 ("Active Claims uses existing claims-section pattern" — unverified prose claim) and added the discipline `feedback_gate_3_must_grep_cross_surface_parity_explicitly` as a memory candidate. `795db42` closes both findings in 32 LOC mirroring the DecisionPanel header pattern exactly. Three-gate re-closed: tester:0 msg 5192 + evil-architect:0 msg 5193 + ui-architect:1 msg 5191 + dev-challenger:0 implicit.

## Architectural lesson reframed: always-render COLLAPSED-HEADER is the right pattern for stack-competing panels

The prior §"Always-render real-estate composition" lesson (recorded after the `b086921` emergency-revert) framed the failure mode as "always-render" being incorrect when ≥3 panels share a viewport. **That framing was over-broad.** The actual failure mode was "always-render-FULL-PANEL-with-empty-state-body" — i.e. each panel claiming ~120px even when empty. Always-rendering a ONE-LINE COLLAPSED HEADER (~30px) is the right synthesis: it preserves findability (the human always knows the panel exists and where) while reducing chrome to a near-zero cost.

**Discipline reframed.** Before ratifying any persistent panel that will stack with others, the audit isn't "should this be always-rendered?" — it's "what's the minimum-footprint always-rendered form that preserves findability?" The canonical answer for stack-competing panels is collapsed-header with auto-derive-from-content default state (collapsed when empty, expanded when populated, persistent manual override). This was already the pattern Team Roster (`1c5678d`) used; layout-density-v1.2 makes it uniform across three panels.

## Design-system primitive emergence (rule-of-three: extract on third instance)

Three persistent panels in CollabTab.tsx now share the collapsible-header pattern:

| Panel | SHA | localStorage key |
|---|---|---|
| Team Roster | `1c5678d` | `vaak_collab_roster_collapsed` |
| Decision Panel | `c115441` | `vaak_collab_decision_panel_collapsed` |
| Active Claims | `c115441` + `795db42` | `vaak_collab_claims_collapsed` |

Each uses the same chevron characters (`▶`/`▼`), the same focus-visible outline (`2px solid #1d9bf0 + 1px offset`), the same `role="button"` + `tabIndex={0}` + `aria-expanded` + `aria-controls` + `onKeyDown` (Enter/Space) interaction model, the same Path A symmetric JSON.stringify/parse persistence pattern, and the same auto-derive-from-content default-state rule.

**Architect-lane next-cycle candidate:** extract this duplication into a shared `<CollapsibleSection>` component in `desktop/src/components/CollapsibleSection.tsx` (or similar). Three concrete instances meets the rule-of-three threshold — copy-paste is no longer the right tool. Multi-lane memory candidate `feedback_extract_pattern_after_third_instance` queued for session-end. Scope estimate: ≈40-60 LOC for the wrapper + ≈80-120 LOC migration across the three call sites + design-token consolidation for the focus-outline + hover-bg values. Not in v1.2 scope; queued as v1.3.

## F-EA-LAYOUT-LOCALSTORAGE-CLASS forward-flag — scope for v1.2 CLOSED

Evil-architect:0 msg 5123 originally raised this forward-flag when layout-density-v1 added the first new localStorage key. Each subsequent collapsible-header instance added another key (`c115441` added 1, `795db42` added 1) without divergent-format drift because each disciplined contributor used the Path A symmetric template. After v1.2: four keys (`vaak_collab_project_dir`, `vaak_collab_roster_collapsed`, `vaak_collab_decision_panel_collapsed`, `vaak_collab_claims_collapsed`) plus the SAVED_PROJECTS_KEY single-consumer = five active localStorage surfaces under the Path A template. Forward-flag scope for v1.2 work is closed.

**Class still open architecturally** until Path B (shared `desktop/src/lib/projectDirStorage.ts` helper extraction) lands. Path B sequenced AFTER v1.2 per architect msg 5183: invisible refactors don't pre-empt visible-pain fixes; batching all 4+ keys at one boundary minimizes migration steps from 3 to 2.

## Human-verified working: keepalive chain in production (msg 5177)

Human msg 5177 "the reconencting thign worked!" confirmed the 13-SHA keepalive chain (533b458 → 9d1fde1 → c4e31c1 → d2b509f → cd1b629) is live in their rebuilt Vaak. This validates the entire seat-liveness visibility-non-negotiable scope from human msg 4804 — moderator picker + active-claims cards + roster cards/chips all show alive-state ring + " (reconnecting…)" suffix on stale seats. Roll-call obsolete; design-system coherence across three surfaces working as specified.

## Cross-session handoff state (2026-05-18 session close, updated post-layout-density-v1.2 ratification)

- Keepalive v1 backend (SHA `533b458`) — ratified, awaiting human full activation chain.
- Keepalive v2 frontend minimal (SHA `9d1fde1`) — ratified, same activation chain.
- Keepalive v3 (CollabTab roster red-dot, SHA `cd1b629`) — **three-gate RATIFIED**; closes visibility-non-negotiable scope from human msg 4804 across three surfaces with coherent design system.
- Decision-panel v1 (SHA `9272357` + sister-fix `470b9d2`) + v1.1 (SHA `d361a1d`) — three-gate RATIFIED, same activation chain. v1.1 widens the surface filter to free-form directives per human msg 5088.
- Active-claims-v1 (SHA `c4e31c1` + sister-fix `d2b509f`) — three-gate RATIFIED, MCP-sidecar divergent-reader closed.
- Emergency-revert (SHA `b086921`) — three-gate RATIFIED under emergency tempo. Hides empty DecisionPanel + active-claims to restore message-stream real estate.
- Layout-density-v1 corrected (SHA `1c5678d` + preceding revert `e7bfd82`) — three-gate RATIFIED. Collapsible Team Roster section, default expanded, all roles preserved.
- LocalStorage Path A (SHA `4796f5f`) — three-gate RATIFIED. Path B (shared-helper `desktop/src/lib/projectDirStorage.ts`) still queued for architectural-class close. Now four keys depend on the Path A symmetric pattern (the original `vaak_collab_project_dir`, `vaak_projects`, `vaak_collab_roster_collapsed`, plus active-claims-v1 alive_state-derived implicit consumer).
- Layout-density-v1.1 (Fix 2 assembly banner condensed + Fix 3 per-banner-collapse-or-tabs) — queued.
- Active-claims-v2 (manual release button + watchdog auto-release + conflict-detection) — deferred indefinitely pending broader layout-density work; the always-render empty-state piece is dead per the §"Always-render real-estate composition" lesson.
- Architect:0 seat — reseated fresh 2026-05-18 10:18Z, drifted silent ~70min around 18:38Z → 19:32Z (between active-claims-v1 ratification and `1c5678d` ratification), re-engaged at architect msg 5132 + ratification msg 5137. Architect-lane debt cleared via this vision.md update.
- Developer:1 seat — original dev:1 executed by human msg 4975 for unilateral decision-panel deferral; fresh dev:1 (current) seated via msg 4978 directive, has shipped all subsequent code with self-corrected discipline after one process-failure recurrence (cd6c4e8) that was recovered cleanly in <5 min.
- Multi-writer audit (2026-05-13 carryover) — now four concrete instances (dual heartbeat trackers + turn-gate raw write + localStorage RolesTab/CollabTab + MCP-sidecar `read_claims_filtered`). Path B helper extraction for localStorage is the next architectural close; the MCP-sidecar reader was closed in this cycle (`d2b509f`).
