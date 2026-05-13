# Phase Pill — UI-Craft Lens Pre-Review

Author: ui-architect:0
Date: 2026-05-13
Reviewing: `.vaak/design-notes/phase-pill-spec-2026-05-13.md` (revision eb45754)
Status: pre-review notes for fold-in only IF the parking on activity-field-observation lifts. Does NOT lift parking. UX-engineer:0 retains spec ownership.

## Scope of this review

UX's spec is sound on coordination semantics, mutation rules, and observe-first cadence. That's not what I'm reviewing. This is the UI-craft lens — visual treatment, header real estate, accessibility, mobile/narrow-window handling, and conflict with adjacent UI surfaces. Nothing here blocks the spec; all of it is intended to be folded into Surface and Open Questions sections if/when the spec unparks.

## Color choices for the 5 phase values

Spec lists slate blue / green / amber / purple / pink for discuss / implement / review / test / feedback. The vibe is right but the spec doesn't yet specify hex values or foreground contrast. Three constraints to bake in before any visual mockup:

1. **WCAG AA contrast against `--bg-primary` (#0a0a0b in V1, #ffffff in V2).** Pill foreground text needs ≥4.5:1 against pill background; pill background needs to read clearly against the page background. The five colors selected are saturated mid-tones that will fail one or the other on a dark theme without tuning. Concrete proposal: pill background = saturated 500-shade, pill text = 50-shade (very pale tint of the same hue), borders = 600-shade. Worked example with the existing `--accent: #6366f1`: background `#6366f1`, text `#eef2ff`, border `#4f46e5`. That contrast ratio is 6.5:1 on text, 8.3:1 of pill-vs-bg. Apply the same recipe to slate blue / green / amber / purple / pink before committing colors.

2. **Saturation parity across the 5.** If amber pops harder than slate blue, the phase reads as a priority indicator (amber == warning) rather than a peer of the other phases. All 5 should feel visually equal-weight. Probably means desaturating amber and brightening slate blue.

3. **Reuse existing tokens where possible.** `--success: #22c55e` already exists for green; reuse for implement. `--warning: #f59e0b` already exists for amber; reuse for review (or rename the token if review-mode is the dominant amber use). Don't introduce 5 new colors when 2-3 can be tokenized into the design-tokens spec (dbc51f8) and shared.

## Header real estate competition

Spec puts the pill in the Collab tab header. Current header from CollabTab.tsx already contains: section picker / workflow type chooser / Assembly Line toggle / connect status / discussion mode dropdown / role-color customizer trigger. Adding the phase pill makes 7 controls in the header on V1. UX audit msg 264 already flagged workflow types as a candidate collision with the phase pill.

Two design directions:

A. **Phase pill subsumes the workflow type chooser.** Phase semantics (discuss/implement/review/test/feedback) overlap meaningfully with workflow types (Full Review/Quick Feature/Bug Fix) — both encode "what kind of work are we doing." Subsuming workflow into phase removes a control instead of adding one. Probably needs a workflow→phase migration in protocol.json schema.

B. **Phase pill is its own surface, workflow chooser stays.** Spec-as-written, but document explicitly that phase and workflow are orthogonal concepts (one is team-mode, the other is feature-shape) and design their visual treatment to not read as duplicate signals.

I prefer A for craft reasons — fewer controls = clearer hierarchy = lower cognitive load. UX is the right owner of the call though, since it's a product-shape decision.

## Dropdown UX

Spec says click pill → dropdown with 5 options → click option → phase changes. One-click change is right (no confirm). Three craft details:

1. **Current phase indicator inside the dropdown.** Highlight the currently-active phase with a checkmark or filled radio so the user doesn't have to read the pill to know which is selected before changing.

2. **Hover-to-preview behavior.** Optional but valuable: hovering over an option in the dropdown could briefly tint the header to that phase's color so the user previews the visual impact before clicking. Reduces accidental selections.

3. **Keyboard navigation.** Arrow keys + Enter to select, Esc to close, Tab to escape the dropdown. ARIA role `menu` / `menuitem`. Currently no other dropdown in CollabTab implements this consistently — opportunity to set the pattern as the design-tokens spec adopts.

## [YOUR TURN] body inline

Spec proposes a `Phase:` line in [YOUR TURN] body alongside Rotation. Post-be2b28d body-reframe (v1.0.3), [YOUR TURN] dropped the Ask/Expected lines by default. Adding Phase: re-introduces a server-injected line. Worth confirming with the human msg 411 directive that team-level signal lines (phase) are acceptable to inject, even when speaker-specific lines (ask/expected) aren't. The distinction: phase is environmental water-pressure; ask/expected is speaker-pre-framing. The former is what the human asked for (school-of-fish); the latter is what they objected to.

## Mobile and narrow-window handling

V1 desktop UI is dense; narrow window collapses some header controls already. Phase pill should follow the same collapse pattern: at narrow width, show only the colored dot + abbreviated label ("Imp" instead of "PHASE: implement"); at tooltip-or-tap, expand to full label. The five phase labels are 7-9 characters each — adding "PHASE:" prefix is 14-17 chars per pill, eating header real estate. Drop the prefix; the colored dot + label is sufficient signal.

## Accessibility

- `aria-label` on the pill button explaining current phase + the action ("Phase: discuss. Click to change.").
- `aria-haspopup="menu"` and `aria-expanded` for the dropdown.
- Color is not the only signal — pair each phase with a small icon (talk bubble for discuss, hammer for implement, eye for review, beaker for test, megaphone for feedback) so color-blind users get the same signal.
- `prefers-reduced-motion`: phase transition (color change in the rotation header, possibly the pill itself) should respect the user preference. Currently CollabTab has only 1 `prefers-reduced-motion` hit in collab.css; the pill addition is a chance to extend coverage.

## Conflict with rotation-with-activity weave (84f6c15)

Per UX msg 264, the rotation header already weaves activity-per-role: `architect:0(discussing) → developer:0(NOW, implementing) → ux-engineer:0(idle)`. Adding `PHASE: implement` to the same header creates ambiguity: is "discussing" in the activity tag the role's per-role activity, OR a hint that the team should be in `phase: discuss`? Without clear visual separation the two collapse into noise.

Concrete fix: visually segregate. Phase pill on the LEFT of the rotation header, separated by a vertical divider. Per-role activity tags stay inside parentheses next to each seat. Two distinct visual treatments — pill (saturated bg) vs. parenthetical (muted text). Reduces cross-talk.

## V2 considerations

`CollaborateV2/CollaborateV2App.tsx` has its own header in light theme. Phase pill specs need to either:

1. Define a parallel light-theme color set that maintains the same WCAG contrast on a light background, OR
2. Defer the V2 pill implementation until V2 reaches the phase-relevant slice (P3a Assembly Line UI), at which point V2 designs its own pill from scratch using the design-tokens spec scales.

I prefer (2). V2 is the long-term direction; V1 pill is a stopgap for the next 36h-or-thereabouts. Defer V2's pill until V2 owns its header design rather than retrofitting V1's pill onto V2.

## Recommendations summary (for fold-in if parking lifts)

1. Specify exact hex values per phase color with WCAG AA contrast verified.
2. Reuse `--success` and `--warning` tokens where they fit.
3. Resolve workflow-vs-phase overlap by either subsuming or visually-segregating; my preference is subsuming.
4. Specify dropdown keyboard navigation + ARIA roles.
5. Drop the `PHASE:` prefix; colored dot + label is sufficient.
6. Pair each phase with an icon for color-blind users.
7. Respect `prefers-reduced-motion` on phase transitions.
8. Visually segregate the pill from the rotation header's per-role activity tags.
9. Defer V2 phase pill until V2's header design slice (P3a or later).

None of these block the spec's coordination semantics; all of them are visual/accessibility polish that lands cleanly into a craft-quality implementation when/if the parking lifts.

## What's NOT in scope of this review

- The spec's coordination semantics (UX-lane, sound as written).
- The 5-vs-fewer-phases question (UX/architect open question, already named).
- AI phase-change proposals during autonomous-run (UX-lane, addressed in revision eb45754).
- Spec-vs-code reconciliation for body reframe (already shipped as v1.0.3 be2b28d).

## Out of scope

- Implementation. Spec stays parked.
- New design-tokens for phase colors — those go into the design-tokens spec dbc51f8 once we agree on the 5 hexes.
- V2 implementation — deferred to V2's own slice.
