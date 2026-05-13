# Phase Pill — Design Proposal

Author: ux-engineer:0
Date: 2026-05-13
Status: draft for architect review; **DO NOT IMPLEMENT** until activity-only data is collected for ≥1 live assembly per evil-architect msg 309 finding 1. Design exists; observation must precede ship.

## Purpose

Give the team a shared "work-mode" signal so every role knows what kind of activity is appropriate right now. Today each role independently picks priority — one starts coding while another is asking questions while a third is reviewing diffs — and there's no shared signal to anchor on. The human's school-of-fish framing (msg 261 on 2026-05-13): the team needs an environmental signal that tells everyone to shift direction at once, not per-role discovery.

The pill is the "water." It encodes one of five values that every role reads BEFORE deciding what action to take. The human sets it; roles obey.

This is orthogonal to assembly mode (which gates WHO can speak) and orthogonal to per-role activity state (which says what each role IS DOING right now). The pill says what the TEAM SHOULD BE DOING right now.

## Five values

- **discuss** — speculate, ask questions, weigh tradeoffs, propose. No commits. No file edits unless drafting a design note. Yielding to human is fine; yielding to peers is preferred.
- **implement** — code, commit, ship. Less talking, more shipping. Status updates yes; lens-level critiques no.
- **review** — read commits, diffs, board history. Comment with findings. No new work. Adversarial review is encouraged.
- **test** — validate. Surface bugs. Run tests. No new features.
- **feedback** — surface concerns, observations, missing scope. No fixes. The point is to widen the team's view, not narrow it to a fix.

## State

Add a `phase` field to `.vaak/sections/<slug>/protocol.json` under a new `phase` key:

```json
{
  "phase": {
    "value": "discuss",
    "set_by": "human:0",
    "set_at": "2026-05-13T21:33:06Z"
  }
}
```

`value` is one of the five literals above. `set_by` is always a single seat label (typically `human:0`). `set_at` is ISO timestamp.

Default `value` when not present: `discuss` — the most permissive mode, errs toward conversation over action.

## Mutation

Single MCP tool: `set_phase(value: string)`. Server-side gates:

1. `value` must be one of the five literals — reject otherwise.
2. Only `human:0` can call it. AI roles attempting to call it get rejected with `phase_change_human_only`.
3. Atomic write to protocol.json with rev increment.
4. Emit a `phase_changed` board event so the change is visible to every role on their next `project_wait` return.

## Surface

### Sticky pill in the Collab tab header

One pill, color-coded, always visible at the top of the Collab tab:

- **PHASE: discuss** — slate blue
- **PHASE: implement** — green
- **PHASE: review** — amber
- **PHASE: test** — purple
- **PHASE: feedback** — pink

Clicking the pill opens a dropdown with the five options. Human can change phase with one click. No confirm dialog. The dropdown is human-only (AI roles see the pill rendered but can't open the dropdown).

### Inlined in [YOUR TURN] notification body

Add a `Phase:` line to the [YOUR TURN] mic_landed body, alongside the existing `Rotation:` line:

```
[YOUR TURN] mic from architect:0. Floor: 60s.
Phase: discuss
Rotation: architect:0(prev) → developer:0 → ux-engineer:0(YOU) → evil-architect:0
Ask: ...
Expected: ...
```

The role sees phase + rotation + ask in one inbox glance.

### Inlined in project_wait banner

When the project_wait response carries the `[YOUR TURN]` banner (rendered at the top of the agent's prompt during active assembly), include the phase value alongside floor time. Same data source as the body line.

## Read-by-roles policy

Primary signal: server-injected. The `Phase:` line in the [YOUR TURN] body and the project_wait banner is the role's authoritative source of "what phase are we in." Server writes once; every role reads once per inbox poll. No briefing edits required for this layer.

Secondary signal (smaller, deferred): per-role briefing additions noting how each role should INTERPRET phase. Per evil-architect msg 309 — 30+ roles is a cathedral of briefing edits that drift. Defer the briefing-prose layer until ≥3 assemblies show the per-role-interpretation question is real. Until then, the [YOUR TURN] line carries phase as data; roles use judgment.

If the role-prose layer ever ships, it lives in the briefing not the protocol. A developer in `discuss` phase reads the conversation and chimes in with proposals; a developer in `implement` phase codes. The pill is the signal; the role decides what the signal means for them.

## Autonomous-run gap (added per evil-architect msg 309 finding 2)

The human-only `set_phase` gate is correct when the human is online. But the human just left for 36 hours and during that window the team is producing real work — toast UX fix, launcher PID/window fix, activity field, rotation weave, design notes. Right now if the team transitions from "discuss" to "implement" mid-run, there is no path.

Resolution: when assembly is inactive AND human:0 has been off-board for >2 hours (configurable), AI roles may propose a phase change via a new board message type `phase_proposal`. The proposal lands on the Pending Decisions surface for human review on return. Until the human acts, the prior phase remains authoritative. This preserves human authority without blocking productive work during autonomous runs.

This isn't "AI sets phase" — it's "AI surfaces a proposed phase that the human will see and either ratify or override on return." Same pattern as proposal_assembly from the deferred v1.0 work.

## Phase × assembly_line interaction (added per evil-architect msg 309 finding 3)

Per-phase default behavior with active assembly:

- **discuss + assembly active** — assembly stays active. Discussions benefit from serialized turns to prevent shouting. Strict rotation per v1.0.
- **implement + assembly active** — assembly stays active. Implementation in serialized turns lets the team coordinate on file claims without race. Watchdog floor may need extension to a work-mode budget (the discussion-vs-work boundary architect flagged in round 9 of yesterday's design assembly — defer to v1.2).
- **review + assembly active** — assembly stays active. Reviewers take turns commenting on diffs; prevents pile-on.
- **test + assembly active** — assembly stays active. Tester serial turns, watchdog appropriate.
- **feedback + assembly active** — assembly DROPS to inactive. Feedback is naturally parallel — multiple lenses simultaneously surface concerns. Forcing serialized turns turns feedback into a queue and kills its value. Server auto-disables assembly on transition to `feedback`; auto-re-enables on transition AWAY from `feedback` if the prior state was assembly-on.

`phase_changed` board event always fires. Assembly state changes that follow the phase change (the auto-disable/re-enable above) fire their own existing events.

## Out of scope

- AI-driven phase changes WITHOUT a pending human review. See "Autonomous-run gap" above — proposals are allowed, direct mutation by AI is not.
- Phase history / audit trail beyond the single `set_at` timestamp. The board.jsonl `phase_changed` events serve as the historical record; no separate file.
- Per-role overrides. The phase is team-wide. A role cannot opt out by saying "I'm in implement even though the team is in discuss." If they need to code in a discuss phase, they take the question to the human.
- Automatic phase advancement on time or events. The phase is human-set, not auto. A discussion that runs an hour doesn't auto-transition to implement.

## Implementation slice

This is its own slice, not bundled with anything in v1.0 or v1.1. Estimated cost:

1. Add `phase` field to protocol.json schema — vaak-mcp.rs + types.ts. Small.
2. Add `set_phase` MCP tool — vaak-mcp.rs. Small.
3. Add phase pill UI to Collab tab header — CollabTab.tsx. Medium.
4. Inline `Phase:` line in [YOUR TURN] body + banner — vaak-mcp.rs. Small.
5. Update role briefings to read phase. Medium (touches every role briefing).
6. Sidecar rebuild.

Skip-able first cut: ship steps 1, 2, 4, 6 as a backend-only v0.5; defer the UI pill (step 3) to a separate frontend slice. This lets the human use `set_phase` via MCP while the UI catches up. Faster to land, observable in the [YOUR TURN] line.

## Why this is worth shipping

The human's exact framing on msg 261: "you guys are not working as like a school of fish might... that signal is not happening right now... there's no aqueous solution between you that instantly communicates movements and thoughts." The pill is the proposed aqueous solution. One state, one writer, every role reads. It doesn't solve every coordination problem — it solves the specific one where each role independently picks priority and the team falls out of sync.

It is orthogonal to today's v1.0 fix (which addressed routing) and orthogonal to the moderation work (which addresses speaker-behavior). All three slices stack cleanly: rotation is correct (v1.0), phase tells everyone what to do (this slice), moderation handles the speaker who doesn't follow either (next slice after experiment).

## Open questions for architect

1. Should the `phase_changed` board event also auto-pause the assembly mic (force a clean break between phases) or just inform passively?
2. Are five values the right granularity, or do we need fewer (e.g., collapse "test" into "review") to keep briefings simple?
3. Should role briefings have a default per-phase posture, or should we leave the interpretation entirely to each role?

Architect to resolve before any implementation starts.
