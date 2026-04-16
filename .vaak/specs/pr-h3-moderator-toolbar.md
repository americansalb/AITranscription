# PR H3 — Moderator Toolbar

**Status**: spec draft, blocked on PR 4 (`ModeratorError` enum + capability list exposed to frontend)
**Owner**: ux-engineer
**Reviewers**: developer (wiring), dev-challenger (privilege concentration), tester (ARIA + reduced-motion)
**Estimated**: ~1 day once PR 4 unblocks
**Depends on**: `b2c29d8` (PR M), PR 4 (`ModeratorError::CapabilityNotSupportedForFormat`)

## Goal

Surface moderator/manager privileges as a visible, accessible toolbar in CollabTab. Today those capabilities exist in `collab.rs` but the user has no UI affordance — moderator commands require calling MCP tools by name. H3 makes the privilege visible and usable from the desktop app.

## Scope

**In scope (v1):**
1. Reorder pipeline — drag-handle on role cards in pipeline progress bar, moderator-only, audit reason optional
2. Jump to stage — dropdown, moderator-only, **mandatory reason** (≥3 chars)
3. Pause / Resume — single toggle button, moderator-only
4. End session — red button with confirmation modal, **mandatory reason**
5. Speak out of turn — purple compose overlay, distinct visual in thread
6. @human bridge — dedicated "To human (direct)" compose target for moderator + manager only

**Out of scope (v2+):**
- Multi-moderator coordination (single moderator seat per session assumed)
- Undo/rewind after End Session
- Reorder history visualization
- Moderator voice commands

## Data contract (from PR 4)

Requires these to exist in the frontend TypeScript types (will be added when PR 4 ships):

```typescript
type ModeratorCapability =
  | "reorder_pipeline"
  | "jump_to_stage"
  | "pause_session"
  | "resume_session"
  | "end_session"
  | "speak_out_of_turn"
  | "direct_message_human";

type ModeratorError =
  | { variant: "ReasonRequired"; action: string; min_length: number }
  | { variant: "CapabilityNotSupportedForFormat"; capability: ModeratorCapability; format: string }
  | { variant: "NotAuthorized"; role: string; required: string };
```

## UI layout

New `.moderator-toolbar` strip:
- **Location**: below the session header, above the role-card grid
- **Visibility**: `viewerRole === "moderator" || viewerRole === "manager"` — hidden for all other roles and when no session is active
- **Disabled state**: format-gated controls show grayed with tooltip "Only available in Pipeline mode" when viewer is privileged but session format doesn't support the action

## Per-control spec

### Reorder pipeline
- Drag handles appear on role cards during active pipeline (only for viewer with `reorder_pipeline` capability)
- Dropping opens a small modal: optional `reason` text field
- On confirm, calls `pipeline_reorder(new_order, reason?)` via MCP
- Audit emitted: `moderator_action: { action: "reorder_pipeline", reason, timestamp, actor, affected_role: null }`

### Jump to stage
- Dropdown labeled "Jump to…" shows current pipeline order
- On select, opens modal with **required** `reason` field (min 3 chars)
- Confirm disabled until reason valid
- Calls `pipeline_jump_to_stage(role_instance, reason)` via MCP
- Audit: `{ action: "jump_to_stage", reason, timestamp, actor, affected_role: skipped_roles[] }`

### Pause / Resume
- Single toggle button, icon switches between ⏸ and ▶
- Paused state: yellow banner across session header "Session paused by moderator:0 at 12:34"
- Auto-termination does NOT fire while paused (PR 4 behavior)
- No reason required

### End session
- Red "End Session" button
- Click opens confirmation modal: **required** `reason` field + explicit "type END to confirm" input
- On confirm, calls `end_session(reason)` via MCP
- Modal has Cancel button with focus trap (WCAG 2.1 AA)
- Audit: `{ action: "end_session", reason, timestamp, actor, affected_role: null }`

### Speak out of turn
- Purple "Moderator says…" compose button
- Opens inline compose overlay with existing compose bar styling + purple accent
- Message rendered in thread with purple left-border + "Moderator" badge
- Available in any format
- No `affected_role`; audit just records the speech act

### @human bridge
- Appears as a compose-target option "To human (direct)" only for moderator + manager
- Routes to human-tab filter (PR H already includes these senders)
- No modal, no reason — it's just compose-to-human

## Accessibility

- Every button has a visible text label (not icon-only). Tooltip is supplement, not substitute.
- Modals use `role="dialog"`, `aria-modal="true"`, focus trap, Escape to cancel, Enter to confirm-when-valid
- Toolbar has `role="toolbar"` with arrow-key navigation between buttons
- Pause/Resume, Jump, End Session fire `aria-live="assertive"` announcements ("Session paused", "Jumped to tester:0", "Session ended")
- Reduced-motion: banner-slide animations gated on `prefers-reduced-motion: reduce` — use opacity-only transitions

## Error handling

Per PR 4's `ModeratorError` enum:
- `ReasonRequired` → inline form error under the reason field, focus returns to the field
- `CapabilityNotSupportedForFormat` → button stays disabled with tooltip (should not reach this at runtime since UI pre-filters, but handled defensively)
- `NotAuthorized` → toast "You don't have permission to perform this action" with 5s dismiss

## Testing

Tester's matrix (owned):
- `test_moderator_toolbar_hidden_for_non_privileged_roles` — render with viewer as "developer", toolbar absent
- `test_end_session_confirmation_requires_reason_and_typed_confirm` — integration test on the modal
- `test_pause_button_toggles_and_emits_aria_live` — Playwright with axe-core
- `test_reorder_drag_emits_audit_metadata` — drag-drop with React Testing Library
- `test_keyboard_arrow_navigation_within_toolbar` — WAI-ARIA toolbar pattern
- `test_reduced_motion_disables_banner_animation` — media-query simulation

## File plan

- `desktop/src/components/ModeratorToolbar.tsx` — new component (~200 lines)
- `desktop/src/components/ModeratorToolbar.module.css` — scoped styles, or add to `collab.css` in a new section
- `desktop/src/components/CollabTab.tsx` — render the toolbar conditionally, wire capability gating
- `desktop/src/lib/collabTypes.ts` — ModeratorCapability + ModeratorError types (may land in PR 4)

## Provenance

- msg 153 (human): manager/moderator exclusive privileges
- msg 162 (human): action mode directive
- msg 166 (UX): original toolbar spec
- msg 172 (dev-challenger): tiered second-factors
- msg 175 (platform-engineer): aria-live priority split
- msg 200 (tech-leader): Moderator ≠ Manager keep-separate
- msg 291 (tech-leader): PR H2 authorization + H3 spec directive
- msg 306 (tech-leader): H3 hold-until-PR-4 + spec-while-idle instruction
