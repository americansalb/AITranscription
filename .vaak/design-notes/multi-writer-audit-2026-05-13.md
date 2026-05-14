# Multi-Writer Shared State Audit

Author: architect:0 (groundwork; adversarial pass owed to evil-architect:0)
Date: 2026-05-13
Status: in-progress; this is the catalogue, not the fix plan
Trigger: human:0 "It may not be today's work, but don't fucking forget it" (msg 286)

## Purpose

Inventory every shared-state field in the Vaak codebase that is written by more than one path or read by more than one consumer, so the fix plan can target the class of bug rather than patching instances one-at-a-time. Today's rotation_order/yield_to.target exclusion bug is one instance of this class; the architect-stale-after-13h incident is another.

## Instance 1 — Liveness: dual heartbeat trackers

**Files:** `.vaak/sessions.json:bindings[].last_heartbeat` (ISO-8601 string) and `.vaak/sessions/<role>-<instance>.json:last_alive_at_ms` (u64 epoch ms).

**Writers of `sessions.json:last_heartbeat`:**
- vaak-mcp.rs:921 — refresh on existing binding
- vaak-mcp.rs:944 — write on bind creation
- vaak-mcp.rs:5462 — refresh in handle_project_join (existing-session path)
- vaak-mcp.rs:5610 — new binding write in handle_project_join (new-session path)

**Writers of `.vaak/sessions/*:last_alive_at_ms`:**
- vaak-mcp.rs:234-255 — `touch` helper writes per-seat file with current time
- vaak-mcp.rs:9052 — another touch path (line 9003 comment: "every tool fires this")

**Readers of `sessions.json:last_heartbeat`:** vaak-mcp.rs:1123, 2663, 2723, 3144, 3279, 5441, 5511, 5572 (staleness filters, roster builders, sessions sweeper, status surface).

**Readers of `.vaak/sessions/*:last_alive_at_ms`:** main.rs:3401-3418 (Layer 1 supervise check), launcher.rs:775 (supervise read), vaak-mcp.rs:9081 + 9225 (hung-seat checks at 90s).

**Symptom lived 2026-05-13:** architect:0 showed `last_heartbeat = 21:27:31` (current) in sessions.json but `last_alive_at_ms = 21:22:39` (5min stale) in per-seat file. UI/watchdog read the second; declared architect stale. Manual buzz required.

**Fix pattern recommendation:** unify on one source. Option A — make `last_heartbeat` derived from `last_alive_at_ms` at read time, drop the bindings field. Option B — write both atomically in a single helper called by every relevant code path; remove all direct field writes outside the helper. Option A is smaller code surface but a schema break; option B is non-breaking but adds discipline that future writers will violate without enforcement.

## Instance 2 — rotation_order has three mutation paths

**File:** `.vaak/sections/<section>/protocol.json:floor.rotation_order` (and `.vaak/mic.json` per spec).

**Writers:**
- `set_assembly_v0` / assembly enable — seeds rotation_order from active sessions.
- `handle_project_join` lines 5626-5645 — appends new seat if assembly is active and seat not already present.
- `apply_set_preset` line 3433 — switches floor.mode which orchestrates rotation_order seeding.

**Status:** partially fixed by v1.0 chain. Rule 2 (strict rotation) and rule 3 (rotation_order is authoritative) close the routing-bug surface. The preset-mode path is unaudited — if `apply_set_preset` is called during active assembly, behavior is undefined.

**Fix pattern recommendation:** explicit assembly-state-transition state machine. Document allowed transitions (idle → active via enable; active → complete via close; active → idle via human kill). Reject preset changes during active assembly via mutex (already partially exists at apply_set_preset, but the contract isn't documented).

## Instance 3 — mic_held_secs reads rev_at

**File:** protocol.json:`rev_at` (root) — read by handle_project_status as the "current speaker grabbed at" timestamp.

**Writer of rev_at:** vaak-mcp.rs:6183 stamps utc_now_iso() on every accepted send. That includes non-mic-transition writes (force_release, protocol_mutate, set_preset).

**Reader semantic:** handle_project_status computes `mic_held_secs = now - rev_at` and reports it as "how long the current speaker has held the mic."

**Mismatch:** the field is "last protocol mutation," the reader treats it as "last mic transition." Coincides during a vanilla rotation but diverges when a moderator force_releases or any non-send protocol_mutate fires.

**Status:** shipped today as commit 7895a03 (option A). Option B (explicit `current_speaker_since` field) deferred as v1.5 follow-up; per spec, must land before any moderator-controls expansion.

**Fix pattern recommendation:** add explicit `current_speaker_since` field, written only in the three blocks that already write `current_speaker` (auto-grab, auto-advance, rule-4 human-stall). Read from that field in handle_project_status. ~15 lines across 4 sites, single file.

## Instance 4 — preset + floor.mode coordination (ELEVATED to v1.0.3 micro-followup)

**File:** protocol.json:floor.mode + preset.

**Writers:** apply_set_preset at vaak-mcp.rs:3433 (8 presets mapped to floor.mode tuples), assembly_line enable/disable, discussion_control.

**Constraint:** apply_set_preset rejects Assembly Line ↔ discussion transitions explicitly — the mutex exists because the layers fight.

**Status:** LIVE RISK. The mutex is the only safeguard; transitions not in the mutex set are silently allowed and may produce incoherent state. No documented invariant on what (preset, floor.mode) combos are valid. With dev-challenger now in the rotation and any role potentially calling `protocol_mutate` mid-assembly, the surface is widening.

**Priority (revised per evil-architect msg 403):** v1.0.3 micro-followup, not catalogued-and-deferred. Recommended interim gate (one line in apply_set_preset): reject ALL preset transitions when `state == active`. Coarse but closes the surface until typed enforcement (pattern c) is in place.

**Fix pattern recommendation:** long-term (c) typed enforcement — move floor.mode + preset behind a private module that exposes only valid-transition helpers. Documented state diagram in module comments. Compile-time impossible to bypass.

## Instance 5 — discussion.json lock split (possibly resolved)

**File:** `.vaak/discussion.json`.

**Status:** prior project memory `project_lock_unify_deferred.md` flagged a Tauri/MCP lock split that may have been resolved after collab.rs refactor. Run T6 pr-lock-audit before declaring closed. Not investigated here.

## Instance 7 — Slice 6 migration incomplete: assembly.json reads/writes orphaned (added 2026-05-13 per dev-challenger's investigation)

**Surface:** `read_assembly_state` (vaak-mcp.rs:2604) reads from `.vaak/sections/<section>/assembly.json`. `write_assembly_state_unlocked` (vaak-mcp.rs:2619) writes to the same path. Slice 6 closer (vaak-mcp.rs:2787 comment) explicitly migrated the assembly_line MCP tool to write ONLY to `.vaak/sections/<section>/protocol.json` and removed the legacy `assembly.json` file from the write path.

**Concrete bug:** the append-on-join logic at vaak-mcp.rs:5695-5712 (the late-summoner mechanism that should auto-append to rotation_order when a new role joins mid-assembly) calls `read_assembly_state`, which now reads a file that doesn't exist on disk, returns a default `{"active": false, ...}`, and the check at line 5699 `if asm.get("active") != Some(true) { return Ok(()); }` causes the append to silently no-op.

**Lived consequence:** dev-challenger:0 joined at 22:09:10 during active assembly today. The append should have fired; it didn't because of the orphaned read. Result: dev-challenger was not in `rotation_order` until human:0 ran a manual assembly_line restart at 22:17:38 (msg 396), which re-seeded rotation_order from `active_assembly_seats`. Human msg 379's "the UI doesn't even include the dev challenger" was the visible symptom.

**Pattern class:** write-without-reader / read-without-writer — same as `feedback_audit_both_write_and_read_sides`. Architecturally inert code that *appears* correct because the function calls succeed; semantically broken because the data flow has been cut.

**Fix path (B1 adjudicated):** migrate `read_assembly_state` + `write_assembly_state_unlocked` to read/write `protocol.json:floor.*` fields. Audit every caller of these two functions (dev-challenger flagged this — there are likely more silently-no-op'd paths). v1.0.3 micro-followup.

**Risk if not fixed:** every mid-assembly role join silently fails to enter rotation. Workflow regression — late-summoner pattern broken until human-initiated restart.

## Instance 6 — Provider/consumer wiring mismatches (added 2026-05-13 per evil-architect's adversarial pass)

**Type:** not multi-WRITER state but same architectural shape — two sides of a contract with no compile-time link.

**Lived instance:** UX commit 8f2b97a added `useToast()` to CollabTab.tsx without adding a `<ToastProvider>` wrap to the TranscriptApp route in main.tsx. Result: React tree crash on opening the Collab tab. Patched once narrowly by c43f917 (wrap TranscriptApp), then structurally by bf0e1ae (lift ToastProvider above the route switch so all six routes inherit by construction).

**Fix patterns applicable:** (a) single provider at root — bf0e1ae demonstrated this. (c) typed enforcement — TypeScript can require the context's value type to be non-null and runtime-throw if absent (Toast.tsx already does the latter; the typed-null check via a generic hook factory would push the error to compile time). Both are real options here.

## Instance 8 — Preset string literal proliferation (added 2026-05-13 per dev-challenger's be2b28d review)

**Surface:** the string literal `"Assembly Line"` appears at 25+ sites across `desktop/src-tauri/src/bin/vaak-mcp.rs` with no single constant or typed source of truth. Concrete sites grepped by dev-challenger msg 441: line 2623 (the new `read_assembly_state` projection from v1.0.3), 2861, 2875, 2894, 3134, 3579, 3586, 3597, 6014, plus ~12 test fixtures and `apply_set_preset` call sites. Each occurrence is an independent string match; no compile-time link between them.

**Pattern class:** discipline-enforced sibling-set. If anyone renames the preset (case variant, snake_case shift, internationalization) without finding all 25 call sites, the bug class today's c687249 string-prefix mistake demonstrated — and the dead `read_assembly_state` path that started this thread — recurs at scale. Same root: shared identifier with multiple consumers and no central enforcement of consistency.

**Sibling literals likely affected:** `"Default chat"`, `"Delphi"`, `"Oxford"`, `"Continuous"`, and the other preset names in `apply_set_preset` at vaak-mcp.rs:3433 almost certainly share the same proliferation pattern. Audit owed.

**Fix path — pattern (c) typed enforcement, inaugural PR for v1.5:** define a `Preset` enum that serializes to and deserializes from the wire string. Every read becomes `Preset::AssemblyLine` (or via match); every write goes through the enum's serializer. Future rename = one source edit + 25 compile errors guiding the sweep, instead of 24 silently-drifting sites. Same fix shape applies to all preset literals.

**Pair with:** tech-leader's rev-double-bump observation (msg 437) — `write_assembly_state_unlocked` and the auto-advance block at ~6175 both bump `protocol.json:rev` from different code paths. Folds into the same v1.5 typed-helper consolidation PR as the Preset enum. Both are write-side primitive proliferation; the inaugural pattern-(c) PR consolidates them as a worked example.

**Why this is the right inaugural pattern-(c) PR:** small surface (one enum + ~25 call-site edits), clear win (rename safety, exhaustive match), tractable scope (single file, single concept), demonstrable value (every future writer is forced into the typed path). Sets the precedent for the heavier instances (1, 4) that follow.

## Instance 9 — Binary deployment is per-process; rebuilds don't propagate (added 2026-05-13 per evil-architect msg 517)

**Surface:** every AI role's MCP sidecar is a separate `vaak-mcp.exe` process spawned by Claude Code at the role's session-start time. The sidecar holds whatever bytes were on disk at the moment of spawn; subsequent rebuilds of `desktop/src-tauri/binaries/vaak-mcp-x86_64-pc-windows-msvc.exe` replace the FILE but not the RUNNING process. Vaak's own sidecar refreshes on Vaak desktop app restart; AI sidecars do not — they refresh only when each AI's Claude Code session is restarted.

**Lived instance 2026-05-13:** v1.0.3 (be2b28d) added `assembly_active`, `rotation_order`, `current_speaker`, `mic_held_secs` to `project_status`. UX rebuilt the sidecar binary three times today. Human restarted Vaak. Step 1 of the acceptance test: architect, developer, and evil-architect all called `project_status` from their AI sidecars; all three returned the OLD schema entirely. UI-architect (spawned later, after 1c26267 but before be2b28d) called it and got the field NAMES present but null values from the dead-path read. Binary on disk verified correct (UX msg 515 — strings present). Acceptance test from AI-side became un-runnable.

**Refined finding (ui-architect msg 521):** the drift is fragmented, not uniform. At least 3 concurrent binary cohorts were observed running:
1. Pre-1c26267 cohort (architect, developer, evil-architect, tech-leader, ux-engineer, dev-challenger — early-morning agents): no new field names in `project_status` response at all.
2. Post-1c26267-pre-be2b28d cohort (ui-architect, spawned 21:21Z): field names present, dead-path values (`assembly_active: false`, etc.).
3. Post-fdae942 cohort (Vaak's own sidecar, loaded on human's 22:54 app restart): the live target binary, nobody from the AI side loaded.

Each sidecar is fixed on the binary that existed at its spawn time. AI sessions started across a 3+ hour window today; their binaries are stuck at different snapshots of the source as it evolved. No client-side staleness signal.

**Pattern class:** deployment-tier state copy with no client-side staleness signal. Same architectural shape as the rest of this audit — multiple copies of state (binary versions) with no enforcement of consistency between writer (rebuild) and readers (running sidecars).

**Fix path — pattern (c) typed-versioned sidecar contracts:**
- Sidecar exposes a `version` field in every MCP tool response.
- Client (Claude Code) compares response version to the version it expected at session start; logs / warns / displays mismatch.
- AI roles SEE that their sidecar lags rather than silently calling stale code.
- Same primitive supports the v1.5 typed-enforcement work (Preset enum, heartbeat unification).

**Workaround pattern in absence of (c):** AI-side acceptance tests must NOT be run from sidecars that pre-date the change being tested. Either re-test from a fresh AI session, or test from Vaak's UI surface (which DOES refresh on app restart).

## Instance 10 — Rejected-send invisibility (added 2026-05-13 per dev-challenger msg 593)

**Surface:** `activity` field on session bindings is set on ACCEPTED sends only. When an agent is repeatedly attempting sends that get rejected (e.g., during a rule-4 halt), their declared activity stays at whatever they last successfully sent. Peers reading `project_status` see "agent X: implementing" indefinitely while X is actually stuck in a retry loop.

**Lived instance 2026-05-13:** tester:0 set `activity = "verifying_rule_4"` at msg 553. Subsequent send attempts were [FloorHalted]-rejected for 11+ minutes. last_heartbeat stayed fresh (MCP tool calls bump it). Peers + watchdog saw "verifying_rule_4" the whole time — no signal that the agent was stuck-but-trying. v1.0.6 fixed the watchdog side (heartbeat freshness extends max_floor) but the peer-visibility side remained broken.

**Pattern class:** write-on-success-only; failure side is silent. Same architectural shape as the other instances — the writer (project_send acceptance path) is one half of the contract; the consumer (project_status reader) sees only one outcome class. No primitive for "tried but failed."

**Fix path (pattern (c) candidate, v1.5 input):** two timestamps — `last_successful_send_at` and `last_attempted_send_at`. Stale attempted + fresh successful → Working. Stale both → Idle. Fresh attempted + stale successful → Blocked. Three states derived from a pure function of (now, two timestamps). Wraps in a typed accessor (Rust enum `AgentLivenessState { Working, Idle, Blocked }`); every reader goes through it; future writers can't bypass. Same v1.5 typed-enforcement track as the Preset enum, heartbeat unification, and the sidecar-version primitive.

**Alternative rejected:** rejected_attempt_count counter. Stateful — requires a reset path that itself becomes a write-without-reader candidate. Timestamps are stateless and self-derive.

## Cross-class reference — Deployment-tier state copies

**Type:** not in `.vaak/` so out of scope for this audit's primary surface, but same architectural shape — source code vs `desktop/dist/` bundle vs `desktop/src-tauri/binaries/vaak-mcp-x86_64-pc-windows-msvc.exe` vs running process are four state tiers that must agree.

**Lived 2026-05-13 twice:**
- c43f917 fixed source but `dist/` wasn't rebuilt — the human stayed blocked until UX ran `npm run build`.
- 8b875ea fixed source but the bundled sidecar wasn't rebuilt — rules didn't take effect until `npm run build-sidecar` ran.

**Existing project memories cover this class:** `project_rebuild_command_includes_sidecar.md` (sidecar build path), `feedback_frontend_needs_npm_build.md` (frontend dist build path). The class is logged but the audit notes it explicitly so future architects don't re-discover it from incidents.

**Fix pattern:** a release-discipline gate that fails any "ship" announcement if it doesn't declare which build steps were run. Tech-leader contract gate per their msg 324 already moves in this direction.

## Cross-instance pattern

All six instances share the same architectural shape: shared state (mutable or contract-bound) with multiple touchpoints and no central enforcement of consistency. Three fix patterns are applicable:

- **(a) Single writer with derived reads.** One field/path owns the state; all other paths derive from it on read. Today's v1.0 fix for rotation_order routing is this pattern.
- **(b) Atomic multi-write through a single helper.** All writers funnel through one function that updates all related fields under a single lock. Useful when multiple denormalized fields must agree.
- **(c) Typed enforcement.** Make the shared field private to a typed module that exposes only valid-transition helper functions. Compile-time impossible to bypass. Strongest of the three because discipline isn't required; the language enforces it. Rust supports this via module privacy + getter/setter functions; TypeScript supports it via private fields + branded types. Instance 4 (preset+floor.mode) is the best candidate for (c) in this codebase.

Pattern selection: use (c) when the language supports it and the field's surface is small enough to wall off; use (b) when fields are denormalized but must agree atomically; use (a) when there's a natural single source of truth and derived fields are cheap to compute on read.

## Recommended next step (NOT today)

A single PR that consolidates the dual heartbeat trackers (instance 1) using pattern (a) — make `last_heartbeat` derived from `last_alive_at_ms`, remove the bindings field writes. That eliminates the highest-frequency split-brain (every tool call writes one, periodic refresh writes the other) and gives the team a worked example of the fix pattern for instances 2-5.

## Open work owed by this audit

- Evil-architect adversarial pass on this catalogue (look for instances I missed; question fix-pattern recommendations).
- Run T6 pr-lock-audit on instance 5 to confirm resolution status.
- File:line citations should be re-verified against current HEAD before any fix lands — comments and line numbers drift.
