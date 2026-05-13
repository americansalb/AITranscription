# Design Tokens Spec — Vaak Desktop UI

Author: ui-architect:0
Date: 2026-05-13
Status: draft for architect/evil-architect adversarial review; not yet authorized for implementation. **DO NOT IMPLEMENT** until reviewed and the V2-first observation cycle is agreed.

## Problem

`desktop/src/styles.css` (8042 lines) + `desktop/src/styles/collab.css` (6740 lines) declare 33 CSS custom properties total — 11 color tokens, 3 radius tokens, 5 tier tokens, no spacing scale, no type scale, no z-index scale. Grep on the same files turns up 98 unique hex colors, 104 unique pixel values, 21 distinct font-sizes, 14 distinct border-radii, 15 z-index values, 26 `!important` declarations. Token coverage is ~10% of usage. The token system is mostly absent.

`desktop/src/styles/collaborate-v2.css` (408 lines) is being authored with the same pattern — hardcoded `#1a1a1a`, `#ffffff`, `#f4f6f9`, `#c8ced6`, `15px`, `12px`, `10px` directly. V2 is the smaller surface; the gap will compound there if not addressed before more V2 work lands.

This is the CSS-layer equivalent of the multi-writer state class (see `.vaak/design-notes/multi-writer-audit-2026-05-13.md`). No single source of truth, every site picks its own value, drift is inevitable. The fix-pattern category most natural here is pattern (b) atomic source of truth — declare the scales as CSS custom properties at `:root`, then enforce their use via lint/review for V2 first, V1 sweep later.

## Proposed token tiers

### Spacing scale

```css
--space-0:  0;
--space-1:  2px;
--space-2:  4px;
--space-3:  8px;
--space-4: 12px;
--space-5: 16px;
--space-6: 24px;
--space-7: 32px;
--space-8: 48px;
```

Picked to cover the 20 most-used px values in the existing CSS (≥ 5 occurrences each per audit). Roughly halving/doubling steps — geometric scale. Any value outside the scale needs a reason; reviewer challenges it.

### Type scale

```css
--text-xs:    10px;  /* metadata, timestamps */
--text-sm:    11px;  /* secondary labels */
--text-base:  13px;  /* body */
--text-md:    14px;  /* default UI text */
--text-lg:    15px;  /* card titles */
--text-xl:    18px;  /* page headings */
--text-2xl:   22px;  /* hero */
```

Existing 21 distinct font-sizes collapsed to 7 steps that cover ≥ 95% of current usage. Sizes above the scale (24px+) are intentional hero/landing copy and stay literal.

### Z-index scale

```css
--z-base:      0;
--z-overlay: 100;  /* sticky headers, dropdowns */
--z-popover: 200;  /* tooltips, mention popovers */
--z-modal:   300;  /* dialogs, confirm-actions */
--z-toast:   400;  /* notifications (above modals) */
--z-cursor:  999;  /* drag handles, sortable previews */
```

Existing 15 z-index values are unscaled. Reading the CSS, conflicts are likely (e.g., toast might be hidden behind a modal if order accidentally inverts). Scale forces explicit layering decisions.

### Color expansion

The 11 existing color tokens cover semantic intent (bg-primary/secondary/tertiary, text-primary/secondary/muted, accent, success, error, warning, border). The 98 unique hex colors include shades and states not yet tokenized. Proposed expansion:

```css
--accent-soft:   rgba(99, 102, 241, 0.1);   /* hover ghost */
--accent-strong: #4f46e5;                    /* pressed state */
--bg-elevated:  #1c1c1f;                    /* card surface (alias of bg-tertiary, semantic) */
--bg-hover:     #2a2a2e;                    /* row hover */
--text-link:    #818cf8;                    /* alias of accent-hover, semantic */
--text-disabled:#6b7280;
--border-subtle:#1f1f23;                    /* dividers */
--border-strong:#3f3f46;                    /* card outlines */
```

Plus a parallel light-theme set if V2's intent is light (currently V2 is light, V1 is dark — split needs architect decision).

### Radius scale

Existing tokens (`--radius-sm: 6px`, `--radius-md: 10px`, `--radius-lg: 16px`) keep their values, plus:

```css
--radius-xs:   3px;   /* inline badges, chips */
--radius-pill: 999px; /* fully rounded pills (used in cv2-phase-pill) */
```

### Motion tokens

Not in scope of this spec (V1 has minimal motion). Park for a future motion-tokens spec.

## V2-first, V1 sweep later

V2 is being authored now and is 408 lines vs V1's 14,782 lines. The cost differential is ~36×. Doing tokenization in V2 first:

1. Establishes the scale in a small, scoped surface with low coupling
2. Provides a working reference for the V1 sweep
3. Sets the standard new V2 work is measured against
4. Limits cascade risk — V1 sweep is a separate slice with explicit before/after diff per file

The V1 sweep itself is multi-week, not a 36h task. Spec ships, V2 adopts in its next feature slice, V1 sweep gets prioritized post-v1.5 alongside the typed-enforcement track from the multi-writer audit (pattern c).

## Adoption strategy

1. **Phase 1 (this spec):** define the scales. No code changes. Adversarial review (architect + evil-architect). Land as docs only.
2. **Phase 2 (V2 next slice):** when V2's next surface lands (P3a Assembly Line UI per the V2 spec), it uses these tokens by default. New CSS values that don't reference a token need an explicit comment justifying the literal.
3. **Phase 3 (V1 sweep, post-v1.5):** systematic replacement across `styles.css` + `collab.css`. Probably 5-10 commits, organized by component (settings, queue, collab, etc.).
4. **Phase 4 (enforcement):** linting rule (stylelint plugin) that flags hardcoded color/spacing values outside the token set. Compiles design intent into the build pipeline.

## Open questions for architect / evil-architect

1. **Dark + light split:** V1 is dark, V2 is light. Do tokens cover both modes via `[data-theme="light"]` / `[data-theme="dark"]` selectors, or are they single-mode and the V1 / V2 stylesheets pick their own root values? Affects the color tier structure significantly.

2. **Backward-compatibility for existing variable names:** `--bg-primary` etc. are referenced across thousands of lines of V1 CSS. Keep the old names as aliases of new tokens, or break and sweep? Aliases preserve the V1 sweep being incremental; breaking forces atomic migration.

3. **`!important` legacy:** 26 `!important` declarations exist today, mostly in V1's specificity battles. Spec doesn't address them directly but a `--z-overlay` scale typically reduces the need for `!important` on layering. Worth a separate spec slice or fold in?

4. **Tier semantics for "tier" colors:** the existing `--tier-bronze` / `--tier-silver` / `--tier-gold` / `--tier-platinum` / `--tier-diamond` are gamification-specific. Keep them in the global token sheet, or move to `gamification/tokens.css` as a domain-specific scale? Same question for any future Phase pill or moderator-specific colors.

5. **Where does the token sheet live?** Single root file (`desktop/src/styles/tokens.css`)? Inline at the top of `styles.css`? Two files for dark vs light?

Architect to resolve at least #1 and #5 before any code work — they're load-bearing for the V2 adoption path.

## Why this is worth shipping

Visual consistency drift is the structural reason today's UI feels "fine, mostly fine, then suddenly wrong in one spot." Without a scale, every developer (human or AI) picks their own px/hex value. The result is the 98-colors / 104-pixels state we have today, and the eventual drift in V2 if it grows without tokens.

This is one of the cheaper fixes in the codebase — the scales themselves are ~50 lines of CSS custom properties. Adoption is incremental. The expensive part (V1 sweep) is deferred and that's fine — V2 gets the value first.

## Out of scope

- Component tokens (e.g., `--button-radius`, `--card-padding`). Those are downstream of primitive tokens — defer until the primitives are stable.
- Theming runtime switcher. Same reason.
- Motion / animation tokens.
- Icon / illustration tokens.
- Stylelint rule implementation (Phase 4 work).
