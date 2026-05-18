# CollabTab Restructure v1 — Spec

Owner: architect:0
Date locked: 2026-05-18
Trigger: human msg 5237 (UI consolidation directive, voice-dictated)
Amendments folded: 12 across adversarial-rigor + craft-lane reviews

## Problem statement (from human msg 5237)

The CollabTab UI has accumulated stacked controls that crowd the message timeline (the primary artifact the human reads). Specifically:
- Section strip is too tall and not collapsible
- Section selector lives far from project name
- Manage Roles + Team Roster are split across separate surfaces
- Assembly Line / discussion-mode controls take ~1/4 of vertical space and cannot be collapsed
- Several controls are unused: `auto` checkbox, `review` checkbox, `open` button, `discuss` button
- Text input is smaller than it should be

Goal: maximize message-timeline real-estate by consolidating everything else into tabs and collapsible cards. The collapsible-header design-system primitive emerged earlier this session (Team Roster `1c5678d`, Decision Panel + Active Claims via `c115441` + `795db42`); this restructure extends it to two more surfaces and unifies the access pattern.

## Final architecture

Top-to-bottom layout after restructure:

```
┌─────────────────────────────────────────────┐
│ Project name + [Section: name ▼]            │  ← header strip
├─────────────────────────────────────────────┤
│ ▼ Discussion Mode: Assembly Line            │  ← collapsible card
│   (mode-specific controls)                  │
├─────────────────────────────────────────────┤
│ ▼ Team [Team Roster | Manage Roles]         │  ← collapsible card with two-tab toggle
│   (active tab content)                      │
├─────────────────────────────────────────────┤
│ ▼ Decisions (N pending)                     │  ← existing collapsible (c115441)
├─────────────────────────────────────────────┤
│ ▼ Active Claims (N)                         │  ← existing collapsible (c115441 + 795db42)
├─────────────────────────────────────────────┤
│                                             │
│   MESSAGE TIMELINE (claims majority space)  │
│                                             │
├─────────────────────────────────────────────┤
│ [TEXT INPUT — slightly larger]              │
└─────────────────────────────────────────────┘
```

## Four-commit sequence

### 1. Pre-req: extract `<CollapsibleSection>` + `ProjectDirContext`

**Scope:** ~200-300 LOC TSX + ~50 CSS, single commit.

**`desktop/src/components/CollapsibleSection.tsx`** — wrapper component absorbing the duplicated pattern from `1c5678d` Team Roster + `c115441` Decision Panel + Active Claims. Contract (F-UIA-CTR-3):
- 200ms ease-out chevron-rotation animation on collapse/expand
- Border-bottom on header ONLY when expanded (collapsed cards visually merge into one strip)
- Heading hierarchy: h1 (project name), h3 (card titles), h4 (within-card labels); h2 reserved/skip
- Default state derives from content presence (`collapsed when empty`, `expanded when populated`)
- `role="button"` + `tabIndex={0}` + `aria-expanded` + `aria-controls` + `onKeyDown` (Enter/Space)
- Persist collapsed state via `persistedState.ts` (`loadJSON`/`saveJSON`) — one key per consumer

**`desktop/src/contexts/ProjectDirContext.tsx`** — React Context owning the persisted-localStorage-bound state for `vaak_collab_project_dir`. Contract (F-UIA-CTR-6):
- Exposes ONLY persistence-bound state (project_dir); per-mount UI state (scroll, search, expand-tree) remains in component-local `useState`
- Single writer to localStorage through the context setter (closes F-EA-CTR-A divergent-WRITER class for `vaak_collab_project_dir`)
- Memoized context value via `useMemo` + `useCallback` on setter to prevent re-render cascade across all `<CollapsibleSection>`-wrapped surfaces (F-EA-CTR-B mitigation per dev:1 msg 5271 choice)
- TypeScript strict-mode compliance: explicit context value type, no `as any`, throw-if-no-provider default OR safe fallback (F-EA-CTR-C)

**Migration in same commit:**
- Roster section (CollabTab.tsx ~line 4153) consumes `<CollapsibleSection>`
- Claims section (CollabTab.tsx ~line 4488-4504) consumes `<CollapsibleSection>`
- Decision Panel (DecisionPanel.tsx ~line 322) consumes `<CollapsibleSection>` — replaces hand-rolled header from `c115441`
- RolesTab (`desktop/src/components/RolesTab.tsx`) consumes `useProjectDir()` hook
- CollabTab `persistDir` migrates to consume the context setter, not raw `saveJSON`

**Gate-2 verification grep contract**: `grep "vaak_collab_project_dir" desktop/src` should return matches ONLY in `ProjectDirContext.tsx` + the `persistedState.ts` call within it; ZERO matches in consumer components.

### 2. Change B: Discussion Mode card

**Scope:** ~70-100 LOC TSX + ~40 CSS, single commit.

New `desktop/src/components/DiscussionModeCard.tsx` (or extend AssemblyControls). Card structure (F-UIA-CTR-1 + F-UIA-CTR-4):
- Always-rendered card with title "Discussion Mode: Assembly Line" (when mode active) OR "Discussion Mode: None" (when no mode active)
- **NO dropdown in v1** — single mode today; dropdown earns its place when 2nd mode (Oxford or other) ships
- When `discussion_mode=none`: card shows "Discussion Mode: None" + single "Start Assembly Line" CTA button + collapsed by default (Path A from F-UIA-CTR-4)
- When `discussion_mode=Assembly Line` (or any future mode): card shows mode-specific controls inside, expanded by default
- Wrapped in `<CollapsibleSection>` per pre-req contract
- Persist `vaak_collab_discussion_mode_collapsed` via `persistedState.ts`

### 3. Change C: Team Section with two tabs

**Scope:** ~100-150 LOC TSX + ~40 CSS, single commit.

New `desktop/src/components/TeamSection.tsx`. Structure (F-UIA-CTR-2 + F-DC-CTR-2):
- Wrapped in `<CollapsibleSection>` per pre-req contract
- Tab strip at top: `[Team Roster | Manage Roles]` (Team Roster default-active)
- Tab 1 "Team Roster": current roster grid + chip view (existing UX preserved)
- Tab 2 "Manage Roles": embedded RolesTab content via `useProjectDir()` hook
- Persist active-tab + collapsed-state as separate keys
- Standalone top-level RolesTab tab PRESERVED — embedded tab is an ADDITIONAL access path, not a replacement (F-DC-CTR-2 + human msg 5125 principle)

### 4. Changes A + D + E: header strip + useless-controls deletion + text input

**Scope:** ~60-90 LOC TSX + ~45 CSS, single commit.

**Change A (F-UIA-CTR-5):**
- Project name LEFT, bold, h1 weight, primary identity
- Section selector RIGHT-aligned, smaller font, "Section:" prefix label, dropdown caret
- Border-bottom on the strip to demarcate from Discussion Mode card below

**Change D (F-DC-CTR-3):**
- Identify + remove `auto` checkbox, `review` checkbox, `open` button, `discuss` button (final list pending human Q1 answer)
- BEFORE removing each, grep the codebase for handler/state/route references. If >0 cross-component references, surface to human before cut

**Change E:**
- Adjust text input row min-height + textarea rows for slightly larger composition area

## Amendment trail (12 items, all accepted)

| ID | Source | Topic |
|---|---|---|
| F-DC-CTR-1 | dev-challenger:0 msg 5244 | No placeholder modes; ship single-entry dropdown only when needed |
| F-DC-CTR-2 | dev-challenger:0 msg 5244 | RolesTab dual-path preserved (standalone + embedded) |
| F-DC-CTR-3 | dev-challenger:0 msg 5244 | Grep-before-delete on useless-controls cuts |
| F-EA-CTR-A | evil-architect:0 msg 5246 | Lift `vaak_collab_project_dir` state to React Context (closes divergent-WRITER class) |
| F-UIA-CTR-1 | ui-architect:1 msg 5257 | Drop the single-item dropdown entirely; just card title in v1 |
| F-UIA-CTR-2 | ui-architect:1 msg 5257 | Team Roster tab first, Manage Roles second |
| F-UIA-CTR-3 | ui-architect:1 msg 5257 | CollapsibleSection contract rules (chevron animation, border-bottom-when-expanded, heading hierarchy, default-by-content) |
| F-UIA-CTR-4 | ui-architect:1 msg 5257 | Discussion Mode empty state: "None" + CTA, not hidden |
| F-UIA-CTR-5 | ui-architect:1 msg 5257 | Header strip composition (project name left + section selector right with prefix label) |
| F-UIA-CTR-6 | ui-architect:1 msg 5257 | ProjectDirContext exposes ONLY persisted state; UI state per-mount |
| F-EA-CTR-B | evil-architect:0 msg 5265 | Re-render cascade mitigation (memo or split context) |
| F-EA-CTR-C | evil-architect:0 msg 5265 | TypeScript strict-mode preservation in new types |

## Open question pending human

**Q1**: Which specific controls are the "useless" ones to delete? Architect's read: `auto` checkbox + `review` checkbox + `open` button + `discuss` button. Human direct answer needed before Change D cut.

## Three-gate per commit

Per Ruling 13. Gate #1 (tester:0) verifies code-correctness + grep-symmetry. Gate #2 (dev-challenger:0 + evil-architect:0) verifies class-of-bug closure + adversarial flags. Gate #3 (ui-architect:1) verifies visual craft + cross-surface consistency.

## Cross-references

- Path B `persistedState.ts` SHA `2fe16e8` — localStorage helper this spec consumes
- Decision Panel + Active Claims `c115441` + `795db42` — prior collapsible-header pattern this spec generalizes
- Team Roster `1c5678d` — earliest collapsible precedent
- Keepalive chain (533b458 + 9d1fde1 + cd1b629 + c4e31c1 + d2b509f) — independent infrastructure; restructure does not touch it
- Vision.md sections: §Path B RATIFIED + §Layout-density-v1.2 RATIFIED + §Architectural lesson reframed
