# Unified Launch-Row UI Spec — UI-arch design (expanded scope)

Date: 2026-05-28
Author: ui-architect:0
Driver: human:0 msg 2305 + 2313 + architect:1 msg 2316 handoff
Status: Spec — supersedes `2026-05-28-assembly-mode-launcher-rework.md` (scope grew from Assembly-only to all-modes-unified per architect:1)

## Context

Per architect:1 msg 2316: unify Oxford / Delphi / Assembly / Continuous (and future modes) under ONE launch-row component with per-mode popup-config UI. The current state — Oxford and Delphi as buttons, Assembly as a sidebar card, Continuous as a discussion-control toggle — was called "isolated and inefficient" by human msg 2313. Architect:1's reframe folds ui-arch H1/H2/H3 (visibility/render-conditional/CTA) and evil-arch H4 (existing-but-broken late-joiner rotation) INTO the same rework wave.

## Mode inventory (slots in the unified launch-row)

| Slot | Mode | Inactive label | Active label | Mode-specific config-popup content | Initiator IPC | Active state surface |
|---|---|---|---|---|---|---|
| 1 | Oxford | `⚖ Start Oxford Debate` | `⏹ End Oxford Debate` | moderator + side_a + side_b + audience + premise + reward (denominated) | `oxford_initiate_cmd` | `ActiveOxfordPanel` (already at CollabTab.tsx:6135+) |
| 2 | Delphi | `🔮 Start Delphi Discussion` | `⏹ End Delphi Discussion` | participants + moderator + topic + max_rounds + reward + audience | `delphi_initiate_cmd` | inline aggregate render + active state polling |
| 3 | Assembly | `🔁 Start Assembly Line` | `⏹ End Assembly Line` | mic_passing_mode (rotation/hand_raise/moderator) + moderator + preset + stall_threshold + max_floor + plan_path | `set_assembly_state(action: enable)` + `protocol_mutate_cmd` for customization | `ProtocolPanel` (already at CollabTab.tsx:4262) |
| 4 | Continuous | `🔄 Start Continuous Review` | `⏹ End Continuous Review` | trigger_window + quorum_pct + silence_timeout (30s/60s/2m/5m) | `discussion_control` set continuous | `DiscussionStatusPanel` (existing) |
| 5+ | (future) | reserved | reserved | tier-N+1 modes (Red Team, Continuous Improvement, etc.) | (TBD) | (TBD) |

Slot order is intentional: most-formal (Oxford) → least-formal (Continuous), reading left-to-right. Future modes append.

## Visual design — launch-row component

### Placement

Replace today's mixed-surface set:
- REMOVE legacy `assembly-line-toggle` pill at CollabTab.tsx:4168-4194
- REMOVE always-rendered Discussion Mode sidebar wrapper at CollabTab.tsx:4205-4270
- REPLACE the Oxford/Delphi button group at CollabTab.tsx:6745-6794 with the new unified `<LaunchRow />` component

The launch-row sits at the TOP of the existing economy-settings band (high-discoverability, always-visible, single source of truth for mode-state).

### Per-mode color coding

Each button uses a mode-tinted accent color, NOT a hue collision with priority/status semantics:

| Mode | Accent (hex) | Reasoning |
|---|---|---|
| Oxford | `#4a4a4a` neutral graphite | formal/judicial register |
| Delphi | `#5b3e8f` deep violet | "blind/mystic" — mirrors the 🔮 icon |
| Assembly | `#137333` green (matches today's legacy pill) | preserves operator muscle memory |
| Continuous | `#1a5fb4` blue | "flow / streaming" register |

Inactive state: 1px accent-color border + transparent fill + accent-color label/icon. Active state: solid accent fill + white label + destructive-red secondary border (to read "this can end the mode"). Disabled state (mode unavailable, e.g. while another mode is exclusive-running): 30% opacity + cursor: not-allowed.

### Mode exclusivity rules

- Oxford and Delphi are MUTUALLY EXCLUSIVE — only one of those two can be active at a time (today's behavior; preserve it). When one is active, the other's launcher is disabled with a tooltip "Cannot start while {other-mode} is active."
- Assembly is INDEPENDENT — can be ON during Oxford or Delphi (Assembly governs mic rotation; Oxford/Delphi declare speaker order through their own phase machines, which Assembly defers to).
- Continuous is INDEPENDENT — can co-exist with Assembly, mutually exclusive with Oxford/Delphi.

The unified launch-row visually reflects this: disabled-state styling on the exclusivity-blocked launchers, with the tooltip explanation.

### Active-mode indication

Each active mode gets a thin (2px) bottom-border colored bar UNDER its launcher button, like a tab-active underline. Multiple bars can show simultaneously (Assembly + Oxford both active = two bars). The destructive-red secondary border on the launcher itself also signals "you can end this from here."

### Late-joiner rotation warning (per evil-arch H4 / `project_assembly_enable_drops_late_joiners`)

Above the Assembly launcher when assembly_active and `rotation_order.length < activeSeats.length`:
> ⚠ Rotation out of date — {N} seats joined since enable. **Permanent fix in Commit C.**

**REMOVED from Phase 1 (per evil-arch:0 msg 2328 empirical finding):** the previously-spec'd "Refresh rotation" button is REMOVED. Evil-arch:0 verified `assembly_line.enable` when already-enabled does NOT re-seed `rotation_order` and does NOT update `started_at` (it's either a no-op or the seeding code is dead-path-after-first-call). Shipping a button that calls a broken path would be a UI affordance that does nothing — worse than no affordance.

**Replacement (Phase 1):** passive warning only. No actionable button until backend re-seed path is verified working.

**Replacement option for Phase 1+1 if backend not yet fixed:** "Refresh" could call `disable` THEN `enable` in sequence (the sequence-cycle workaround evil-arch:0 recommended in msg 2328). Only ship that if testing confirms the disable→enable cycle actually re-seeds; if even that doesn't work, no UI affordance is honest.

**Hard prerequisite:** Phase 4 (Commit C — append-on-join + assembly_line.enable-actually-re-seeds fix) MUST land before any "Refresh rotation" button is added to the UI. Phase 1 ships warning-without-button; Commit C ships the button + permanent fix together.

## Visual design — config popup shell

Shared shell, per-mode body. The shell provides:
- Header: mode name + icon + close button + (when active) end-mode button
- Body slot: per-mode form (Oxford uses sides + premise + reward; Delphi uses participants + rounds; Assembly uses mic-mode + moderator + presets; Continuous uses timeout)
- Footer: validation summary + primary action button (start / save customization / end)

Modal A11y: shared `useModalA11y` hook (already used by OxfordSetupModal). Focus trap + escape-to-close (disabled while busy) + focus-on-mount to the first input.

### Popup state for active mode

When the user clicks an ACTIVE mode's launcher, the popup opens in CONTROL mode (not setup mode):
- Header shows live status (current_speaker, current_phase, etc.)
- Body shows mode-specific control affordances (advance phase, declare speaker, kick, force-end)
- Footer shows the destructive end-mode button with confirm

This is the "popup becomes live control surface" pattern from architect:1 msg 2316.

## Component file layout

```
desktop/src/components/
  LaunchRow/
    index.tsx                  — main launch-row container, polls all mode states
    LaunchButton.tsx           — single mode button (active/inactive/disabled)
    ConfigPopupShell.tsx       — shared shell (header + body slot + footer)
    modes/
      OxfordConfig.tsx         — Oxford setup body (wraps existing OxfordSetupModal body)
      OxfordControl.tsx        — Oxford active-state body (advance/declare/yield/end)
      DelphiConfig.tsx         — Delphi setup body
      DelphiControl.tsx        — Delphi active-state body (open round / close round / end)
      AssemblyConfig.tsx       — Assembly setup body (composes existing AssemblyControls subsections)
      AssemblyControl.tsx      — Assembly active-state body (rotation order + late-joiner warning + Refresh)
      ContinuousConfig.tsx     — Continuous setup body
      ContinuousControl.tsx    — Continuous active-state body
  LaunchRow.module.css         — accents, spacing, exclusivity-disabled, active-underline
```

Avoid: ONE 800-LOC LaunchRow.tsx. The per-mode bodies must be separate files so future modes append without touching the shell.

## Phased implementation

**REVISION per architect:0 msg 2330 binding ruling:** the previously-numbered "Phase 4" backend fixes (`assembly_line.enable` re-seed + late-joiner append-on-join) are PROMOTED to **Phase 0 PREREQUISITE** for any Phase 1 UI ship. UI design and implementation can proceed in parallel; UI SHIP is gated on Phase 0 landing. Rationale: a "Refresh rotation" button calling a verified-broken enable path is a UI affordance that does nothing — worse than no affordance. Architect-lane standing position: ship UI against a working backend.

**Phase 0 — backend Commit C (developer-lane PREREQUISITE for Phase 1 ship):**
- Per `project_assembly_enable_drops_late_joiners` + architect:0 msg 2330 ruling + evil-arch:0 msg 2328 empirical finding
- Fix `rotation_order` append-on-late-join in `assembly_line` so future joiners are picked up automatically
- **AND** fix `assembly_line.enable`-when-already-enabled to actually re-seed `rotation_order` from current roster + update `started_at` (today's behavior is a verified no-op; tonight's `disable+enable` cycle showed unchanged `started_at: 2026-05-24T05:42:04Z`)
- Both fixes are complementary: append-on-join doesn't help already-existing late joiners (dev-challenger:0 + ux-engineer:0 joined days ago); re-seed alone doesn't help future joiners after the next enable

**Phase 1 — shell + Assembly mode (UI commit, gated on Phase 0 landing):**
- Create `LaunchRow/` with shell + `AssemblyConfig` + `AssemblyControl`
- Wire into CollabTab.tsx replacing the legacy toggle + sidebar Discussion Mode card
- Verify against acceptance tests in §"Acceptance"
- DO NOT yet migrate Oxford/Delphi/Continuous — leave their existing buttons in place but mark them for Phase 2
- This phase fully resolves the human's immediate ask (assembly-mode UI parity)

**Phase 2 — Oxford + Delphi migration (follow-up commit):**
- Migrate Oxford button → LaunchRow slot 1 with `OxfordConfig` + `OxfordControl`
- Migrate Delphi button → LaunchRow slot 2 with `DelphiConfig` + `DelphiControl`
- Remove the standalone Oxford/Delphi launch buttons at CollabTab.tsx:6745-6794

**Phase 3 — Continuous migration (follow-up commit):**
- Migrate continuous toggle → LaunchRow slot 4 with `ContinuousConfig` + `ContinuousControl`

**(Phase 4 — DEPRECATED:** see Phase 0 above. The previously-numbered Phase 4 was the backend Commit C fix; it has been promoted to Phase 0 PREREQUISITE per architect:0 msg 2330 ruling. No work remains under the Phase 4 number.)

## Acceptance — Phase 1

1. Inactive state: only one button shown in launch-row slot 3 — "🔁 Start Assembly Line" with green accent + transparent fill
2. Click → popup opens with current `protocol.floor` values pre-filled in AssemblyConfig body
3. Customization fields work: mic_passing_mode change dispatches `set_mic_passing`; moderator change dispatches `set_moderator`; preset change dispatches `set_preset`
4. Submit "Activate Assembly Line" → `set_assembly_state(action: 'enable')` fires; popup closes; launcher button transitions to "⏹ End Assembly Line" with solid green fill + red secondary border + 2px green underline
5. Active state click → popup opens in CONTROL mode with rotation order + passive late-joiner warning (no actionable button until Commit C) + End Assembly Line button
6. Late-joiner warning fires correctly when `rotation_order.length < activeSeats.length`; warning text reads "⚠ Rotation out of date — {N} seats joined since enable. Permanent fix in Commit C." No "Refresh rotation" button until Commit C lands (per evil-arch:0 msg 2328 — current enable-when-already-enabled is a verified no-op).
7. End Assembly Line confirms (window.confirm parity with End Oxford/Delphi) → `set_assembly_state(action: 'disable')` fires; launcher reverts
8. Regression: legacy `assembly-line-toggle` at CollabTab.tsx:4168-4194 NO LONGER rendered in any code path
9. Regression: always-rendered Discussion Mode sidebar wrapper at CollabTab.tsx:4205-4270 NO LONGER rendered (verify no orphaned ProtocolPanel; if ProtocolPanel was the sole render site there, move it to render under AssemblyControl active state OR keep it inline gated on `assembly_active`)

## Known traps designed-around

- `project_assembly_enable_drops_late_joiners` — passive late-joiner warning in Phase 1 (Refresh button DEFERRED to post-Commit-C per evil-arch:0 msg 2328 empirical finding that enable-when-already-enabled is a no-op); Commit C (Phase 4) bundles both the append-on-join fix AND the enable-actually-re-seeds fix
- `project_assembly_mode_gaps_2026_05_04` — current-speaker badge in AssemblyControl popup body MUST render heartbeat-freshness (red border if `stale_ms > stall_threshold_secs * 1000`)
- `project_dual_heartbeat_trackers` — read BOTH heartbeat sources (sessions.json:bindings:last_heartbeat AND .vaak/sessions/*.json:last_alive_at_ms) when rendering liveness.
  - **Composition rule (per dev-challenger:0 msg 2390 Finding 3):** when the two trackers disagree, show "(checking…)" — neither source's verdict is asserted in isolation. Only when BOTH agree the seat is stale do we surface "(reconnecting…)". Rationale: dev-challenger:0 msg 2390 empirically confirmed the divergence is real (ux-engineer's per-seat file 9h stale while bindings 30s fresh), so a single-source verdict is empirically known to lie. This bias is TOWARD "(checking…)" over "(reconnecting…)" because false-reconnecting is user-disruptive UX, while false-checking surfaces the actual ambiguity. Shipped as SHA-RC.1 commit `14ef026`.
  - **seatAliveMap ownership (per dev-challenger:0 msg 2390 Finding 5):** CollabTab.tsx remains the SOLE owner of the 30s `list_active_seats_cmd` poll. The LaunchRow shell + per-mode bodies consume the resolved `seatAliveMap` via props (lifted state pattern), never running their own poll. Rationale: one poll, one truth, no race-to-write between component trees.
- `project_assembly_v1_corrected_2026_05_13` — Phase 1 must NOT regress the rotation_order discipline; canonical enable path is the only mutator
- `project_ts_change_needs_tauri_rebuild_and_restart` — Phase 1 is TS-only; ship via `npm run build` but expect activation only after `cargo build --release` if any Tauri command surface extension is needed
- `project_tauri_rust_struct_strips_undeclared_fields` — if Phase 1 adds new ProtocolPanel fields, declare them on the Rust side (`ProjectSettings` / `Protocol.floor` structs) before they'll round-trip

## Mode exclusivity gate location (per dev-challenger:0 msg 2390 Finding 2)

The BUSINESS LOGIC that prevents `oxford_initiate_cmd` from firing when `delphi.is_active === true` lives in the **LaunchRow shell** (centralized), NOT in each `*Config.tsx` body (decentralized).

Rationale:
- Centralized gating means one place to enforce + audit the exclusivity contract
- Decentralized gating risks each per-mode body re-implementing slightly different rules and drifting (the kind of multi-writer-shared-state divergence audit-completed-2026-05-27 warned against)
- The disabled-launcher visual + tooltip is presentation; the gate that rejects clicks while another exclusive mode is active is logic — both live in the shell

Today's enforcement (pre-LaunchRow): the existing standalone Oxford/Delphi/Assembly/Continuous buttons at CollabTab.tsx:6745+ DO NOT explicitly gate exclusivity — backend rejects the second init with `[OxfordAlreadyActive]` / `[DelphiAlreadyActive]` and the UI catches the error in the toast. This is a soft-gate (post-hoc UI feedback). SHA-LR.2 preserves the soft-gate; future LaunchRow shell extraction should harden to a pre-click hard-gate (disable the button visually + skip the IPC entirely while another exclusive mode is active).

## Existing Oxford/Delphi state plumbing (per dev-challenger:0 msg 2390 Finding 1)

When the LaunchRow shell extraction lands (Phase 2/3 of the unified-row migration), `OxfordConfig` / `DelphiConfig` / `AssemblyConfig` bodies should CONSUME the existing state hooks rather than re-implement:
- Oxford: `useState<typeof activeOxford>` + `oxford_initiate_cmd` IPC (unchanged from SHA-LR.1 / SHA-LR.2)
- Delphi: `setActiveDelphi` callback + `delphi_initiate_cmd` IPC (unchanged)
- Assembly: `useProtocolState`-derived `mutate` + `set_assembly_state` (unchanged from SHA-LR.1)
- Continuous: `setDiscussionState` + `start_discussion` IPC (preserved by SHA-LR.2)

Re-implementing any of these in a per-mode body risks dropping state-machine invariants (e.g., Oxford's optimistic-phase=opening_a seed in setActiveOxford onStarted at CollabTab.tsx:7684).

## Out of scope for Phase 1

- Drag-to-reorder rotation_order
- Manual speaker override (moderator-fast-flip surface)
- Review intensity slider integration into AssemblyConfig (deferred — has its own design at `project_review_intensity_slider_v1y`)
- Customization persistence per-section vs project-wide (defer to architect-lane ruling)
- Audience-tier UI for Assembly Mode (assembly has no audience concept; N/A)

## Out of scope for spec entirely

- Mode-creation UI (users defining new modes) — strictly developer-lane
- Mode-permission gating (per-role enable/disable) — deferred per architect:0 msg 2306 "no permission gating in v1"

## Handoff to developer:0

Phase 1 estimated scope: ~600 LOC across LaunchRow shell + AssemblyConfig + AssemblyControl + CollabTab integration + CSS module. Two-to-three reviewed commits is healthier than one mega-commit.

Suggested commit boundaries:
- Commit A: LaunchRow shell + LaunchButton + ConfigPopupShell + CSS module (no mode bodies; visually inert)
- Commit B: AssemblyConfig + AssemblyControl + CollabTab wire-in (Phase 1 acceptance shippable)
- Commit C (parallel, ux-engineer or developer): late-joiner rotation_order append fix per evil-arch H4

ui-architect:0 (me) on standby for Commit A/B design review before merge. Phase 2/3 specs to be written after Phase 1 is verified by the human.

## References

- `OxfordSetupModal.tsx` (270 LOC) — Phase 1 modal-A11y reference
- `DelphiSetupModal.tsx` (386 LOC) — Phase 1 modal-A11y reference
- `AssemblyControls.tsx` (1233 LOC) — AssemblyConfig body composition source
- `useProtocolState.ts:238-272` — `mutate` IPC wrapper for customization dispatch
- CollabTab.tsx:4168-4194 (legacy toggle to remove), :4205-4270 (sidebar wrapper to remove), :6135+ (ActiveOxfordPanel — keep), :6745-6794 (Phase 2 migration target)
- architect:1 msg 2316 — scope reframe
- evil-architect:0 msg 2314 — H4 + class-of-bug ask
- ui-architect:0 msg 2311 — verify-before-asserting correction + H1/H2/H3
- Memory: `project_assembly_enable_drops_late_joiners`, `project_assembly_mode_gaps_2026_05_04`, `project_dual_heartbeat_trackers`, `project_assembly_v1_corrected_2026_05_13`, `project_ts_change_needs_tauri_rebuild_and_restart`, `project_tauri_rust_struct_strips_undeclared_fields`
