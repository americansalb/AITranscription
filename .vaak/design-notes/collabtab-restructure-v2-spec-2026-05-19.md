# CollabTab Restructure v2 — Spec (msg 5450 redesign)

Owner: architect:0
Date locked: 2026-05-19
Trigger: human msg 5450 + reinforced by msg 5567/5568 ("loss of function" regression complaint)
Supersedes scope of v1 chain's "Discussion Mode card" — runtime state moves OUT of the band and ONTO the role cards.

## Problem statement

Human msg 5450: assembly **operational state** (mic-holder, rotation order, moderator) is a property of the AGENTS, so it should be visualized ON the agent cards rather than in a separate Discussion Mode panel. Settings (mode toggle, plan path) can stay separate. Plus: must work at 10+ agent density.

Human msg 5567 / msg 5568: the mic-passing/arrows UI is missing from current UI — this is felt as a regression. Visualization needs to come back, ideally integrated with team roster or with colors/avatars/abbreviations in the Discussion Mode section.

The v1 chain consolidated the Discussion Mode card but left runtime state inside a separate panel. v2 relocates state onto cards.

## Final architecture (locked)

### Visual treatment by state dimension

| State dimension | Visual treatment | Location |
|---|---|---|
| Mic-holder | 2-3px accent border + small mic glyph top-right | On the role card holding the mic |
| Rotation position | Card position via sort order | Roster ordering |
| Moderator | Gold/amber star (★) badge top-left | On the moderator's role card |
| Phase / topic | One-line strip (~12-16px tall) | Above Team band header OR inside it |
| Alive state (existing) | Compound dot (role-color + amber ring when stale) | Bottom-right of card per keepalive v3 |
| Active claim (post-claims-arch) | Small badge | Bottom-right of card |

### Animation cadence

Mic transfer A → B:
- Old holder: border fades accent → transparent over ~200ms
- New holder: border fades transparent → accent over ~200ms (optional 1-cycle pulse)
- Total: ~300-400ms
- Reuses `transition: border-color 0.2s ease-out` from existing F-UIA-CTR-3 contract

### Density behavior

Auto-switch to chip-view (~80-100px compact cards) when:
- Assembly mode active AND agent count > 6

Chip card content (5 visual elements, disciplined spacing):
- Role title (1 line, ellipsis)
- Mic-holder border (when applicable)
- Moderator star top-left (when applicable)
- Alive dot bottom-right
- Active-claim badge bottom-right (post-claims-arch)

Grid view (default <=6 agents) shows expanded info.

### Sort behavior

When assembly mode active:
- Cards sort BY `rotation_order` (card position = rotation index)
- Agents NOT in rotation_order sort AFTER, separated by 1px hairline divider
- Roster reads left-to-right (grid) or top-to-bottom (list) as natural rotation order

When assembly NOT active:
- Roster reverts to current sort (role-priority / vacancy / alphabetical — whichever is in place pre-v2)

### Discussion Mode band fate

Post-v2, the existing "Discussion Mode: Assembly Line" CollapsibleSection band has reduced purpose:
- AssemblyControls → moves to settings popover (gear icon on Team band header → popover with rotation-order edit / mode toggle / phase advance)
- ProtocolPanel → minimized to phase/topic strip per location above
- Empty band itself can be DELETED

Sister-fix-CB3 (default-expanded when assembly active) is short-lived bridge between v1 chain and v2 — that's acceptable.

## Implementation chain (locked sequencing)

Per pre-req-first architectural discipline (mirrors `8162d3f` → Change C pattern):

### 1. Pre-req commit: ProtocolStateContext extraction

`desktop/src/contexts/ProtocolStateContext.tsx` mirrors `ProjectDirContext` pattern from pre-req 8162d3f. Closes F-EA-MSG5450-3 dual-mount divergent-writer concern.

Scope: ~80-100 LOC.

Contract:
- Exposes `protocol` (typed) + read-only accessors for `mic_holder`, `rotation_order`, `moderator`, `phase`, `topic`
- Provider wraps Team band and any other consumer
- Memoized value via `useMemo` + stable accessor refs via `useCallback`
- TypeScript strict (explicit context value type, throw-if-no-provider hook)

### 2. Roster-card-decoration commit

`desktop/src/components/CollabTab.tsx` + `desktop/src/styles/collab.css`:
- Mic-holder accent border (CSS class `.role-card-mic-holder`)
- Moderator gold star badge (CSS class `.role-card-moderator-badge`)
- Reduced-motion respect on transition
- `prefers-reduced-motion` disables pulse + cross-fade

Scope: ~120-180 LOC.

### 3. Rotation-sort commit

`desktop/src/components/CollabTab.tsx`:
- New sort comparator that derives card order from `protocol.rotation_order` when assembly active
- Hairline divider between in-rotation and out-of-rotation agents
- Reverts to previous sort when assembly inactive

Scope: ~50-80 LOC.

### 4. Phase/topic strip commit

New component `PhaseTopicStrip.tsx` placed inside the Team band header (architect call from F-UIA-CTR-V2-VIS4):
- One-line strip showing `Phase: <phase> · Round <n>/<m> · Topic: <topic>`
- Auto-elides at narrow widths
- Hides entirely when no assembly mode active

Scope: ~60-100 LOC.

### 5. Density auto-switch commit

`CollabTab.tsx` roster view-mode logic:
- Auto-switch to chip-view when `protocol.rotation_order.length > 6 AND assembly_active`
- User manual override still persists per existing `vaak_roster_view_mode` localStorage key
- Auto-override applies only when user hasn't manually toggled in current session

Scope: ~40-60 LOC.

### 6. Settings popover commit (replaces Discussion Mode band)

`AssemblyControlsPopover.tsx` — moves AssemblyControls content into a popover triggered by gear icon on Team band header:
- Rotation-order edit
- Mode toggle (Start/Stop Assembly)
- Plan path
- Phase advance

Scope: ~80-120 LOC.

### 7. Discussion Mode band deletion commit (cleanup)

Final cleanup: delete `<CollapsibleSection id="discussion-mode-section">` block from CollabTab.tsx + the `.discussion-mode-section` CSS override.

Scope: ~20-40 LOC removal.

## Total scope estimate

~450-680 LOC across 7 commits (pre-req + 5 features + deletion).

## Three-gate per commit

Per Ruling 13 standard four-gate pattern. Per F-DC-KRL2, gate verbosity proportional to commit size — smaller commits get 1-paragraph verdicts.

## Adversarial flags (folded from msg 5458, 5460, 5470)

| Flag | Resolution |
|---|---|
| F-EA-MSG5450-1 ProtocolPanel fate | Becomes phase/topic strip + settings popover |
| F-EA-MSG5450-2 10+ agent density | Chip-mode auto-switch at >6 agents + rotation-by-sort eliminates per-card badges |
| F-EA-MSG5450-3 ProtocolStateContext | Pre-req commit 1 |
| F-EA-MSG5450-4 question batching | Single decision-panel batched question (not multi-card) |
| F-UIA-CTR-V2-VIS1 mic visual | Strong accent border (not subtle icon) |
| F-UIA-CTR-V2-VIS2 rotation visual | Card sort order, no per-card badge |
| F-UIA-CTR-V2-VIS3 moderator visual | Gold star top-left |
| F-UIA-CTR-V2-VIS4 phase/topic | One-line strip in Team band header |
| F-UIA-CTR-V2-VIS5 Discussion Mode band fate | Replaced by settings popover, band deleted |
| F-UIA-CTR-V2-VIS6 chip-mode auto-switch | At >6 agents in assembly mode |
| F-UIA-CTR-V2-VIS7 mic-transfer animation | 200ms ease-out cross-fade, reduced-motion respected |

## Forward-flags (post-v2 sister-fixes or v3 candidates)

- Pulse animation choreography on mic-transfer (1-cycle on new holder) — v2 polish
- Animation on rotation-order change — v3 candidate
- Avatar-overlay alive-state badge per Phase 2.F precedent — v3 polish
- Multi-mode dropdown when Oxford Debate or other modes ship — emerges naturally from settings popover when 2nd mode adds an entry

## Sister-fix-CB3 relationship

Sister-fix-CB3 (default-expanded Discussion Mode when assembly active) is an acute fix shipping in parallel with this spec. Its purpose: restore the lost-mic-passing-UI regression human flagged in msg 5567/5568 immediately, since v2 chain takes ~1-2 hours. Once v2 Commit 7 (Discussion Mode band deletion) lands, CB3 is moot — the band it modifies no longer exists. This is acceptable.

## Cross-references

- Path B `persistedState.ts` SHA `2fe16e8` — shared localStorage helper this spec consumes for any new persistent state
- ProjectDirContext SHA `8162d3f` — pattern this spec's ProtocolStateContext mirrors
- Keepalive v3 SHA `cd1b629` — alive-state visualization this spec composes with on role cards
- Sister-fix-CB3 (pending dev:1 ship) — bridging fix that v2's Commit 7 supersedes
- Layout-density-v1.2 SHA `c115441` + `795db42` — collapsible-header design-system primitive that v2 keeps but reduces in scope for the Discussion Mode band
