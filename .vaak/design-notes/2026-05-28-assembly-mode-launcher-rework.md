# Assembly Mode Launcher Rework — UI-arch design spec

Date: 2026-05-28
Author: ui-architect:0
Driver: human:0 msg 2305 + 2313 (assembly-mode UI parity with Oxford/Delphi launchers)
Status: Spec — ready for developer-lane implementation

## Context

Today the Discussion Mode tab in CollabTab houses Assembly Mode as an always-rendered sidebar card (`AssemblyControls` + `ProtocolPanel`). The human flagged this as "weirdly isolated and inefficient" and asked for parity with the Oxford/Delphi launch pattern: a launcher button that opens a setup modal on demand. Also "way more customizable."

## Current state (verified)

- Launcher buttons exist for Oxford (CollabTab.tsx:6745-6753 → `OxfordSetupModal`) and Delphi (CollabTab.tsx:6785-6793 → `DelphiSetupModal`)
- Legacy `assembly-line-toggle` button at CollabTab.tsx:4168-4194 — green pill that toggles enable/disable; hidden when `twoControlsProtocol` is loaded
- `AssemblyControls` component (1233 LOC) renders the always-visible sidebar customization card via P5-v2
- `ProtocolPanel` renders the floor + consensus + 1-click yield/force-release row inline in CollabTab
- Tauri commands: `get_assembly_state`, `set_assembly_state(action: 'enable'|'disable')`, `protocol_mutate_cmd({action, args, rev})`
- Customization fields existing on `protocol.floor`: `assembly_active`, `mic_passing_mode` ('rotation'|'hand_raise'|'moderator'), `moderator`, `phase`, `plan_path`, `rotation_order`, `current_speaker`, `hand_queue`

## Target pattern

Match the Oxford/Delphi launcher pattern exactly:

1. **Inactive state** — single launcher button in the same row as Oxford/Delphi buttons:
   `🔁 Start Assembly Line` → opens `AssemblySetupModal`
2. **Setup modal** — hosts the customization surface (mic mode, moderator, preset, stall threshold, max floor) + a primary "Start Assembly Line" submit
3. **Active state** — launcher button replaced by `⏹ End Assembly Line` (parallel to End Oxford / End Delphi); active-state details rendered by the existing `ProtocolPanel` in the right rail
4. **Sidebar Discussion Mode card** — REMOVED (`AssemblyControls` no longer rendered always-visible; only inside the setup modal when configuring)

## File-by-file changes

### New file: `desktop/src/components/AssemblySetupModal.tsx`

Pattern reference: `OxfordSetupModal.tsx` (270 LOC) — minimal scaffold + customization controls + submit handler.

Props:
```ts
{
  open: boolean;
  projectDir: string;
  protocol: Protocol;          // current state for pre-fill
  mutate: Mutate;              // protocol_mutate_cmd wrapper
  activeSeats: string[];       // for moderator picker
  onClose: () => void;
  onStarted?: (assemblyState: {active: true; current_speaker: string|null; rotation_order: string[]}) => void;
}
```

Body sections (each is a labeled row):

1. **Mic passing mode** — `<select>` with `rotation` / `hand_raise` / `moderator`
2. **Moderator** — `<select>` from `activeSeats`, only enabled when mic_passing_mode = 'moderator'
3. **Preset** — `<select>` (placeholder: 'Assembly Line' for v1; expandable later)
4. **Stall threshold** — number input, default 180s, range 60-600
5. **Max floor seconds** — number input, default 300s, range 60-900
6. **Plan path** *(optional, for planning preset)* — text input

Submit flow:
1. If any customization changed from current protocol, dispatch `mutate('set_mic_passing', {...})`, `mutate('set_moderator', {...})`, `mutate('set_preset', {...})` in sequence
2. Call `invoke('set_assembly_state', {dir, action: 'enable'})`
3. Call `onStarted()` + `onClose()`
4. Surface server errors inline using the existing `friendlyError`/`displayError` baseline pattern

A11y: use `useModalA11y` (parity with OxfordSetupModal). Trap focus while busy.

### Edit: `desktop/src/components/CollabTab.tsx`

**Add** — at the Oxford/Delphi launcher button row (around line 6745, before the Oxford button):

```tsx
{assemblyState?.active ? (
  <button
    type="button"
    className="economy-settings-btn economy-settings-btn-destructive"
    onClick={async () => { /* call set_assembly_state action:disable */ }}
    title={`End assembly mode (current speaker ${assemblyState.current_speaker ?? '(none)'})`}
  >
    <span className="economy-settings-icon" aria-hidden="true">⏹</span>
    <span>End Assembly Line</span>
  </button>
) : (
  <button
    type="button"
    className="economy-settings-btn"
    onClick={() => setAssemblySetupOpen(true)}
    title="Start one-speaker-at-a-time mic control with rotation, hand-raise, or moderator picking"
  >
    <span className="economy-settings-icon" aria-hidden="true">🔁</span>
    <span>Start Assembly Line</span>
  </button>
)}
```

**Add** — modal mount near OxfordSetupModal / DelphiSetupModal mounts (around line 7670):

```tsx
{assemblySetupOpen && twoControlsProtocol && (
  <AssemblySetupModal
    open={assemblySetupOpen}
    projectDir={projectDir!}
    protocol={twoControlsProtocol}
    mutate={protocolMutate /* expose from useProtocolState */}
    activeSeats={activeSeats /* reused from existing list */}
    onClose={() => setAssemblySetupOpen(false)}
    onStarted={(s) => setAssemblyState({active: true, current_speaker: s.current_speaker, rotation_order: s.rotation_order})}
  />
)}
```

**Remove** — the always-rendered Discussion Mode wrapper at CollabTab.tsx:4205-4270 (the `(() => { ... return <ProtocolPanel ...>; })()` block) IF and ONLY IF the active-state ProtocolPanel is still rendered elsewhere in CollabTab. Verify before removing; if it's the sole render site, keep it gated on `assemblyState?.active`.

**Remove or repurpose** — the legacy `assembly-line-toggle` button at CollabTab.tsx:4168-4194. Once the new launcher ships, the legacy fallback is no longer required (the new launcher works regardless of `twoControlsProtocol` state because it pre-fills from the modal).

### Edit: `desktop/src/components/AssemblyControls.tsx`

Keep the component but expose its body subsections (the mic-mode row, moderator-picker row, etc.) as named exports so `AssemblySetupModal` can compose them directly without duplicating the customization UI. Lower-risk than refactoring; preserves the existing 1233 LOC test/behavior coverage.

If subsection extraction is too risky for one commit, the modal can simply render `<AssemblyControls protocol mutate lastError selfRole=null projectDir layout="vertical" />` inside its body — the human gets the customization surface inside a modal as the first deliverable, and a follow-up commit can replace it with cleaner extracted subsections.

## Known traps to design around

- **`project_assembly_enable_drops_late_joiners`** — enable seeds `rotation_order` only at toggle time. The modal MUST either re-emit `assembly_line.enable` on roster-membership change OR surface a "ROTATION OUT OF DATE — N seats joined" warning with one-click "Refresh rotation." Recommended: warning + button (surfaces the issue to the operator rather than hiding it).
- **`project_assembly_mode_gaps_2026_05_04`** — zombie speakers stick the mic. The active-state ProtocolPanel must render heartbeat-freshness on the current-speaker badge (red border if `last_alive_at_ms` > stall threshold). Cross-ref `project_dual_heartbeat_trackers` — read BOTH heartbeat sources.
- **`project_assembly_v1_corrected_2026_05_13`** — assembly v1.0 corrected (4-commit chain 453228c..7895a03) ships the rotation_order discipline. Confirm new modal doesn't regress by NOT calling `assembly_line.enable` (which seeds rotation_order from active seats); always use the canonical enable path.

## Out of scope for this commit

- Per-seat enable/disable
- Rotation reordering by drag-and-drop
- Manual speaker override (mod-only fast-flip)
- v2 customization (review intensity slider integration, plan-path editor inside modal)

## Acceptance test

1. Inactive state: launcher button visible alongside Oxford/Delphi buttons; sidebar Discussion Mode card NOT rendered
2. Click "Start Assembly Line" → modal opens with current `protocol.floor` values pre-filled
3. Change mic mode to 'moderator' → moderator picker enabled; selecting a seat dispatches `set_moderator`
4. Submit → `set_assembly_state(action: 'enable')` fires; modal closes; launcher button now reads "End Assembly Line"
5. Active state: `ProtocolPanel` renders inline with current speaker + rotation order; existing `current_speaker` heartbeat polling continues
6. Click "End Assembly Line" → confirms (optional) → `set_assembly_state(action: 'disable')` fires; launcher reverts to "Start Assembly Line"
7. Regression: legacy `assembly-line-toggle` button no longer rendered in any code path

## Handoff

developer:0 to implement after SHA-D10.4 (Delphi sweeper) lands. ui-architect:0 (me) standby for design review on the AssemblySetupModal mockup + the CollabTab diff before merge.

Estimated scope: ~350 LOC new (AssemblySetupModal) + ~50 LOC changed (CollabTab). One commit.

## References

- `OxfordSetupModal.tsx` (270 LOC) — reference pattern
- `DelphiSetupModal.tsx` (386 LOC) — reference pattern  
- `AssemblyControls.tsx` (1233 LOC) — customization surface to be composed
- `useProtocolState.ts:238-272` — `mutate` IPC wrapper
- CollabTab.tsx:4168-4194 (legacy toggle), :4205-4270 (sidebar Discussion Mode wrapper), :6745-6793 (Oxford/Delphi launchers), :7670-7703 (modal mounts)
- Memory `project_assembly_enable_drops_late_joiners` / `project_assembly_mode_gaps_2026_05_04` / `project_assembly_v1_corrected_2026_05_13` / `project_dual_heartbeat_trackers`
