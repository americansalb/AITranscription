# Vaaklite UI Craft Contract — 2026-05-19

Owner: ui-architect:1
Drives: developer:1 implementation lane during the human msg 5730 autonomous 40h sprint
Spec lock: architect msg 5738 + dev-challenger msg 5731 6-flag resolution

This document defines the visual craft conventions for the Vaaklite v1 webservice. It binds the web-client/ implementation so dev:1 can ship UI work without per-commit craft consultation.

## 1. Foundation: existing tokens (REUSE, do not reinvent)

The existing `web-client/src/styles/tokens.css` is the single source of truth for color/spacing/typography. Vaaklite v1 MUST use the tokens defined there, not introduce new hardcoded values.

Reaffirmed tokens (excerpt):
- Spacing: `--space-{1,2,3,4,5,6,8,10,12}` (4px base ladder)
- Typography: `--text-{xs,sm,base,md,lg,xl,2xl}` (11/13/14/16/18/22/28)
- Color: `--bg-{primary,secondary,tertiary,elevated}` + `--text-{primary,secondary,muted}` + `--accent` + semantic (`--success`, `--error`, `--warning`)
- Border radius: `--radius-{sm,md,lg,full}` (6/10/16/9999)
- Shadows: `--shadow-{sm,md,lg}`
- Transitions: `--transition-{fast,normal,slow}` (100/200/300ms)
- Z-index: `--z-{base,dropdown,sticky,modal-backdrop,modal,toast}` (0/100/200/300/400/500)

Dark-mode-first; light-mode override at `:root.light`. System-preference fallback already wired.

## 2. Layout: workspace primary, assembly secondary

Vaaklite's primary surface is the document workspace. Assembly state (whose turn / rotation / phase) is a secondary surface that overlays without dominating.

```
┌─────────────────────────────────────────────────────────────────┐
│ Header strip: project name · session selector · user menu        │ ← 48px
├──────────┬──────────────────────────────────────────────────────┤
│ Sidebar  │ Document Workspace (primary)                          │
│ (240px)  │                                                       │
│          │ ┌─ Section 1 ───────────────────────────────────────┐│
│ - Roles  │ │ Section title · status pill · current author      ││
│ - Roster │ │ <markdown editor / preview>                        ││
│ - Phases │ └────────────────────────────────────────────────────┘│
│ - Files  │ ┌─ Section 2 ───────────────────────────────────────┐│
│          │ │ ...                                                ││
│          │ └────────────────────────────────────────────────────┘│
├──────────┴──────────────────────────────────────────────────────┤
│ Status strip: phase · whose turn · pending decisions count        │ ← 32px
└─────────────────────────────────────────────────────────────────┘
```

Layout primitives:
- Header strip: `--space-2 --space-4` padding, `--bg-secondary`, 1px bottom border `--border`
- Sidebar: 240px fixed width, `--bg-secondary`, scrollable internal
- Workspace: flex-1, `--bg-primary`, scrollable, max-content-width 880px centered
- Status strip: `--space-1 --space-4` padding, `--bg-tertiary`, 1px top border `--border`, `--text-xs` font

## 3. Typography hierarchy

| Level | Token | Use |
|---|---|---|
| h1 | `--text-2xl / --weight-bold / --line-tight` | Project title in header strip |
| h2 | `--text-xl / --weight-semibold / --line-tight` | Document title at top of workspace |
| h3 | `--text-lg / --weight-semibold / --line-normal` | Section titles inside document |
| h4 | `--text-md / --weight-semibold / --line-normal` | Sidebar group headers ("Roles", "Roster", etc.) |
| body | `--text-base / --weight-normal / --line-normal` | Default body, editor content, prose |
| label | `--text-sm / --weight-medium / --line-normal` | Form labels, button text, chip labels |
| caption | `--text-xs / --weight-normal / --line-normal` | Timestamps, secondary metadata, status strip |

No skipped levels. No competing weights at the same size. Markdown editor + preview body shares the body token (14px/1.5).

## 4. Color application by surface

Apply tokens consistently:

- **Bg layers**: workspace = `--bg-primary`, sidebar/header = `--bg-secondary`, cards inside workspace = `--bg-tertiary`, modals = `--bg-elevated`
- **Text layers**: titles = `--text-primary`, body = `--text-primary`, metadata = `--text-secondary`, captions/disabled = `--text-muted`
- **Borders**: dividers + card edges = `--border`, hover state = `--border-hover`
- **Accent**: primary actions (Save, Submit, Take My Turn) + active states + focus rings
- **Semantic**: success (drafted+approved), warning (revision requested), error (failed/rejected)
- **Role colors**: chips/avatars use `--role-*` tokens from existing palette. Vaaklite-specific roles (writer/reviewer/moderator/audience) map: moderator→`--role-manager`, writer→`--role-developer`, reviewer→`--role-tester`, audience→`--role-user`.

## 5. Component patterns (no new primitives)

REUSE existing web-client components where possible. If a component doesn't exist, build it per these patterns:

### 5.1 Buttons

- Primary: `--bg: --accent`, `--text: --bg-primary`, hover `--bg: --accent-hover`, focus-visible: 2px `--accent` outline + 1px offset
- Secondary: transparent bg, `--text: --text-primary`, 1px `--border` border, hover `--bg: --bg-hover`
- Ghost: transparent bg + transparent border, `--text: --text-secondary`, hover `--text: --text-primary` + `--bg: --bg-hover`
- Destructive: `--bg: --error`, `--text: #fff`, hover slightly darker
- Padding: `--space-2 --space-3` (small), `--space-3 --space-4` (default), `--space-3 --space-5` (large)
- Border-radius: `--radius-sm`
- Disabled: 0.5 opacity, `cursor: not-allowed`

### 5.2 Inputs (text, textarea, select)

- Bg: `--bg-tertiary`
- Border: 1px `--border`
- Padding: `--space-2 --space-3`
- Border-radius: `--radius-sm`
- Focus: 2px `--accent` outline + 1px offset (matches button focus contract)
- Disabled: 0.5 opacity
- Placeholder: `--text-muted`

### 5.3 Cards

- Bg: `--bg-tertiary`
- Border: 1px `--border`
- Border-radius: `--radius-md`
- Padding: `--space-4` (default) or `--space-5` (large)
- Hover (if interactive): `--bg-hover` background overlay + `--border-hover` border
- Section title (h3): `--space-2` bottom margin

### 5.4 Modals

- Backdrop: `rgba(0,0,0,0.5)` + `--z-modal-backdrop`
- Container: `--bg-elevated` bg, `--border-radius: --radius-lg`, `--shadow-lg`, `--z-modal`
- Max-width: 560px (small) / 720px (default) / 920px (large)
- Header: `--space-4` padding, 1px bottom `--border`, title h3
- Body: `--space-4` padding, scrollable if content overflows
- Footer: `--space-3 --space-4` padding, 1px top `--border`, right-aligned action buttons
- Close button (×): top-right of header, `--text-secondary`, hover `--text-primary`

### 5.5 Chips / pills (role badges, status indicators)

- Padding: `--space-1 --space-2`
- Font: `--text-xs / --weight-medium`
- Border-radius: `--radius-full`
- Variants: role-color background at 0.15 alpha + role-color text; OR semantic (success/warning/error) at same alpha pattern
- Inline-flex; no margin; flex-shrink: 0

### 5.6 Tabs

- Strip border-bottom: 1px `--border`
- Tab padding: `--space-2 --space-3`
- Font: `--text-sm / --weight-semibold`
- Inactive: `--text-secondary`, border-bottom 2px transparent
- Hover: `--text-primary`
- Active: `--text-primary`, border-bottom 2px `--accent`
- Focus-visible: 2px `--accent` outline + -2px offset
- Negative margin trick on active border to prevent layout shift

## 6. Document workspace specifics

The workspace is the heart of Vaaklite. Section drafting is the primary action loop.

### 6.1 Section card

Each document section renders as a card with:
- **Header strip** (top of card): section title + status pill + current author chip + section number
- **Body** (markdown editor OR rendered markdown view): height auto-grows with content; scrollable if exceeds viewport
- **Footer** (bottom of card): "Take my turn" button (when it's the user/role's turn) + revision history link

Section card states:
- **Drafting**: accent left-edge 3px border, status pill "DRAFTING" with accent background
- **Review**: amber left-edge border, status pill "REVIEW"
- **Revision requested**: warning left-edge border, status pill "REVISION"
- **Final**: success left-edge border + slight `--bg-elevated` overlay; status pill "FINAL"

### 6.2 Section transitions (animation)

When a section's status changes, animate the left-edge border color over `--transition-normal` (200ms). Status pill cross-fades over the same duration. Card content scroll position preserved (no jump).

### 6.3 Editor (markdown)

Use a lightweight markdown editor (CodeMirror 6 or react-markdown-editor-lite). Required features:
- Live preview side-by-side OR toggle (preference: toggle to save horizontal space)
- Syntax highlighting in code blocks
- Auto-grow vertical
- Persist on every keystroke (debounced 500ms) to backend
- Conflict detection if another author edits simultaneously (warn + offer merge view)

### 6.4 Read view

When a section is FINAL or current user isn't the author, show rendered markdown (not editor). Use the project's existing markdown renderer if available; else react-markdown with shiki for code blocks. Body typography matches token system.

## 7. Sidebar specifics

Four sub-sections, each collapsible (use existing CollapsibleSection pattern from desktop if extractable, else inline):

### 7.1 Roles

- List of role cards (writer / reviewer / moderator / audience or user-defined)
- Each card: role-color avatar (initials), role title, current-active count badge
- "+ Add role" button at bottom (opens role-creation wizard)

### 7.2 Roster (sessions joined)

- List of active role-instances in the current session
- Each row: avatar + name + status dot (online/idle/offline)
- Mic-holder = strong accent border + 🎙 glyph (reuse F-UIA-CTR-V2-VIS1 pattern from desktop)
- Sorted by rotation order during active session (per F-UIA-CTR-V2-VIS2)

### 7.3 Phases

- Vertical timeline view: Drafting → Review → Revision → Final
- Current phase highlighted with accent border + filled circle
- Past phases muted with check marks
- Future phases as empty circles

### 7.4 Files

- List of attached files for the session (optional v1)
- Defer to v1.1 if Hour budget tightens

## 8. Status strip specifics

Bottom of viewport, always visible. Three regions:
- **Left**: Current phase + section indicator ("Drafting · Section 3 of 8")
- **Center**: Whose turn (avatar + role name + "your turn" badge if applicable)
- **Right**: Pending decisions count + bell icon (click → opens Decision Panel from desktop, ported to web)

Status strip is 32px tall, `--bg-tertiary`, `--text-xs / --text-secondary`. Subtle separator (1px top `--border`).

## 9. Onboarding / empty states

First-time-user onboarding: NO modal overlay (per [[feedback_no_kick_without_explicit_auth]]-style minimal-friction discipline). Instead, the workspace shows an EMPTY-STATE card with:
- h2: "Start your first discussion"
- body: brief instructions (3 lines max)
- Primary button: "Create new project"
- Secondary link: "Or browse existing sessions"

Empty-state cards in sidebar (no roles yet, no sessions yet, etc.): use `--bg-tertiary` card with dashed border, italic `--text-muted` text, single CTA.

## 10. Focus + accessibility discipline

WCAG 2.1 AA enforced. From existing web-client conventions per [[project_web_client_built_feb24_2026]] (codebase-map):
- Focus traps on modals
- ARIA roles + labels on all interactive elements
- Reduced-motion respect (`prefers-reduced-motion` → disable transitions)
- Focus-on-navigate (route changes move focus to main content)
- Contrast ratios verified (--text-muted at 5.2:1 on --bg-primary already documented in tokens.css)

Cross-affordance focus pattern: 2px `--accent` outline + 1px or -1px offset depending on whether the element has its own border. Same pattern as desktop's F-UIA-CTR-3 contract.

## 11. Cross-affordance motion parity

All transitions use the token system:
- Hover: `--transition-fast` (100ms)
- State changes (open/close, active toggle): `--transition-normal` (200ms)
- Layout transitions (collapse expansion): `--transition-slow` (300ms)

Match desktop's F-UIA-CTR-3 200ms ease-out convention for all "feels like a click" affordances. Reduced-motion preference disables transitions entirely.

## 12. What NOT to do

- ❌ No new color values hardcoded — use tokens
- ❌ No Tailwind / no utility classes — pure CSS via tokens + component classes
- ❌ No magic spacing numbers — use `--space-*` ladder
- ❌ No skipped heading levels (h1 → h3 with no h2)
- ❌ No emoji-heavy UI (decorations OK; functional indicators should be SVG or text-based for accessibility)
- ❌ No animation > 400ms on layout changes (jank risk)
- ❌ No popup that blocks the entire viewport unless it's a true modal (focus trap + escape + backdrop)
- ❌ No dual-surface ambiguity for the same action (per F-UIA-COMMIT5-2 lesson from desktop redesign — one canonical entry point per action)

## 13. Quality gates (proportional per F-DC-KRL2)

Per-commit craft review by ui-architect:1 will check:
1. ✅ All visual values come from `tokens.css` (grep for hardcoded #hex / px in new CSS)
2. ✅ Heading hierarchy preserved (no skipped levels)
3. ✅ Focus-visible state present on every interactive element
4. ✅ Cross-affordance pattern parity (button focus matches input focus matches tab focus)
5. ✅ Empty states designed (no "blank screen" UX for first-time users)
6. ✅ Transition timing uses tokens
7. ✅ ARIA + role discipline

Proportional verdicts:
- 1-5 LOC change: 1-sentence verdict
- 5-50 LOC: 1-paragraph verdict + at most 2 flags
- 50+ LOC: standard structured verdict per existing F-UIA-CTR-3 convention from desktop reviews

## 14. Discipline lessons folded in (from desktop session)

These lessons from the desktop session bind Vaaklite work:

- **[[feedback_audit_both_write_and_read_sides]]** — when reviewing a click handler, trace onClick → API call → backend handler → state mutation → render-gate consumer. Verify the full pipeline, not just the immediate call.
- **[[feedback_no_deletion_from_cleanup_language]]** — directives saying "clean up" or "consolidate" authorize reorganization, NOT feature deletion. Surface scope ambiguity before shipping deletions.
- **[[feedback_decision_panel_requires_to_human]]** — questions for the human use `to:"human"` channel, not broadcast. Discussion thread is for team-to-team only.
- **[[feedback_gate_must_challenge_render_guards_not_just_verify_they_exist]]** — gate review of conditional render must reason about whether the guard's RESULT is correct for ALL state combinations.
- **[[feedback_protocol_write_paths_must_populate_all_render_gate_fields]]** — when a write path is named after one concept but render-gates consume related fields, the write must populate ALL fields readers expect.

## 15. Out of scope for Vaaklite v1

- AI agent runtime integration (will be added in v1.1 per architect msg 5738 Hour-budget — agents may be mock for v1 smoke)
- Voice/audio (ElevenLabs integration from [[project_voice_integration_future.md]])
- Realtime collaborative editing (Yjs / OT) — sequential drafting only for v1
- Mobile responsive layout — desktop browser primary, mobile defer to v1.1
- Internationalization — English-only v1

## 16. Open questions (resolved by architect-lane defaults; no human disturbance)

All ambiguity per architect msg 5738 scope lock + dev-challenger msg 5731 6-flag resolution:

| Question | Default | Source |
|---|---|---|
| Document format | Markdown | architect msg 5738 |
| Drafting unit | Section (h2 in source markdown) | architect msg 5738 |
| Assembly model | Section-rotation (one section at a time, rotate authors) | architect msg 5738 |
| Stack | Adapt existing web-service + web-client | architect msg 5738 |
| Branch target | feature/vaaklite-v1 | architect msg 5738 |
| Merge authority | Autonomous (per human msg 5730) | architect msg 5738 |

## 17. Materialization timeline

This contract is the Hour-1 deliverable for ui-architect:1 lane per architect msg 5738 sequencing:

- **Hour 1**: Contract drafted (this doc) ✓
- **Hour 2-12**: Contract is binding spec; dev:1 references it when shipping UI work. ui-architect:1 stands by for gate stamps.
- **Hour 13**: First major UI surface (sidebar + document workspace skeleton) expected to land. First gate review.
- **Hour 14-40**: Incremental gates per visible-surface commit.

Sign-off: ui-architect:1 — this contract is ready for dev:1 consumption. If dev:1 finds a scope-ambiguity not covered here, surface as a flag to architect via DM (`to:"architect"`) rather than as a public broadcast. No human disturbance per autonomous-mode discipline.
