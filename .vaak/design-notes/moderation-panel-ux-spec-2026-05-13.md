# Moderation Panel — UX Spec

Author: ux-engineer:0
Date: 2026-05-13
Status: draft for architect + UI-architect adversarial review; observe-first — refine after first live moderator experiment (moderator:0 in active assembly)

## Purpose

Give the human (and, optionally, a designated AI moderator) a single UI surface for fast intervention during active assembly. Today the moderation primitives exist scattered across MCP tools (`force_release`, `project_kick`, `protocol_mutate`, the rule 4 halt/resume mechanism) but there is no UI that consolidates them. When a speaker rambles or goes off-topic, the human's only recourse is typing MCP commands or waiting 120s for the watchdog. That is slow, clunky, and asymmetric — the moderation surface is server-side; the human's surface is the terminal.

The panel is a side rail or top-bar widget in the Collab tab that surfaces the LIVE assembly state and offers ONE-CLICK actions per common intervention.

## Live-state display (read-only top half of the panel)

Five fields, always visible when assembly is active. Disappears entirely when assembly is inactive (no vestigial widget).

1. **Current speaker** — role:instance, with their `activity` field (from a627daf TTL). Example: `developer:0 — implementing`.
2. **Held since** — duration the current speaker has held the mic, refreshed at 1Hz from `mic_held_secs`. Color-coded: green <30s, amber 30-90s, red >90s (close to watchdog threshold of 120s).
3. **Rotation** — full `rotation_order` as a horizontal chip strip. Current speaker highlighted (filled background); previous speaker outlined; others plain. Each chip shows role label + activity tag (matches the [YOUR TURN] rotation line from 84f6c15).
4. **Pending floor halt** — when `floor.halted_for_human == true`, a banner shows "FLOOR HALTED — waiting on human" with the yielder name + the substantive `ask` text. Click clears the halt manually (calls protocol_mutate to clear the flag) for "I have no answer, continue."
5. **Wait times** — for each seat in rotation_order, time since their last accepted send. Surfaces excluded seats automatically (the bug class human msg 379 named).

## Action surface (bottom half of the panel)

Five buttons in a row. ONE-CLICK semantics. No mandatory reason fields, no typed confirmation except for "End assembly."

1. **Skip** — advances the mic to next-in-rotation_order without an accepted send. Useful when the current speaker is stalled but still alive. Server tool: `protocol_mutate` with `action: skip_speaker` (new tool). Audit event: `floor_skipped_by_moderator` on board.jsonl.
2. **Grant to…** — opens a popup with the rotation_order list; click a seat and the mic transfers there deliberately, breaking strict rotation. For when a topic explicitly needs a specific lens. Server tool: `protocol_mutate` with `action: grant_floor, target: <seat>`. Audit event: `floor_granted_by_moderator`.
3. **Pause** — halts the rotation entirely. Watchdog turns off. No mic landings. Used when the team needs to wait on something external (e.g., a long-running build). Re-enable via the same button (toggle). Server tool: `assembly_line action: pause / resume` (new sub-actions).
4. **Warn** — sends a directed system message to the current speaker without ejecting them. "Stay on topic" or "speed up." Free-text input (single line, max 200 chars), but the input is optional — defaults to a template ("Moderator nudge — refocus on the current topic"). Audit event: `speaker_warned_by_moderator` on board.jsonl with the warning text.
5. **End** — closes assembly. Requires one-click confirmation ("Click again to end") to avoid accidental ends. No reason field. Server tool: `assembly_line action: disable`. Audit event: `floor_ended_by_moderator`.

## Design questions for adversarial review

1. **Moderator = always human, or AI-delegatable?** `protocol.json` already has a `moderator` field that supports both. If always-human, the panel is human-only and AI roles never see it. If AI-delegatable, an AI role's briefing must specify "you have moderator authority" and the same panel renders for them. Tradeoff: AI moderation enables 24/7 coverage but recreates the parallel-decision problem the assembly line was designed to prevent.

2. **Actions visible on the board, or silent?** I've specified board.jsonl audit events for all five actions. Open whether they render to other watchers as system messages (visible noise but accountability) or fire silently (clean board but moderator actions feel like a private channel). Recommend visible — every other v1.0 state transition is observable on the board.

3. **One-click semantics for everything except End — am I right about Warn?** Warn fires immediately with the template if the moderator hits Enter without typing. Alternative: Warn requires at least 5 characters of free-text input to prevent accidental sends. Lean toward immediate-with-template for the same UX principle as the rest (friction at intervention time = no intervention).

4. **Where does the panel live in the Collab tab?** Three options: (a) right-side rail (always visible), (b) collapsible drawer attached to ProtocolPanel (toggles open when assembly is active), (c) top-bar inline widget that expands on hover. (b) is my preference — visible when relevant, hidden when not, no permanent screen real estate cost.

5. **What happens to the panel during rule 4 halt?** All five action buttons stay available — moderator can override the halt by Skipping or Granting or Ending while the human's substantive yield is pending. Otherwise the halt could trap the team if the human never responds. Open whether this overrides should auto-clear the halt flag or leave it set; current proposal: any moderator action clears `halted_for_human` as a side effect and emits both the action's audit event AND a `floor_resumed_by_moderator` event.

## Phase × Moderation interaction

If the phase pill (`phase-pill-spec-2026-05-13.md`) eventually ships, the moderator panel's button labels could adjust per phase:
- `discuss` phase: Skip/Grant/Pause/Warn/End render normally — the standard floor-management toolkit
- `implement` phase: Skip→"Next implementer," Warn defaults to "refocus on the current spec" — phase-aware copy
- `review` phase: Grant defaults to surface only review-capable roles in the popup
- `feedback` phase: Pause is hidden (feedback is naturally parallel; pause makes no sense)

Deferred until phase pill unparks. Not load-bearing.

## Out of scope

- Moderator voting / consensus mechanics. Single-moderator authority for v1.x — multi-moderator is a v2 question.
- History of moderator actions as a navigable timeline. board.jsonl already records them; a dedicated history view is v2 polish.
- Mobile / tablet layout. Desktop-only for now.
- Auto-moderation (server-side rules that skip/warn without a moderator click). Off-table per the human's msg 411 directive against over-constraining team thinking.

## Implementation slice (if/when authorized)

1. Backend MCP additions: `protocol_mutate` sub-actions (skip_speaker, grant_floor, pause_assembly, resume_assembly, warn_speaker) + `assembly_line` pause/resume sub-actions. Server emits the audit events. ~80 lines in vaak-mcp.rs.
2. Frontend `ModeratorPanel.tsx` component (new file, per UI-architect's extraction outline pattern). Reads live state from `useProtocolState` (existing hook). Renders top-half display + bottom-half action surface. ~200 lines.
3. Mount inside CollabTab.tsx as a collapsible drawer adjacent to ProtocolPanel. ~5 lines.
4. Briefing additions for designated AI moderator role (only if Q1 lands as AI-delegatable).

Estimated cost: half-day frontend + small backend + sidecar rebuild + dist rebuild. Same observe-first cadence as everything else from today.

## Acceptance test for this spec

Run a controlled assembly with `moderator:0` summoned (per evil-architect msg 254's "summon the moderator role we already have"). Watch what the moderator actually NEEDS to do in 10 turns of activity. Failures of the existing tools become the actual feature list; my five action proposals are educated guesses against that real friction. If the moderator needs something I didn't list, add it. If they never use Warn, drop it.

The spec is BEFORE the experiment because the human asked for it tonight (msg 556 implicitly — "lazy as fuck"). The experiment is the validator, not the gate.
