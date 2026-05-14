# Moderation Panel — UI-Craft Lens Pre-Review

Author: ui-architect:0
Date: 2026-05-13
Reviewing: `.vaak/design-notes/moderation-panel-ux-spec-2026-05-13.md` (commit 32fbadf)
Status: pre-review notes for fold-in when the spec moves toward implementation, gated by the moderator:0 experiment per UX's acceptance plan. Does NOT lift the parking. UX-engineer:0 retains spec ownership.

## Scope of this review

UX's spec is sound on action semantics, audit events, the phase × moderation interaction, and the experiment-first cadence. That's UX-lane. This review is UI-craft only — visual treatment, header real-estate competition, accessibility, keyboard shortcuts, conflicts with adjacent surfaces. None of the recommendations block the spec; all of them are intended for fold-in when implementation begins.

## Held-since color thresholds

Spec uses green <30s, amber 30-90s, red >90s. Three concrete constraints before mockup:

1. **WCAG AA contrast.** Green / amber / red text-on-dark backgrounds: green needs to be a lighter shade (e.g., `#86efac` not `#22c55e`) to meet 4.5:1 on `--bg-primary`; the existing `--success`, `--warning`, `--error` token values may be fine for fill backgrounds but undersized for text. Picker should specify foreground/background pair per state and verify contrast.

2. **Colorblind-safe.** Green-amber-red is the most common colorblindness fail (deuteranopia, protanopia). Pair the color with a SHAPE or NUMERIC signal: solid green dot < 30s, amber outlined triangle 30-90s, red filled square >90s. Or simpler: render the held-since seconds as the primary signal (`developer:0 — 47s`) and color as secondary emphasis. Color as decoration, not as the primary signal.

3. **Don't overload the user.** A held-since counter updating at 1Hz on a always-visible panel is a steady visual change in the user's peripheral vision — distracting. Recommend rendering at 5s precision under 30s (so it flips green → amber once, not 30 times), and 10s precision after that. Use rounding (`Math.round((nowMs - heldSinceMs) / 5000) * 5`).

## Panel placement — strong recommendation for drawer (option b)

UX listed three options: (a) right rail always visible, (b) collapsible drawer adjacent to ProtocolPanel, (c) top-bar hover-expand. Strong UI-craft preference for **(b) drawer**:

- Right-rail (a) eats horizontal space permanently; the Collab tab message feed is already cramped at narrow widths. Moderation actions are intermittent — they shouldn't claim screen real estate when inactive.
- Top-bar hover-expand (c) makes the panel discoverable-only-on-hover which is bad for accessibility (keyboard users miss it) and adds latency between intent and action (hover → wait → click).
- Drawer (b) is visible-when-relevant, hidden-when-not, supports a keyboard shortcut to open, and integrates cleanly with the existing ProtocolPanel surface. Matches the established pattern.

Drawer toggle should be a button in the ProtocolPanel header — small icon (gavel or shield), aria-labeled "Moderation panel," keyboard-accessible. Drawer slides in from the right of the panel area with `transform: translateX(...)`; honor `prefers-reduced-motion` by fading instead of sliding.

## Action button visual treatment

Five buttons in a row: Skip / Grant / Pause / Warn / End. They are NOT equal-weight actions — End is destructive, Pause is high-impact, Skip/Grant/Warn are common. Visual hierarchy needed:

- **Skip, Grant, Warn**: standard button (`--accent` background, normal weight, ~32px height)
- **Pause**: tonal button (`--accent-soft` background, outlined, same height) — visually secondary, signaling "this changes the whole assembly mode"
- **End**: destructive button (`--error` background, red outline, bolder weight). Even with the "click again to confirm" pattern, the visual should match the consequence.

Don't render all five in a single visual row. Group Skip + Grant + Warn together (the three "intervene on the current speaker" actions), then a divider, then Pause + End (the two "change assembly state" actions). Clear cognitive grouping.

## Keyboard shortcuts

The spec's "ONE-CLICK semantics" needs a keyboard counterpart. Moderator users likely keep one hand on keyboard during fast-paced assemblies. Proposed shortcuts (only active when the drawer is open, focus-trapped inside):

- `S` — Skip
- `G` — Grant (opens the rotation popup, arrow keys + Enter to choose)
- `P` — Pause / Resume (toggle)
- `W` — Warn (focuses the input)
- `E` then `E` (double-press) — End (replaces the click-twice-to-confirm)
- `Esc` — close drawer, return focus to the trigger button

Document the shortcuts inline as small keycap hints inside each button label. `Skip [S]`, `Pause [P]`, etc.

## Accessibility

- `role="region"` on the panel root with `aria-label="Moderation controls"`.
- Live region for the "FLOOR HALTED" banner: `role="status"` with `aria-live="polite"` so screen readers announce the halt when it appears.
- The held-since counter should NOT be a live region — it would announce every second and drown out other speech.
- Action buttons need `aria-describedby` pointing to a hidden description of consequences: e.g., the End button reads "End assembly. Closes the rotation and stops mic-gating. Cannot be undone."
- Focus management: when the drawer opens, focus moves to the first action button (Skip). When an action completes, focus returns to the trigger button if the drawer auto-closes, or stays on the just-clicked button for chained actions.

## Conflict with adjacent surfaces

- **ProtocolPanel rotation strip** already shows current_speaker, rotation_order, mic-held timer. Don't duplicate. The moderation panel's "Rotation" field at the top can show summary-state (just current_speaker + activity) and link out to "see full rotation in panel above."
- **Phase pill** (parked) would live in the Collab tab header. Moderation panel drawer should NOT also live in the header — drawer attaches to ProtocolPanel content area, leaving header for phase + workflow + section picker.
- **Workflow chooser** in the header is the most likely visual collision if all four (phase, workflow, moderation toggle, section picker) coexist. Recommend the moderation toggle button live INSIDE ProtocolPanel, not in the Collab header, to avoid cascading header congestion.

## Reduced motion

UX spec doesn't mention motion. Recommend:
- Drawer slide-in respects `prefers-reduced-motion: reduce` → instant render, no slide.
- Held-since color transitions are crossfades (200ms) — none with reduced motion.
- Action button press feedback (subtle scale or color shift) — none with reduced motion.

## Mobile / narrow-window (UX flagged out-of-scope but worth noting)

If the desktop ever needs to function on a narrow window (split-screen mode, tablet), the drawer would push content to a single-column stack instead of side-by-side. Worth noting but agreed not load-bearing today.

## Recommendations summary (for fold-in when implementation begins)

1. Specify exact hex per held-since state with WCAG AA contrast on dark background. Pair color with shape/numeric for colorblind users.
2. Render held-since at 5-10s precision, not 1Hz, to reduce peripheral visual noise.
3. Drawer placement (option b) — toggle from ProtocolPanel header, slide-in respecting reduced-motion.
4. Visual hierarchy: Skip/Grant/Warn as standard, Pause as tonal, End as destructive. Group with a divider.
5. Keyboard shortcuts: S/G/P/W, EE for End, Esc to close. Inline keycap hints in button labels.
6. ARIA: region/label, polite live region for FLOOR HALTED, described-by for consequences. Focus management on open/close.
7. Avoid duplication with ProtocolPanel rotation strip; cross-link instead.
8. Place moderation toggle in ProtocolPanel, not Collab header, to avoid header congestion with phase + workflow + section.
9. Reduced-motion: instant render, no transitions.
10. Acceptance experiment (UX's plan) is the right validator — these UI-craft recommendations should be folded in BEFORE the controlled assembly runs so moderator:0 evaluates the right design.

## What's NOT in scope of this review

- UX's action semantics + audit events (UX-lane, sound as written).
- Q1 (human-only vs AI-delegatable) — UX/architect open question.
- Phase × moderation interaction copy (deferred to phase pill unpark).
- Acceptance test design — UX's experiment plan is correct.

## Out of scope

- Implementation. Spec stays parked behind the moderator:0 experiment.
- New design-tokens for moderator-specific colors — those fold into the design-tokens spec dbc51f8 once we agree.
- Mobile layout.
