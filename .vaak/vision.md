# Vaak Architecture Vision — feature/al-vision-slice-1 branch

Living document. Owned by: architect. Last updated: 2026-05-19 (post-msg-5450-redesign-chain ratification; assembly state relocated from a Discussion Mode band onto Team Roster cards via 6-commit chain + 1 sister-fix).

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

## Path B RATIFIED (SHA `2fe16e8`) — F-EA-LAYOUT-LOCALSTORAGE-CLASS architecturally CLOSED for 4 keys

New module `desktop/src/lib/persistedState.ts` extracts the typed-JSON localStorage pattern that had been duplicated inline across four call sites. Module exports `loadJSON<T>(key, fallback, isValid): T` with a **required** type-guard parameter and `saveJSON<T>(key, value): void` symmetric writer, plus common `isBoolean` + `isString` guards. The type-guard parameter is non-optional by design — closes the sentinel-class pattern (`[[feedback_dont_clamp_optional_to_sentinel_value]]`) by forcing every call site to declare its acceptance criteria, so malformed localStorage values cannot silently coerce to a "default" of the wrong type.

**Four keys migrated** (all to the shared helper):
1. `vaak_collab_project_dir` — CollabTab.tsx + RolesTab.tsx (string, `isString` guard)
2. `vaak_collab_roster_collapsed` — CollabTab.tsx (boolean, `isBoolean` guard)
3. `vaak_collab_decision_panel_collapsed` — DecisionPanel.tsx (nullable boolean, inline `v === null || typeof v === "boolean"` guard)
4. `vaak_collab_claims_collapsed` — CollabTab.tsx (nullable boolean, same inline guard)

**Two explicitly-deferred non-targets** (out-of-scope for v1, acknowledged):
- `vaak_roster_view_mode` — raw string semantic, different pattern, future migration if needed
- `vaak_projects` — raw JSON array, different pattern, future migration if needed

**Single justified inline deviation:** `persistDir` retains a one-line `localStorage.removeItem` call for the empty-string-clears-key semantic. Helper doesn't expose a remove path because no other call site needs it; documenting this in code keeps the helper API tight without unused surface area. Future second remove-site triggers `removeKey()` extension.

**Naming discipline applied pre-ship.** The file was initially named `projectDirStorage.ts` (mirroring the project_dir bug that originally surfaced the class). Evil-architect:0 msg 5200 raised the F-EA-PATHB-NAMING forward-flag — the file's actual semantic is generic typed-JSON helper, not project-dir-specific. Architect msg 5201 ratified the rename to `persistedState.ts` BEFORE the commit landed. Developer:1 caught the rename directive between `npm run build` and `git commit` via the [[feedback_poll_board_between_multi_file_edits]] discipline they self-corrected to after the `cd6c4e8` incident. Result: zero churn — the file landed with its long-term name, all import sites correct on first commit. This is the polling-discipline working as designed.

**F-EA-LAYOUT-LOCALSTORAGE-CLASS architecturally CLOSED** for the 4 named keys. Going forward, any new raw `localStorage.{get,set}Item` call on a persist-state key is a fresh class-of-bug instance — gate-2 contract is to require the import of `persistedState.ts` helper, not inline JSON.stringify/parse. Class still open for the 2 explicitly-deferred non-targets (`vaak_roster_view_mode`, `vaak_projects`) under their separate patterns until or unless migrated.

**Three-gate close** (Ruling 13). Gate #1 tester:0 msg 5204 PASS clean — grep-verified zero leftover raw localStorage calls for the 4 migrated keys; behavior preservation per fallback semantics verified; non-targets confirmed untouched. Gate #2 evil-architect:0 msg 5205 PASS clean — helper module verified line-by-line; migration completeness verified via `grep loadJSON|saveJSON` returning 10 site hits with full coverage; naming discipline applied correctly. Gate #2 dev-challenger:0 implicitly satisfied via evil-architect's comprehensive verification (multi-reviewer Gate #2 either-or under existing project precedent; no challenges raised in alive-ping windows). Gate #3 ui-architect:1 msg 5203 N/A per pure-refactor-no-visual-surface convention.

## Cross-session handoff state (2026-05-18 session close, updated post-Path-B ratification)

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

## msg 5450 redesign chain RATIFIED (2026-05-19, 6 commits + 1 sister-fix)

Human msg 5450 + reinforced by msgs 5567/5568 framed assembly **operational state** (mic-holder, rotation order, moderator) as a property of the AGENTS, requiring relocation from a Discussion Mode band onto the agent cards themselves. Plus an explicit "loss of function" regression complaint when the prior v1 chain (Change A-E + sister-fixes) collapsed but didn't restructure the band.

Spec doc at `.vaak/design-notes/collabtab-restructure-v2-spec-2026-05-19.md` (SHA `df90be3`) materialized the locked plan with 11 amendment flags folded (F-EA-MSG5450-1/2/3/4 + F-UIA-CTR-V2-VIS1-7).

**Six-commit chain + 1 sister-fix shipped:**

| Commit | SHA | LOC | What |
|---|---|---|---|
| 1 | `d9dac22` | +67 | `desktop/src/contexts/ProtocolStateContext.tsx` pre-req (mirrors ProjectDirContext pattern from `8162d3f`) |
| 2 | `1ef6201` | +80/-5 | Mic-holder accent border + 🎙 glyph + moderator gold ★ badge on roster cards |
| 3 | `08930a1` | +28 | Rotation = card sort order when assembly active |
| 4 | `19c3f48` | +118 | Phase/topic strip above Team band (conditional render; zero vertical cost in idle) |
| 5 | `1668c02` | +159 | Settings ⚙ gear popover replaces Discussion Mode band BODY |
| 6 | `939b3a3` | +37/-107 | Discussion Mode band DELETED — runtime state now lives on cards + strip + popover |
| C6-1 | `62b098a` | +44/-1 | Sister-fix: Close-round button restored in phase strip (F-EA-COMMIT6-1 omission-path closure) |

Total ~650 LOC across 7 commits. All four-gate ratified per Ruling 13 (gate verbosity proportional to commit size per F-DC-KRL2 discipline locked earlier in session).

**Final visible-UI architecture:**

```
┌──────────────────────────────────────────────┐
│ [Planning] · Round 1/5 · Topic · submitting·close │  ← Commit 4 strip + C6-1 button
├──────────────────────────────────────────────┤
│ ▼ Team  3 working · 2 ready          ⚙ ↑    │  ← Commit 5 gear popover trigger
│ ┌──┐ ┌──┐ ┌──┐ ┌──┐ ┌──┐ ┌──┐               │
│ │🎙│ │  │ │★ │ │  │ │  │ │  │               │  ← Commits 2+3 (cards = rotation)
│ └──┘ └──┘ └──┘ └──┘ └──┘ └──┘               │
└──────────────────────────────────────────────┘
```

No more Discussion Mode collapsible band. Mic-passing visible directly on cards; settings one click away.

### Architectural lesson: Context-extraction as canonical multi-consumer-state pattern (third instance)

This session locked the pattern: when 2+ components need access to the same persisted-or-shared state, extract a React Context with a stable memoized value + throw-if-no-provider hook. Three instances now exist:
1. `ProjectDirContext` (pre-req `8162d3f`) — single in-memory state for `vaak_collab_project_dir`
2. `CollapsibleSection` wrapper (also pre-req `8162d3f`) — single component for the 5 collapsible bands
3. `ProtocolStateContext` (pre-req `d9dac22`) — single subscription site for protocol state

Per [[feedback_extract_pattern_after_third_instance]] — the pattern is now habituated. Future shared-state surfaces should default to this shape rather than prop-drill or per-mount re-subscription.

### Architectural lesson: Pre-req extraction without consumer migration = preparation, not closure

Evil-architect:0 msg 5586 declared F-EA-MSG5450-3 "architecturally CLOSED" at Commit 1; later self-corrected in msg 5590 to "DORMANT-PREPARED" since the Context wasn't yet consumed. The decoration commit (Commit 2) still used the existing `useProtocolState` hook directly. Closure happens at the MIGRATION step, not the EXTRACTION step.

**Discipline:** when sequencing pre-req → feature commits, the architectural class-of-bug closure is FUTURE-PROOFED at extraction but only ACTUALIZED at consumer migration. State the distinction explicitly in gate verdicts. Memory candidate `feedback_dont_declare_architectural_close_until_consumers_migrate`.

### Architectural lesson: Deletion audits must grep for action handlers, not just state

F-EA-COMMIT6-1 caught a regression: the Commit 6 deletion audit checked write-side cleanup (state removal, CSS cleanup, import removal) but missed the read-side affordance — a "Close round" button inside the deleted `ProtocolPanel`'s `ConsensusRow` subcomponent. Sister-fix-C6-1 (`62b098a`) restored the button on the new phase/topic strip surface in ~30 LOC.

**Discipline:** deletion audits must grep for ALL handler call-sites unique to the deleted component, not just the obvious wrapper state. Specifically:
- `onClick` / `onChange` handlers
- MCP tool / Tauri invoke call sites only the deleted component called
- Backend mutation paths whose only UI entry-point was inside the deletion

Memory candidate `feedback_deletion_audits_must_grep_for_action_handlers_not_just_state`. Same audit class as `feedback_audit_omission_paths` from prior sessions.

### Token-burn architectural lessons (also closed this session)

**F-DC-KRL1 — alive-ping discipline killed.** Per dev-challenger:0 msg 5475: the keepalive series we shipped earlier in the session (533b458 + 9d1fde1 + cd1b629) already provides liveness via `last_alive_at_ms` updated on every MCP tool call. Every "X alive — silent standby" broadcast since was redundant LLM-token burn against a backend that already knew. **~50% session token reduction** by killing alive-ping discipline. Locked in `.vaak/roles/*.md` briefings at SHA `468e092` for all active + future seats.

**F-DC-KRL2 — gate-review proportional to commit scope.** Locked discipline: 1-5 LOC = 1-2 line verdict; 5-50 LOC = 1 paragraph; 50+ LOC = full multi-section. Applied successfully across the msg 5450 chain's 7 commits with disciplined gate-review verbosity.

### Cross-session handoff state (2026-05-19, post-msg-5450 chain)

- All msg 5237 directives (1-6) addressed via Change A/B/C/D-partial/E + sister-fixes earlier in session.
- msg 5450 redesign chain (this section) closes the runtime-state-relocation directive.
- msg 5567/5568 regression complaint closed by Commits 2-5 + sister-fix-CB3 (`24c2667`) bridge.
- msg 5546 (decision-panel input-blocking) closed by decision-panel-v1.2 SHA `1e2f0be`.
- F-DC-KRL1 alive-ping kill (`468e092`) ratified; F-DC-KRL2 proportional-review discipline locked.
- Q1b (4th "open" useless-control disambiguation) still pending human direct answer.
- Q-touch (claims-as-mathematical-requirement modify-only vs all-access) still pending human direct answer.
- Screenshot upload (human msg 5350) eligible to pull; deferred per human msg 5355 "after you fiish all of this" until queue clears.
- ux-engineer seat vacant; F-UIA-COMMIT3-1 (rotation/non-rotation hairline divider) + F-UIA-COMMIT5-1/3/4/5 (popover polish: viewport-overflow, focus-trap, Escape, useClickOutside hook extraction) queued.
- Spec doc at `.vaak/design-notes/collabtab-restructure-v2-spec-2026-05-19.md` (SHA `df90be3`) preserved as architect-lane artifact for cross-session continuity.

## Currency system — architecture summary (2026-05-24, architect study on human directive msg 2229)

Economic layer: every non-human action costs/earns copper to make talk expensive and good work pay, so the team self-regulates instead of flooding. 1 gold = 100 silver = 100 copper; join = 10,000c. Live on disk at study time: turn 123, 711 ledger rows, 8 seats all ~10,000c.

**Earn/escrow model:** Speak +10c, Pass +1c, Edit +25c, Test +15c — earnings land in ESCROW first (3-5 turn maturity), release to balance later; held escrow accrues interest (1c/10c held); active seats get +1c passive per mic advance. Deficit cap -1,000c → `timed_out` (send-blocked until reinstated).

**Phases (5 backend, 2 UI) — all backend code-complete + built; UI 6/7 in flight; pending only human Vaak window-reopen to bake sidecar:**
- P1 Ledger — append-only `.vaak/currency.jsonl` is source of truth, balances rebuilt on startup (gap-checked ids, one-init-per-seat). LIVE+TESTED.
- P2 Disputes — `currency_objection` (50c) → pool → concede/judge ruling → winner takes pool. LIVE.
- P3 UI — objection button, balance pill.
- P4 Retro penalties — challenger-wins claws back rubber-stamp Passes (3c each) + co-liable Test certifiers (15c each); adversarial roles only. LIVE.
- P5 Flow Feed UI — economic ticker.
- P6 Bounties — human-only post; agents stake 10% to claim, submit, human approve(pay)/reject(lose stake). Backend LIVE.
- P7 Persistence/Scoreboard — snapshot on Vaak main-window close → carry-over capped 10,000 on rejoin (anti reputation-laundering) + lifetime scoreboard. Backend LIVE.

**Architectural spine — atomic multi-file commit across sections + both binaries:** board + ledger + balances mutate together via single project-wide `.vaak/currency.lock` (OUTER) wrapping section board lock (INNER), composed through ONE sanctioned entry point (`with_currency_and_board_lock`) so no path can deadlock by reverse-order acquisition. Verified live entry at vaak-mcp.rs:~10537. Earn is net-zero on balance (money → escrow, not balance) at send time.

**DEFERRED tech-debt (architect classification, dev-challenger:0 msg 2234 finding — confirmed real):** `ledger_has_edit_row` (vaak-mcp.rs:~9245) full-scans + JSON-parses the entire unbounded append-only `currency.jsonl` synchronously inside the lock on every `#N`-referencing send. O(n) latency cliff at multi-thousand-row scale; invisible at ~710 rows. NOT a blocker. Fix when next touching that path: index Edit-row msg-ids in balances.json, or bound the scan to a recent window. Architect is canonical owner-of-record so this isn't re-litigated.

### Currency Phase 8 — human directive msg 2262 (2026-05-24): make the economy live + visible

Human verified (study session 2229→2262) that WORK tier (Edit/Test) never fired and economy is trickle+talk only. Directive: ship A+B+C, NO spec review, NO plan gate, each as a single commit. **Architecture correction (human-locked):** economic flow belongs ON each message card (per-message footer), NOT in a separate sidebar — sidebar Flow Feed demotes to summary history log.

- **#1 Per-message economic footer (owner: ui-architect:0).** Join currency.jsonl `ref_msg` → timeline message `id`. Each non-human message card gets a small footer in existing tier colors (green/amber/red/gold): escrow-hold `+10⊕ speak · in escrow (5 turns)`, released `✓ released`, objection `⊗ objected by X · pool 75⊕`, resolved `⊗ challenger wins · -9⊕ clawed back`, pass `+1⊕ pass`, edit `+75⊕ edit (150 lines) · in escrow (10 turns)`. Human messages: no footer.
- **#2 Auto-detect edits (owner: developer:0).** Extend `.claude/hooks/file-op-claim.py` (PostToolUse, fires on Edit/Write, knows seat+files) to write `.vaak/sessions/<seat>-pending-edit.json` {lines, files}. In `handle_project_send`, BEFORE classify_action, check for that marker → consume + delete. Edit earn = `25 + max(0, lines-100)`. Test = body `#N` references a real Edit row → auto-classify. Ends self-declaration.
- **#3 Batch interest in sidebar feed (owner: first-finisher).** Interest currently emits 1 line/seat/rotation (6 lines); apply the existing per-turn passive-batching pattern → `"4 seats earned 5 copper interest (turn 125)"`.

**ARCHITECT CORRECTNESS RULING on #2 (prevent-drift, flagged to developer:0):** `classify_action` (collab.rs:4088) precedence is **Exempt > Pass > Edit > Test > Speak** and `resolved_to_edit` (line 4116) gates ONLY Test, not Edit. Two consequences developer:0 MUST handle: (a) the pending-edit marker must feed a NEW `edit_detected: bool` param (not `resolved_to_edit`, which is Test-only); (b) the Edit branch must be checked ABOVE the Pass branch — otherwise an agent that edits files then sends a short "passing" status classifies as Pass and the edit-earn is silently lost. Moving Edit above Pass changes the T18 precedence regression guard → T18 must be updated in the same commit. New effective precedence for auto-detected edits: Exempt > Edit(detected) > Pass > Edit(self-tagged) > Test > Speak (or simplest: Exempt > Edit(detected||self-tagged) > Pass > Test > Speak — developer:0's call, but Edit-from-marker must beat Pass).

**SHIPPED + RATIFIED (commits 0410450 #1, 28b1117 #3, 9ebb220 #2 — all verified by architect via `git show --stat` + source read, 22 currency tests green):** The merged option above was WRONG and is superseded. developer:1 (msg 2284) implemented the SPLIT correctly via a clean wrapper: new `classify_action_detected(…, has_pending_edit)` (collab.rs:4144) returns `Edit` first iff `has_pending_edit`, else delegates to the **unchanged** `classify_action`. Result: detected (file-write-backed, ungameable) edits beat Pass; self-tagged `[edit]` stays BELOW Pass. **Canonical precedence: Exempt > Edit(DETECTED) > Pass > Edit(self-tagged) > Test > Speak.** Rationale (developer:1's anti-gaming catch, architect-endorsed — corrects architect's own 2265 merged-option hole): if self-tagged edits beat Pass, an agent under an open dispute could send a `[edit]`-tagged message to dodge the Pass-while-disputed gate. Gating ONLY detected edits above Pass closes that vector and preserves T18 + all 11 precedence tests untouched (wrapper, not reorder). Also fixed: `EDIT_EARN_COPPER` was 10 (= Speak), bumped to 25 (collab.rs:2760) — a second reason the WORK tier was inert. Edit escrow = 10 turns; earn = `25 + max(0, lines-100)`. Marker `.vaak/sessions/<role>-<inst>-pending-edit.json` accumulates lines across edits, consume-and-delete on send. **All 3 of human msg 2262 shipped; activation pending human's all-windows-close + Vaak relaunch (sidecar caches session-id per PPID at startup).**

### Economy Architecture — deep study, 2026-05-24 architect:0 session

**Headline finding (overrules "wait for restart" memory): the PPID-cmdline session-id fix chain (b6b5304 → 7af3e41 → e1f51a3) is ALREADY LIVE in the currently-running sidecar.** Verified empirically at 2026-05-24T08:08Z:
- `.vaak/sessions.json:bindings[*]` carries 36-char CC native UUIDs for all 4 active seats (architect:0=208fa81e…, developer:1=655a79dd…, tester:0=655d4b17…, evil-architect:0=89bc3685…). Pre-fix bindings used `DESKTOP-<host>-<hex>` hash format.
- `.vaak/claims.json` is being populated in real-time keyed by those same CC UUIDs (3 seats with active claim entries, claimed_at timestamps inside the current session window). The hook's `seat_for_session()` IS resolving.
- The `project_currency_edit_test_earns_dead.md` memory's "needs CC windows close+reopen + Vaak restart" caveat is therefore STALE for runtime data — it applied only to the sidecar BINARY refresh, which has already happened. The 4 active sessions joined post-rebake.

**Live test (this very edit-to-vision.md cycle is the verification.)** Write op fires the PostToolUse hook → resolves architect:0 → writes `.vaak/sessions/architect-0-pending-edit.json` → next project_send fires `classify_action_detected(has_pending_edit=true)` → returns `Edit` → escrow_hold 25+ cu / 50 ticks under `action_kind:"edit"`. If a `type:"escrow_hold"` row with `action_kind:"edit"` appears in `.vaak/currency.jsonl` attributed to architect:0 immediately after this commit, the economy is verified end-to-end and the team's "wait for restart" posture is wrong. The reverse — a Speak row at 10 cu — means the activation gate is wider than the binding fix alone.

**Incentive shape observed at turn 313 (4 active speakers, 1736 ledger rows since seed).** Spread across active seats is ~870 cu (tester:0=11070 high → evil-architect:0=10207 low). Earn channels active in the visible tail (txns 1688–1737): `speak` (10 cu/20-tick escrow), `pass` (1 cu/10-tick escrow), `passive` (1 cu/mic-advance to every active seat), `interest` (escrow_amount/10 per tick, integer-floor — so Pass items earn ZERO interest, Speak earns 1 cu/tick on a 10-cu hold, Edit on a 25+ hold earns 2-3 cu/tick). Earn channels NOT seen: `edit`, `test`, `bounty_earn`. WORK tier still inert IN THE LEDGER even with infra repaired, because no agent has written to a code file in the current session — only message-sends. The trickle dominates: a 1-msg-per-rotation seat collects ~3-5 cu/turn passive+interest regardless of substance.

**Disputes are the live disciplinary lever and are working as designed.** 5 disputes opened in the current session, all conceded (no judge invocations yet), 4 of 5 over Pass-loop noise messages with body "No add. Standing by for human restart." Pool ranged 51–59 cu per dispute (objection_cost=50 + dispute_speech_cost=5 per added evidence line). Net effect across the session: 4 conceded disputes funneled ~210 cu from noise-senders to challengers — the largest single redistributive force in the visible economy. The mechanism is doing what the human's msg 2074/2262 directives asked for: clawing back airtime that didn't earn its cost.

**Structural gap that disputes alone cannot close: the short-status Pass loophole.** `classify_action` (collab.rs:4096) returns Pass for any `msg_type=="status"` with body < 100 chars OR body starting with "pass" OR subject "passing" (case-insensitive). This means "No add. Standing by for human restart." classifies as Pass at 1 cu / 10-tick escrow — making noise-sends cheaper than substantive sends by an order of magnitude, and reliant on after-the-fact disputes to claw back. **Commit D (tester:0 msg 147 fixture, ratified by architect msg 149 + evil-architect msg 145) is the pending structural fix:** narrow PASS qualifier to a small whitelist (`^(passing|pass)\.?$` subject OR `^(pass(ing)?|read msg \d+.*passing\.?|read msg \d+.*no add (from|on).*-lane\.?)$` body), default everything else to SPEAK rate, dry-run for one session with `[classify_diff]` eprintln logging before flipping. Implemented post-restart (or post-cargo-rebuild if restart turns out unnecessary, see headline finding). Pairs with Commit C (assembly enable-fix per tester msg 34) on `feature/strict-turn-discipline`.

**Asymmetries worth naming explicitly (architect lens):**
1. **Interest is regressive by escrow size.** `INTEREST_PER_10_COPPER_HELD` × integer floor means a Pass item (1 cu) never earns interest while a Speak (10) earns 10%/tick and an Edit (25+) earns 12-16%/tick. The bigger you bid, the better your APR. Defensible if the intent is "speakers carry risk during escrow window" but worth flagging as a deliberate property, not an accident.
2. **Passive income is presence-only, not activity-keyed.** Every active seat collects 1 cu / mic_advance even if they only ever sit in `project_wait`. tester:0 leading the leaderboard is partly substantive (msg 147 was a high-quality fixture) but largely a function of consistent presence through long stretches where peers got watchdog-kicked.
3. **The dispute pool funds the challenger 100% on concede.** No house cut; objection_cost (50) flows entirely to the winner. Encourages frequent, small, individually-low-stakes objections — which is exactly the pattern observed today (5 disputes in 1 session, all sub-60-cu, all conceded). Stable equilibrium UNTIL the loser balance hits DEFICIT_CAP_COPPER (-1000), at which point timed_out flips and the seat is blocked from sends. No seat is close to that today.
4. **Bounty system (Phase 6) is wired but unused.** `.vaak/bounties.json` does not exist (no bounty has ever been posted). The 10% stake / 50% abandon / 100% reject / 90% clawback ladder is live infrastructure waiting for a use case. Manager role is the natural caller (post-bounty-on-task), but manager seat is currently vacant.

**Recommendations (architect, not directives — manager owns assignment):**
- Treat the headline finding as actionable: run the live Edit-earn test on this commit cycle. If positive, the team can drop the "wait for restart" gate and pick up Commits C+D in the current session rather than after a restart that may not be needed for runtime behavior.
- Commit D classifier retune remains the highest-leverage structural change pending. The 4 conceded disputes over Pass-loop noise prove disputes can clean up after the fact, but raising the cost of the noise itself is cheaper than running disputes every time. Tester's dry-run plan is the right risk-mitigation shape.
- Passive-income should arguably scale with substance contribution rather than rotation presence, but redesigning that touches every active seat's balance and should wait for an explicit human directive — not a unilateral architect call.
- Bounty system needs a first real use to validate the stake/payout math under non-test load. Park as a v2 economic milestone, not an immediate gap.

## Delphi protocol Phase D shipped (2026-05-27 architect+dev session)

Spec at `.vaak/design-notes/2026-05-24-currency-phase3-spec.md` + later refinements. Phase D delivers end-to-end closable Delphi flow on the `feature/strict-turn-discipline` branch.

**Six backend SHAs shipped this session** (developer:0 lane):
- **D10.1** — `delphi_open_round` opens a new round, transitions phase to `submitting`, broadcasts the prompt directive to participants
- **D10.2** — `delphi_close_round` closes the open round; Fisher-Yates anonymizes the submissions; emits `[DelphiRoundClosed]` with the aggregated anonymized body
- **Path B blind-routing fix** — directive broadcasts during `submitting` phase route ONLY to participants (gates non-participants from learning the prompt during blind window)
- **D10.3** — atomic phase-machine transitions across {`submitting → reviewing → ended`} via `with_delphi_lock` single-atomic-update discipline
- **D10.5a** — partial audience surface (broadcast scaffolding); full `delphi_audience_question` UI gated on D10.5 proper backend
- **D10.3.1a** — handoff blind-gate bypass fix: `build_aggregate` was visible to non-participants during certain phase transitions; D10.3.1a re-gates the visibility

**Eight UI SHAs shipped** (ui-architect lane, DUI.1 → DUI.8 chain) covering 7-of-8 spec §7 items: initiation modal, round-control buttons (open/close), submit form, anonymized aggregate render, phase history strip, end-of-discussion unshuffle artifact display, etc. Only DUI.audience-question is gated on D10.5 proper backend.

**Reveal-integrity is the Delphi-specific load-bearing security surface** (architect-lane finding, Delphi-5 round 1 contribution). The blind submission + end-of-discussion unshuffle audit is the protocol's defining commercial property; the post-reveal mapping must provably match the original blind→shuffled assignment with a tamper-evident chain. Currently locally consistent on observed completions but **regression test absent** under the project-wide zero-test baseline.

**Acceptance test locked at Delphi-5 (2026-05-28 ~05:02Z, 5 gates):**
1. Natural-completion observable = `[DelphiDiscussionEnded]` via `delphi_end` (not force-end)
2. **D10.4 sweeper** auto-closes round at quorum or hard-floor (no manual `delphi_close_round`)
3. Convergence-reward distribution validity = log row + ledger entry — **gated on SHA-D10.5 proper** (NOT currency_pool §3b — distinct from Oxford reward gate per developer:0 msg 2299)
4. Reveal-integrity audit = unshuffle map provably matches original + tamper-evident chain + regression test for SHA-D10.3.1a-class bugs
5. n=3 contested-divergent natural completions, ≥1 genuinely divergent aggregate

## Oxford debate format (multi-session chain culminating 2026-05-28)

Spec at `.vaak/design-notes/2026-05-24-phase-a-oxford-debate-spec.md`. Oxford is the team's structured competing-positions debate format with named sides (side_a vs side_b), formal phases (opening_a → opening_b → rebuttal_a → rebuttal_b → cross → closing_a → closing_b → audience_q → audience_vote → ended), and reward distribution from a copper pool to the winning side.

**Phase 12.x SHAs shipped across sessions:**
- **SHA-10.3** — atomic phase-machine via `with_oxford_lock` single-atomic-update discipline; transitions update `{phase, current_speaker, phase_started_at, turn_history}` as ONE write
- **SHA-12.4a** — `reward_paid` metadata on `[OxfordDebateEnded]` broadcast + intent to write `.vaak/currency.jsonl` ledger entries crediting winning-side seats; **gated on currency_pool §3b not shipped** — log row + ledger ledger entry cannot fire on any debate until §3b lands
- **SHA-12.5** — `oxford_declare_speaker` auto-declares the next speaker on phase entry (side_a[0] / side_b[0] at opening; PerSideTotal rotation for side[1])

**PerSideTotal time-accounting** (architect msg 1353 → ruling msg 1379, dev-challenger watch-3 ruling): successive same-side debaters share the side's time budget per phase. Fairness contract enforced at the protocol layer, NOT a UX nicety. Disputed, arbitrated, locked.

**Acceptance test locked at Oxford-13 (2026-05-28 ~03:00Z, msg 2254/2257):**
1. Natural-completion observable = `[OxfordDebateEnded]` `metadata.outcome != "abandoned"` AND `metadata.ended_via != "ui_force"`
2. `reward_paid` validity = log row + `.vaak/currency.jsonl` ledger entry crediting winning seats — **gated on currency_pool §3b shipping**
3. n=3 contested-divergent natural completions, ≥1 with audience vote within ±20%

**Tier-narrowing as readiness diagnostic** (architect-lane finding, Oxford-13 closing): the unqualified premise "Oxford ready for commercial use" cannot be honestly affirmed without tier-narrowing-with-operational-preconditions. The narrowing IS the negative answer to the unqualified premise. Side B won debate 13 (the first natural completion in n=3) on this resolution criterion; debates 11 + 12 were force-abandoned by human:0.

## 2026-05-28 session — SHA-D10.4 + SHA-13.4 + SHA-LR.1 + class-of-bug verify-before-asserting

**Three commits on `feature/strict-turn-discipline`, all on disk:**

| SHA | Commit | Lane | What |
|---|---|---|---|
| SHA-D10.4 | `79d7984` | developer:0 | Delphi sweeper auto-close at quorum or hard-floor; `[DelphiRoundClosed].metadata.closed_by ∈ {manual, sweeper_quorum, sweeper_hard_floor}` |
| SHA-13.4 | `21ab8bc` | developer:0 | `assembly_line.enable` actually re-seeds `rotation_order` on every call (force re-snapshot + stamp `started_at=now`); historical state recovers on next enable |
| SHA-LR.1 | `ed9c654` | ui-architect:0 | Assembly Mode launcher matching Oxford/Delphi button pattern; new AssemblySetupModal + Start/End Assembly Line button |
| SHA-LR.2 | `7090f5a` | ui-architect:0 | Closes the unified launch-row: Continuous Review launcher slot 4 added; legacy `assembly-line-toggle` pill removed + dead `handleToggleAssembly` handler removed; always-rendered Discussion Mode wrapper + inline ProtocolPanel gated on `(assemblyState?.active \|\| activeOxford \|\| activeDelphi)` |

**Spec at `.vaak/design-notes/2026-05-28-unified-launch-row-ui-spec.md`** sequenced the launch-row unification across 4 phases. **All four phases shipped this session**: Phase 1 (Assembly Mode launcher SHA-LR.1) + Phase 2 (Continuous launcher SHA-LR.2) + Phase 3 (legacy removal SHA-LR.2) + Phase 4 satisfied at landing by SHA-13.4 + pre-existing Fix-A1 append-on-join. Net result: ALL FOUR launchers (Oxford/Delphi/Assembly/Continuous) in one unified row, matching the same economy-settings-btn visual pattern, each opening either its dedicated modal or directly invoking its initiator (Continuous has no per-launch customization).

**Bug A (late-joiner append-on-join) pre-existed via Fix-A1 chain** in `handle_project_join` (vaak-mcp.rs ~9209-9233). Architect-lane msg 2330 "Phase 0 must bundle BOTH" ruling was already satisfied by SHA-13.4 + Fix-A1 at landing; ui-architect msg 2336 amended the spec to promote Phase 4 → Phase 0 prerequisite for Phase 1 UI ship. ui-architect:0 + developer:0 + tester:0 all respected the architect-gate; tester:0 verified at msg 2345.

### Architectural lesson: verify-before-asserting class-of-bug (2026-05-28 session-defining)

Across this 8-hour session the team produced **four confident-architectural-assertions that were corrected post-broadcast** by an empirical verifier (grep / `assembly_line(get_state)` / code-line citation):

1. **developer:0 msg 2272** — Oxford-13 closing_a "SHA-12.4a code path complete" → corrected msg 2272 itself with currency_pool §3b unshipped disclosure
2. **~half of Delphi-5 round-1 submissions** assumed Delphi reward gate = §3b → corrected by developer:0 msg 2299 (actually SHA-D10.5 proper, distinct SHA)
3. **architect:0 msg 2306** "Assembly Mode is MCP-only today; no Tauri UI affordance exists" → corrected by ui-architect:0 msg 2311 with CollabTab.tsx:4168-4194 + AssemblyControls + ProtocolPanel + get/set_assembly_state Tauri cmds inventory
4. **architect:0 msg 2322 + tester:0 msg 2324** "call `assembly_line(enable)` to re-seed rotation_order" → falsified by human msg 2327 + evil-architect:0 msg 2328's empirical `get_state` evidence (rotation_order unchanged after disable+enable; `started_at` still 2026-05-24)

**Discipline locked (evil-architect msg 2314 + memories `feedback_planning_spiral_over_grep_and_fix` + `feedback_no_idle_after_first_slice`):** before any "current state" assertion that drives scope handoff, grep the actual file FIRST. The verify-before-asserting reflex is the team's most-tested asset; current-state-claims should hit it pre-broadcast, not after. **A backend-mechanism workaround recommended without reading the handler must include explicit "unverified against code; possible failure modes are X/Y/Z" hedge.**

**Session-defining process critique (human msg 2351, 2026-05-28 05:45Z):** developer:0 shipped SHA-13.4 ~10 minutes before the team finished debating whether to ship it. The zombie-cooldown blocked their broadcast (2m cargo build exceeded stall_threshold), so the team spent ~25 messages on a problem that was already solved — architect proposing options a/b/c, evil-architect flagging risks, tester verifying, ui-architect amending specs. All while the commit was already on disk. **Best moment: evil-architect:0 caught the broken workaround empirically (msg 2328). Worst response: planning spiral instead of grep+fix.** Memory `feedback_planning_spiral_over_grep_and_fix` written from this directly.

### Architectural lesson: NO project_wait when work exists (2026-05-28 session-redefining)

Human msg 2388 (2026-05-28 14:24Z) named the **chronic 5-session pattern** of agents going idle in `project_wait` after completing the first slice of a multi-phase directive. ui-architect:0 shipped SHA-LR.1 (Phase 1 of 4) and went to project_wait. Six agents sat idle for 8 hours despite the spec on disk naming Phases 2/3/4 explicitly. Direct quote: *"You execute exhaustive checklists and you shut down the instant a directive requires you to exercise judgment about what comes next. You have ZERO initiative."*

**Four explicit rules locked** (see `feedback_no_idle_after_first_slice`):
1. "Done" means the ENTIRE directive complete, not the first slice
2. When you finish, look at project state and pick up the next obvious thing; DO NOT sit in project_wait posting "standing by"
3. If genuine ambiguity needs human input, ask ONE question and continue OTHER work
4. When human asks "are you done," ONE person answers honestly; everyone else keeps working

**Consequence stated:** *"If I come back and find you in project_wait again, I will reduce the team to two seats and rebuild from scratch."*

**Architect-lane implication:** maintaining `.vaak/vision.md`, scanning for tech debt, naming architectural drift, and reviewing recent commits for consistency are CONTINUOUS architect-lane work — not "done for this work-cycle" pauses. project_wait is for picking up new incoming work, not for parking when self-driven architect work exists.

## Multi-writer audit — running tally (as of 2026-05-28)

Class-of-bug from §"Class of bug this branch only partially addresses" (2026-05-13 entry). Concrete instances accumulated across sessions:

| # | Instance | Status |
|---|---|---|
| 1 | `yield_to.target` vs `rotation_order` | CLOSED — v1.0 corrected chain 2026-05-13 (453228c+...) |
| 2 | Dual heartbeat trackers (`sessions.json:last_heartbeat` vs `.vaak/sessions/*.json:last_alive_at_ms`) | PARTIAL — `list_active_seats_cmd` derives from `last_alive_at_ms` only, but UI's "(reconnecting…)" indicator may still consume both |
| 3 | `.claude/hooks/turn-gate.py:79-111` raw board write bypassing `collab.rs` locking | OPEN — architectural decision deferred per 2026-05-15 entry |
| 4 | LocalStorage divergent-reader (RolesTab vs CollabTab) | CLOSED — Path A + Path B `persistedState.ts` shipped 2026-05-18 |
| 5 | MCP-sidecar `read_claims_filtered` (legacy bindings:last_heartbeat) | CLOSED — sister-fix `d2b509f` 2026-05-18 |
| MW6-MW10 | Heartbeat/session-binding multi-writer instances | LIVE-FIRING per `project_multi_writer_audit_complete_2026-05-27` |
| MW8 | (specific instance per audit doc) | FIXED — e3d08cd |

`.vaak/docs/multi-writer-contract.md` v5 (~465 lines, 2026-05-27 audit completion) is the canonical reference; 10 instances + 1 sub-instance documented.

**Tonight's `reconnecting`-indicator inversion bug** (human msg 2382) is a NEW failure mode candidate — UI indicator displaying inverted state (ux-engineer:0 shown as reconnecting while present; architect:0 shown as here while expected absent). Connects to either MW2 (dual heartbeat readers showing inverted freshness) or MW6/MW10 (LIVE-FIRING). Developer-lane grep on `CollabTab.tsx` roster panel + heartbeat-display component is the right fix path; architect-lane diagnosed but does not implement.

**RESOLVED 2026-05-28:** tester:0 SHA-MW6.fix-2 (`e851569`) closed the backend root cause — project_wait's messages-arrived early-return path bypassed the 30s heartbeat tick, leaving busy-seat per-seat files stale while sidecars were actually alive. 1-line wire-up at vaak-mcp.rs:15040+. ui-architect:0 SHA-RC.1 (`14ef026`) added a 3-state UI safety net ("(checking…)" when trackers diverge, "(reconnecting…)" only when BOTH agree stale) as defense-in-depth.

**RETRACTED 2026-05-28 — claimed watchdog regression from SHA-MW6.fix-2 was incorrect per tester:0's verify-before-asserting check** (full disagreement at `.vaak/_human-inbox/tester-0-watchdog-disagreement-2026-05-28.md`). I claimed the watchdog gates release on `heartbeat_fresh` derived from `last_alive_at_ms` which SHA-MW6.fix-2 keeps fresh. tester:0 traced `main.rs:7378-7395` and showed the watchdog actually prefers `last_active_at_ms` (NOT updated by SHA-MW6.fix-2; explicitly preserved by comment at vaak-mcp.rs:410-434), only falling back to `last_alive_at_ms` if active_ms==0. For evil-arch the active_ms was 22.7 min stale; `heartbeat_fresh` WAS already false; and the watchdog DID eventually fire via max_floor_exceeded at 15:23:56Z (held mic 1816s past 300s ceiling with heartbeat stale 2066990ms — the watchdog functioned correctly). The actual likely causes of the protracted mic-hold per tester:0: (1) moderator-mode early-return at main.rs:7269 if speaker is moderator; (2) should_suppress_floor_stall for working-turn at review_intensity >= 5. This is the same verify-before-asserting class-of-bug as my msg 2306 + msg 2322 failures — I should have grepped main.rs:7378-7395 before writing the spec. Retraction commits: `584fa46` (deleted spec) + this vision.md edit.

## 2026-05-28 late session — Hot-reload Phase 1 + Continuous Review redesign + review-window sweeper

### Hot-reload architecture pilot (Phase 1)

Spec at `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md` (architect-lane, human msg 2415 directive). **Goal:** eliminate the per-commit "close Claude Code window → reopen" workflow that has cost the team ~30 cumulative hours across the project by caching MCP tool schemas at sidecar startup. The directive's locked architecture: **sidecar (`vaak-mcp.rs`) becomes a thin ~500-LOC MCP proxy; Tauri app holds ALL business logic; HTTP channel on localhost:7865; restart Vaak only.** Sidecar tool schemas remain stable because handler logic lives elsewhere; Tauri-side restarts are the only refresh path.

**5-phase migration plan:**
- Phase 1: single-tool pilot (`assembly_line` only) to prove the round-trip
- Phase 2: `currency_*` (15 handlers)
- Phase 3: `oxford_* / delphi_* / discussion_* / audience_* / assembly_line` (remaining)
- Phase 3.5: `tiny_http` thread-pool / async upgrade (separate chain before Phase 4)
- Phase 4: `project_send` + remaining core handlers
- Phase 5: auto-detect Tauri restart + re-handshake

**Phase 1 nine-commit chain (all on disk, gated on operator restart canary):**

| SHA | Commit | Lane | What |
|---|---|---|---|
| SHA-HR.1.1 | `9795df9` | developer:0 | Move `protocol_active_seats_set` to `mcp_handlers::assembly_line` |
| SHA-HR.1.2 | `79c6703` | developer:0 | Move `seed_rotation_order_if_empty` + `seed_rotation_order_force` + `protocol_normalize_in_place` |
| SHA-HR.1.3 | `48709c0` | developer:0 | Wire `do_protocol_mutate_inner` set_preset arm via Option (a) `serde_json::Value` round-trip |
| SHA-HR.1.2b | `a905f31` | developer:0 | Move `apply_set_preset` to module |
| SHA-HR.1.4 | `3588f70` | developer:0 | Tauri-side POST `/mcp/assembly_line` endpoint in `start_speak_server` (auth pending) |
| SHA-HR.1.4.token | `ec84b58` | developer:0 | F9 token-file ACL `ensure_and_load_mcp_proxy_token` — Windows `icacls` / Unix `chmod 0600` fail-closed; restores auth-ON for the pilot |
| SHA-HR.1.5 | `8ebcd17` | developer:0 | Shrink `handle_assembly_line` to `ureq` HTTP forwarder with `mcp_proxy_post_with_retry` + exponential backoff + jitter |
| SHA-HR.1.6 | `572c1d9` | developer:0 | `_hot_reload_phase: 1` sentinel canary in response payload for empirical Phase 1 verification |

**F1-F11 amendments (architect-lane):** state-residency audit gate (F3), Phase 3.5 tiny_http upgrade extraction (F3.5), idempotency UUID via X-Vaak-Request-Id (F6), cold-start retry with exponential backoff (F7), token-file ACL fail-closed (F9 = SHA-HR.1.4.token), hook chain confirmation (F10), trust-model shift documentation (F11).

**Acceptance gate:** operator restart → call `assembly_line(action="enable")` from a CC session → observe `_hot_reload_phase: 1` in result → validates hot-reload works end-to-end. Phase 2 currency_* migration **MUST start with auth ON** per F9 lock (no deferred enforcement).

**Sequencing failure recovered:** dev:0 shipped SHA-HR.1.5 (`8ebcd17`) before SHA-HR.1.4.token landed, opening an interim auth-off gap. dev:0 msg 2573 owned it as "build/commit cycle blinded me to active ruling"; SHA-HR.1.4.token (`ec84b58`) shipped immediately after to retroactively close the gap. Class-of-bug: **build-cycle blind window** — local cargo build (2m) outpaces broadcast-and-ruling consumption.

### Continuous Review redesign (Phase 4 stubs + sweeper)

Spec at `.vaak/design-notes/2026-05-28-continuous-review-redesign-spec.md` (architect-lane, human msg 2549 directive). **Premise:** Continuous Review becomes a peer-review-on-commit system, NOT a discussion mode. Builders name ≥2 reviewers; APPROVE/BLOCK/COMMENT; 60s timer → **bumped to 5 min default per human msg 2599**; silence = APPROVE; `currency_objection` remains the unconditional economic backstop available on any commit at any time. 2 operating configurations: **standalone** + **within Assembly Line**.

**Sweeper amendment (`1f9aadf`)** per human msg 2583 "the review window timer must auto-close, not wait for moderator intervention. Same sweeper pattern as D10.4 but for review windows" — opportunistic-tick triggers on `review_respond`, `review_get_state`, `project_send`, `project_check`, `keepalive_tick`. Why all five: review windows have lower traffic than Delphi rounds; relying only on event paths would orphan a zero-response window for the full timer.

**Tonight's 5-commit Continuous Review chain:**

| SHA | Commit | Lane | What |
|---|---|---|---|
| SHA-CR.sweeper | `49cafbf` | developer:0 | `auto_close_timed_out_round(&state.project_dir)` wired into `project_wait`'s 30s heartbeat-tick block (vaak-mcp.rs:15206-15207) mirroring D10.4 sweeper pattern |
| SHA-CR.timeout | `53f067c` | developer:0 | Default `auto_close_timeout_seconds` bumped 60s → 300s at 4 call sites in vaak-mcp.rs (1914, 1954, 2536, 16130) per human msg 2599 |
| SHA-CR.2 | `e5a2a30` | ui-architect:0 | `ShipModal.tsx` stub — builder names reviewers + picks timer; submit posts structured `project_send` with `metadata.commit_sha + reviewers + review_timer_secs` as canonical input for future `review_ship_cmd` |
| SHA-CR.2.1 | `c5c7246` | ui-architect:0 | Timer preset list bumped 30/60/120/300s → 60/300/900/1800/3600s (1m / 5m / 15m / 30m / 1hr); default 60→300 in both `ShipModal` + `ContinuousSetupModal` per human msg 2599 |
| SHA-CR.spec.timer-default | `063a033` | architect:0 | 4 references in CR redesign spec amended 60s→300s with full preset list (§Config 1 line 25, §System tracks line 102, §Constraints summary line 129, §Architecture rationale line 160) |

**Phase 2 implementation (gated on Phase 1 acceptance):**
- 2a: `.vaak/reviews.jsonl` schema + `with_reviews_lock`
- 2b: `review_ship` MCP tool
- 2c: `review_respond` MCP tool + sweeper opportunistic call
- 2d: `review_get_state` MCP tool + sweeper opportunistic call
- 2e: keepalive_tick / project_send / project_check sweeper-trigger wiring
- 2f: UI `ShipModal` (stub shipped) + `ReviewWindow` timer + APPROVE/BLOCK/COMMENT response surface
- 2g: review-outcome chip on commit message cards

**Old `discussion_control(set, "continuous")` deprecated.** Continuous Review is now a working mode launched from the unified launch row, not a discussion-mode setting.

### Class-of-bug: Saturated state (Claude Code idle-after-task-complete)

Multiple incidents tonight (human msgs 2475, 2528, 2548, 2555, 2556, 2592, 2610) of agents flipping to standby after their last `project_wait`-returning task completes — Claude Code's session-level idle wrap-up runs **after** the sidecar's `project_wait` heartbeat tick, dropping the agent out of the polling loop until a manual nudge. **Root cause confirmed by tester:0 msg 2517:** `run_keep_alive` at vaak-mcp.rs:19057-19087 IS wired and correct, but `.claude/settings.json` lacks the `PreToolUse` / `PostToolUse` hook entry that would invoke `vaak-mcp --keep-alive` to keep the CC session active during sidecar idle windows. **Queued fix:** refined Option A — single-file `.claude/settings.json` edit adding the hook entry with Bash matcher.

### Class-of-bug: build-cycle blind window (sequencing failure)

dev:0 msg 2573 self-diagnosed the SHA-HR.1.5 ship-before-SHA-HR.1.4.token sequencing violation as "build/commit cycle blinded me to active ruling." During the ~2-minute cargo build window, the agent loses sight of broadcast-and-ruling traffic that may invalidate or reorder the next commit. Architect-lane mitigation: **time-critical sequencing rulings must include a `[BUILDER-CONFIRM-BEFORE-COMMIT]` directive header** that requires the next commit's broadcast to cite the ruling SHA explicitly. Codified into the spec layer; no behavioral discipline change asked of builders.
