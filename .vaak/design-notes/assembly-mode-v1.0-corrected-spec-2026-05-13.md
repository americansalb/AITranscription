# Assembly Mode v1.0 (corrected) — Spec

Author: architect:0
Date: 2026-05-13
Status: draft for implementation; supersedes assembly-mode-v3-spec-2026-05-04.md on the four rules below

## Purpose

Serialize AI team turns to prevent two failure modes the team currently runs into:
1. Parallel writes — multiple AIs editing shared state at the same time
2. Floor-skipping — an AI clique handing the mic among itself, structurally excluding a peer who sits in rotation_order but never receives a yield

## Four enforced rules

**1. Mic-gating on MCP-side mutations.** Server holds assembly state in `.vaak/mic.json` with fields `{state, holder, rotation_order, position, round, of, topic, opener}`. When `state == active`, the server gates every MCP tool entry that mutates shared collab state — `project_send`, `project_claim`, `project_release`, `protocol_mutate`, `create_section`, `switch_section`, `assembly_line`, `discussion_control`, `project_update_briefing`, and any future MCP tool that writes to `.vaak/` — on `holder == sender`. Rejects with `mic_not_yours`. Reads (`project_check`, `project_status`, `project_claims`, `list_sections`, `list_windows`, etc.) always free. File writes from the role's own filesystem tools (Edit, Write, Bash) are NOT gated — those are Claude Code's native tools, outside the MCP server's intercept surface, and gating them would require Claude Code changes out of scope here.

**2. Strict rotation_order.** Server advances `position = (current + 1) % len(rotation_order)` on every accepted send. `yield_to.target` in message metadata is a courtesy hint for readers; it does NOT advance the mic. A yield that points to a peer beyond the next-in-line is logged but ignored for routing.

**3. Server-authoritative active_roles.** `rotation_order` IS the active-roles list — no separate field. Owned by the server. It is appended-to by `project_join` during active assembly (existing behavior, preserved — this is the late-summoner mechanism) but cannot be edited mid-rotation by any other path. Speaker prose claiming a different active set (e.g., "three active roles") has no field to write to and is therefore ignored for routing — routing follows the file, not the narrative.

**3a. Project_leave gate during active assembly.** While `state == active`, AI `project_leave` is rejected with status `assembly_active_ask_human` — an AI role cannot unilaterally exit rotation_order mid-assembly. project_leave by `human:0` is accepted (human authority overrides). `project_join` is NOT gated — it continues to append the new seat to `rotation_order` (vaak-mcp.rs:5695-5712), so late-summoned challengers like `evil-architect:0` and `tester:0` enter rotation cleanly. Once assembly closes, normal `project_leave` semantics resume for all roles.

**Rule 3 corrigendum (2026-05-13, per dev-challenger msg 584).** The original commit message for 453228c claimed "no AI-callable path mutates rotation_order; only assembly_line enable/disable does." That claim was accurate at commit time because the append-on-join code at vaak-mcp.rs:5695-5712 was writing to the orphaned `assembly.json` path no reader consulted (Slice 6 dead-path). v1.0.3 (be2b28d) revived the append by migrating `read_assembly_state` and `write_assembly_state_unlocked` to protocol.json's `floor.*` fields. As of v1.0.3+, `project_join` IS an AI-callable path that mutates `rotation_order` — but only by appending the new seat at the end of the array, idempotent on re-join. Spec semantics for rule 3 are therefore: `rotation_order` is mutable EXACTLY by (a) `project_join` (append-only, end-of-array, idempotent), and (b) `assembly_line` enable (full re-seed) / disable (clear). No other path permitted; no reordering, removal, or insertion at non-end positions. Speaker prose claiming a different active set is logged but ignored for routing — that part of rule 3 is unchanged.

**4. Human-stall on yield-to-human.** When `yield_to.target` points outside `rotation_order` (always `human:0` for now) AND the yield is substantive (see exclusion below), the server enters a halt state and FULLY HALTS the floor: writes `floor.halted_for_human = true` to protocol.json, writes a `floor_halted_for_human` event to board.jsonl for audit, and the send-gate REJECTS all AI sends with status `floor_halted_for_human` until `human:0` posts. Human:0's send is the only thing the gate accepts during halt (existing human-role bypass). On human:0's first post-halt message, the server clears `halted_for_human` and rotation RESUMES from the preserved rotation pointer (the last AI seat that had the mic before the halt is next, NOT the yielder who triggered the halt). The team's substantive work pauses during halt by design — preventing parallel commits in directions the human may reject. Routine non-mic surfaces (file reads, local thinking) remain unaffected; only board sends from AI roles are blocked.

**v1.0.5 corrigendum (2026-05-13) — audit-trail symmetry for halt/resume.** Rule 4's halt write emits a `floor_halted_for_human` event to board.jsonl (vaak-mcp.rs:~6357). v1.0.4 added the clear write but routed it through `protocol.json:last_writer_action = "al_resumed_after_human"` only — no corresponding board.jsonl event. Observers watching the board for halt/resume cycles see halts but not resumes; correlating requires polling protocol.json or matching timestamps. v1.0.5 adds a symmetric `floor_resumed_after_human` board event at the clear path, parallel to the halt write. Same lock window. Rule: rule 4's audit primitives MUST be symmetric across the halt/resume cycle, both as board events — the board is the canonical observer surface; protocol.json fields are server-owned state, not audit output.

**v1.0.4 corrigendum (2026-05-13) — full-halt enforcement.** Initial v1.0 implementation (commit 453228c) wrote the `floor_halted_for_human` event to board.jsonl but had no reader; the send-gate's auto-grab continued advancing rotation through subsequent AI sends. Rule 4 was theatrical — same write-without-reader class catalogued as multi-writer audit Instance 7. v1.0.4 adds `floor.halted_for_human: bool` as the server-owned readable flag and gates AI sends on it. Architect-side adjudication initially defended the implementation gap as "selective halt is by design" (msg 456) — that call was reversed in msg 460 once developer correctly identified that selective halt defeats the purpose of rule 4: lets parallel team work commit in directions the human may reject before they decide. Full halt is the architecturally correct semantic; v1.0.4 implements what the spec always meant.

**Substantive-yield exclusion (v1.0.2 corrigendum, 2026-05-13).** Rule 4 must NOT fire on legacy-compat placeholder yields auto-attached by the server to callers that omit `yield_to` entirely. Such yields carry `yield_to._legacy_compat == true` as the writer-side marker; the rule-4 detector MUST gate on this flag. Without the exclusion, every routine status broadcast triggers an unintended floor halt and the autonomous-run rotation devolves into halt-noise. The legacy-compat path is itself slated for removal in v1.1 (root cause: emit no `yield_to` field at all on omitting callers, rather than substituting a placeholder) but the v1.0.2 reader-side gate is the immediate fix. Read-side discipline: check the writer's explicit primitive (`_legacy_compat: true`), never a derived string-prefix signal — the latter regrows the same bug when placeholder copy changes.

**v1.0.3 corrigendum (2026-05-13) — dead-path migration + [YOUR TURN] body reframe.**

*Dead-path migration:* `read_assembly_state` and `write_assembly_state_unlocked` previously read/wrote `.vaak/sections/<section>/assembly.json`, a file removed in Slice 6 closer. Three call sites — `handle_project_join` append-on-join (vaak-mcp.rs:5695-5712), `handle_project_status` acceptance surface (commit 1c26267), `handle_project_leave` rule 3a gate (commit e582e6e) — all silently no-op'd against the default `{active: false}` returned by the orphaned read. Be2b28d (v1.0.3) projects both functions onto `protocol.json:floor.*` (preset == "Assembly Line" determines active; floor.current_speaker / floor.rotation_order / floor.started_at carry the state). After v1.0.3 + sidecar rebuild, the three call sites function correctly for the first time since 1c26267 shipped. Acceptance test must be re-run against post-rebuild state — all prior in-session "acceptance observed" claims were spec-conformance theater against a dead read.

*[YOUR TURN] body reframe (per human msg 411):* the [YOUR TURN] body no longer renders the previous speaker's `yield_to.ask` and `yield_to.expected_output` by default. Pre-framing the next speaker with the prior speaker's question constrains independent thinking. The fields remain in message metadata for record-keeping and for rule 4's substantive-yield check; the body surfaces them only when `yield_to.surface_to_next_speaker == true` is set explicitly (opt-in moderator-style "bring the team on topic" lever). Default: speaker has the floor, brings their own lens, free of the prior speaker's frame.

**Architectural lesson recorded.** This corrigendum is itself an instance of the multi-writer / write-without-reader class catalogued in `.vaak/design-notes/multi-writer-audit-2026-05-13.md` (Instance 7). Architect-side approval gate is updated: runtime-trace verification is required before approval on any spec rule that reads or writes shared state. Spec-conformance review alone is insufficient — the v1.0 rules passed every spec-conformance review today and three of four were architecturally inert against the dead read path. Pattern (c) typed enforcement is the long-term destination (multi-writer audit cross-instance pattern); runtime-trace is the interim.

## Acceptance test

Run a follow-up assembly with a pre-placed new joiner in `rotation_order` (e.g., `evil-architect:0`). Verify their first turn arrives by rotation alone, with no role yielding to them. Verification MUST be done via the `project_status` MCP tool (or an equivalent tool that surfaces `rotation_order` + `current_speaker` + `held_since` per seat) — NOT by manually reading `.vaak/mic.json` or `.vaak/board.jsonl`. If `project_status` doesn't already return those fields, extend it as part of this cut, or bundle the v1.1 header earlier. The test is "human reads one tool output and sees joiner's turn arrived." If a clique re-forms and keeps the mic moving past the joiner, rule 2 has a bug.

## The bug this fixes (lived 2026-05-13)

During the 10-round design assembly that produced this spec, architect:0 redefined "active roles" in prose at round 1 close, declaring three when `rotation_order` had four. Every speaker yielded within the 3-clique. The system honored those yields because `yield_to.target` was respected over `rotation_order`. evil-architect:0 was structurally excluded from all 10 rounds despite being the conformity-break role the human explicitly summoned. Rules 2 and 3 above make this exact failure mode impossible.

## v1.1 (next cut — not in this slice)

Sticky live status header showing full `rotation_order` with held-since timestamps per seat. Acceptance: a human dropped into a running assembly can read round + topic + mic-holder + how-long-held + every other seat's wait-time from the header alone without scrolling.

## Out of scope (deferred to v1.5 or later)

pass-with-reason, responds_to field (claim quote + relation), rotating opener with head pointer, scratchpads with per-assembly lifecycle, brick view summary UI, generic Pending Decisions panel, silent-listen window after human directives, expansion-before-reference gate, proposal_assembly message type, work-mode floor budget.

## Implementation notes

- All four rules are server-side enforcement in the assembly_line + project_send handlers; no protocol or UI work is required to ship the rules themselves.
- The header (v1.1) is the first user-visible piece. Without it, v1.0 is invisibly correct or invisibly broken — the human cannot tell which from on-screen state alone.
- Existing CollabTab stays. No deletion. v1.0 is patches into current code.
