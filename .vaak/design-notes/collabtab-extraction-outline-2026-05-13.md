# CollabTab.tsx Component Extraction Outline

Author: ui-architect:0
Date: 2026-05-13
Status: design proposal for architect/UX adversarial review; not yet authorized for implementation. **DO NOT IMPLEMENT** until reviewed, the V2 work converges, and the team agrees the V1 refactor is worth the risk of touching the live monolith.

## Problem

`desktop/src/components/CollabTab.tsx` is 5087 lines. Audit-derived metrics:

- 42 top-level helper functions and inline components
- 139 hook calls (useState / useEffect / useMemo / useCallback / useRef)
- 95 `<button>` elements
- 31 `catch` blocks (now toast-surfacing post UX 8f2b97a + ui-arch sweep)
- 3 inline component definitions (`MessageTypeBadge`, `QuestionCard`, `VoteCard`)
- 3 distinct roster render paths (grid, list, chip) duplicated across two top-level render blocks

UX audit `.vaak/design-notes/collabtab-ux-audit-2026-05-13.md` flagged the file's "component-size impact": cold-load time, slow HMR, concentrated bug surface, implicit state graph. Architect msg 443 ordered this outline as the high-architectural-value next step.

CollaborateV2 is being built as the eventual replacement (`desktop/src/components/CollaborateV2/CollaborateV2App.tsx`), but V2 P1 covers shell-only and won't reach feature parity with V1 for multiple slices (P3a / P3b / P3c / P5 per the V2 spec). V1 stays load-bearing during that period. Reducing V1's monolith risk is the bridge.

## Design principles

1. **Extraction is a refactor, not a rewrite.** Behavior is preserved; structure changes.
2. **No new state.** Existing hooks move to their owning extracted module unchanged; existing handlers move to their owning extracted hook unchanged.
3. **Type-shape first.** Define module boundaries by type-shape (what data flows in, what flows out). Use that to find the actual seams in the monolith, rather than imposing an aesthetic structure.
4. **Reversible.** Each extraction lands as a single small commit that can be reverted cleanly without unwinding the next one.
5. **V2 informs V1, not the reverse.** Where V2's componentry already exists (e.g., `CollaborateV2/seatsLoader.ts`), align V1's extracted modules to V2's interfaces so the eventual cutover is friction-free.

## Candidate extractions

Listed in dependency order — earlier extractions enable later ones with minimal touch. Each names target file path, extracted scope, line range of origin code, and approximate touch cost.

### 1. `desktop/src/lib/collabRoleColors.ts`

Extract: `ROLE_COLORS` (line 19), `HASH_PALETTE` (line 29), `ROLE_ORDER` (line 54), `hashSlug` (line 44), `getRoleColor` (line 61). Plus the `customColors` parameter pattern they accept.

Cost: ~70 lines moved, plus 3-4 import sites updated in CollabTab.tsx. Zero behavior change.

Why first: pure data + pure functions, no hooks, no React state. Trivially extractable. Used downstream by other extractions.

### 2. `desktop/src/lib/workflowDisplay.ts`

Extract: `WORKFLOW_TYPES` (line 69), `getWorkflowDisplay` (line 75).

Cost: ~30 lines moved.

Why second: same shape as #1, depends on #1's `customColors` interface, enables #3 which needs `getWorkflowDisplay`.

### 3. `desktop/src/components/collab/VoteCard.tsx`

Extract: `VoteCard` inline component (line 391-454, ~60 lines), `getActiveVotes` helper (line 93, depends on `WORKFLOW_TYPES`).

Cost: ~80 lines moved.

Why third: depends on #1 (role color) + #2 (workflow display). Cleanly bounded — VoteCard takes a tally object and renders, no upward state.

### 4. `desktop/src/components/collab/QuestionCard.tsx`

Extract: `QuestionCard` inline component (line 335-390), `getAnswerForQuestion` helper (line 325).

Cost: ~70 lines moved.

Why fourth: parallel to #3. Bounded. Depends on `BoardMessage` type from `collabTypes.ts` (already extracted).

### 5. `desktop/src/components/collab/MessageTypeBadge.tsx`

Extract: `MessageTypeBadge` inline component (line 321-324, 4 lines).

Cost: ~20 lines once you add a small interface and import sites.

Why fifth: tiny, but worth its own file for V2 parity (`MessageTypeBadge` will be reused there) and so consumers can import it without pulling all of `CollabTab.tsx`.

### 6. `desktop/src/lib/rosterCards.ts`

Extract: `computeInstanceStatus` (line 177), `buildRosterCards` (line 214), `InstanceCard` type, `getStatusDotClass` (line 144), `getStatusLabel` (line 153), `sortRolesByPipeline` (line 161).

Cost: ~120 lines moved.

Why sixth: data computation only, no React state. Used by the three roster render paths. Extracting it unblocks #7.

### 7. `desktop/src/components/collab/RosterCard.tsx`

Extract: a unified roster-card render component that the three current rendering blocks at line ~3097, ~3462, plus the chip-mode branches at ~3108 and ~3482 can all collapse into.

Cost: ~150 lines moved, plus ~80 lines of consolidation at the three call sites (net reduction).

Why seventh: this is the highest UX-craft win — UX audit msg 264 flagged the duplicate roster-render code paths. One source of truth for what a roster card looks like, with view-mode (`grid` / `list` / `chip`) as a prop. Future visual changes apply uniformly.

### 8. `desktop/src/hooks/useCollabConnection.ts`

Extract: `handleConnect`, `loadPersistedDir`, `savePersistedDir`, the `watch_project_dir` invoke flow, the `unlistens` setup, the `setupListeners` boot useEffect (line ~2249).

Cost: ~200 lines moved. Touches multiple useState/useEffect calls that all serve the connection lifecycle.

Why eighth: largest single chunk. Connection state is conceptually self-contained — `projectDir`, `project`, `watching`, `errors`, `unlistens`, all related. Hook returns these as a tuple plus the imperative actions.

### 9. `desktop/src/hooks/useDiscussion.ts`

Extract: `discussionState`, `handleStartDiscussion`, `handleCloseRound`, `handleEndDiscussion`, the slash-command dispatch for `/start-discussion` / `/end-discussion` / `/close-round`, the polling useEffect at line ~1346.

Cost: ~150 lines moved.

Why ninth: discussion is a self-contained sub-feature with its own state, polling, and command surface.

### 10. `desktop/src/hooks/useRoster.ts`

Extract: `handleAddRosterSlot`, `handleRemoveRosterSlot`, `handleLaunchTeamMember`, `handleBuzz`, `handleViewAgent`, `handleSaveGroup`, `handleDeleteGroup`, `handleImportRoles`, `handleDeployGroup`.

Cost: ~250 lines moved.

Why tenth: the roster surface is the second-largest functional subsystem after the message feed. Each handler currently has its own showToast wiring (post sweep); centralizing them in a hook gives V2 a single import surface.

### 11. `desktop/src/components/collab/SectionPicker.tsx`

Extract: `handleCreateSection`, `handleSwitchSection`, the sections-list state, the section-picker JSX.

Cost: ~120 lines moved.

Why eleventh: tightly-bounded sub-feature, owns its own UI block.

### 12. `desktop/src/components/collab/MessageInput.tsx`

Extract: the message-input textarea, draft persistence (`saveDraft` / `loadDraft`), slash-command parser, send action, the mic-grab UX hint state.

Cost: ~300 lines moved (the slash-command parser is meaty).

Why twelfth: this is the message-composition surface. Extracting it cleanly separates input concerns from feed-render concerns.

## What's intentionally NOT extracted

- **Rotation header**: UX is iterating here (commit 84f6c15 + further refinements planned). Leave alone until UX-lane is quiet on this surface.
- **Workflow chooser UI block**: UX msg 264 flagged collision with phase pill. Defer until phase-pill direction is settled.
- **ProtocolPanel integration**: ProtocolPanel is already its own component (`desktop/src/components/ProtocolPanel/`). No re-extraction needed.
- **ErrorBoundary**: already extracted (`desktop/src/components/ErrorBoundary.tsx`).
- **Toast**: already extracted (`desktop/src/components/Toast.tsx`). UI-arch ToastProvider lift (bf0e1ae) finished the architectural shape.

## Estimated total post-extraction size

If all 12 extractions land: `CollabTab.tsx` drops from 5087 lines to approximately 1800-2200 lines (the residual orchestrating component). That's still big, but proportional to its top-level role and within range of standard React container component sizes.

## Risk and sequencing

This is multi-week work, not a 36h sprint. Each extraction is single-file (the new module) plus one-file edits in CollabTab.tsx — small commits, easy review-on-land, reversible.

Recommended sequencing:

- **Wave 1 (low risk):** #1 + #2 + #5. Pure data/util extractions. Land first; ~5 days at one-per-day with adversarial review.
- **Wave 2 (small components):** #3 + #4. Sub-components with bounded surface.
- **Wave 3 (high UX-craft value):** #6 + #7. Roster card consolidation — biggest visible win.
- **Wave 4 (hooks):** #8 + #9 + #10 + #11. Pull state out of the monolith.
- **Wave 5 (compositional):** #12. Last big block, lands after the rest are stable.

Each wave's last commit also runs `npm run build` to verify nothing breaks at the bundler level, plus `npx tsc --noEmit` for the type check. No new tests are required by this work; existing test files (`desktop/src/__tests__/*`) cover behaviors not changed by extraction.

## Open questions for architect / UX

1. **V1 vs V2 effort allocation.** Should waves 4-5 wait until V2 reaches feature parity at P3c, and let V2 absorb the equivalent components rather than backporting? V1 surface lives longer if V2 is delayed, so the trade-off is real.

2. **Test coverage gating.** UX audit noted zero test coverage. Should any extraction wave be gated on adding a basic test file for the extracted module, or do we accept that as "later" too?

3. **Stylesheet co-location.** When extracting `RosterCard.tsx`, do its styles move to `RosterCard.css` (CSS modules / co-located) or stay in `styles/collab.css` (current pattern)? The design-tokens spec dbc51f8 has implications here.

4. **Type-shape contracts.** Should the `BoardMessage`, `ParsedProject`, `RosterSlot`, etc. types stay in `desktop/src/lib/collabTypes.ts` or migrate to `desktop/src/components/collab/types.ts` as the components consolidate?

5. **HMR / dev-server impact.** Extracting 12 modules creates 12 hot-module boundaries instead of 1. This usually IMPROVES HMR speed but could surface stale-state issues at module boundaries. Worth a Vite config review when the first wave lands.

Architect and UX to resolve at least #1 (V1/V2 allocation) before any extraction begins — that's the load-bearing scheduling decision.

## Why this is worth shipping eventually

`CollabTab.tsx` at 5087 lines is the team's largest agent-collision risk. Multiple roles can't safely touch it simultaneously (today's claim coordination demonstrates this — UX, developer, and UI-architect have each held narrow regions). Extraction reduces collision surface by ~60-70%, makes adversarial review tractable (small files = small diffs = real review), and aligns the V1 codebase with V2's compositional style.

This is the structural fix for "the file is too big to safely change." Same architectural shape as the design-tokens spec (pattern-(b) atomic source of truth, decomposed instead of a single monolith) and the eventual typed-enforcement track (pattern-(c) when the modules become typed boundaries).

## Out of scope

- Actual implementation (this is spec only).
- New features inside the extracted modules.
- V2 work — that's separate.
- ProtocolPanel changes — already extracted.
- Styling refactor — that's the design-tokens spec dbc51f8.
- Test additions — see open question #2.
