# Typed-CSS Pattern-(c) Enforcement — v1.5 Spec

Author: ui-architect:0
Date: 2026-05-13
Status: design proposal for architect/evil-architect adversarial review; v1.5 pattern-(c) track member endorsed by architect msg 622. Sequenced AFTER the Preset enum inaugural PR demonstrates the v1.5 pattern. **DO NOT IMPLEMENT** until the design-tokens-spec (dbc51f8) unparks AND the Preset enum demonstrates the v1.5 inaugural shape.

## Purpose

Today's CSS layer has the same architectural class as today's multi-writer state bugs (`feedback_audit_both_write_and_read_sides`): the design-tokens-spec (dbc51f8) proposes a single source of truth (CSS custom properties at `:root`) but provides NO compile-time enforcement that consumers actually use them. The 98 unique hex colors and 104 unique pixel values catalogued in dbc51f8 are write-side bypass: tokens exist on the write side (`:root` declarations), but every CSS rule can bypass them on the read side by hardcoding values. The same architectural shape as today's Preset wire-format proliferation (25+ string literals where one enum would suffice), the dead read_assembly_state path (writer wrote to dead file, reader returned defaults), the rule 4 string-prefix check (writer stamped flag, reader matched prose).

This spec is the CSS-layer pattern-(c) typed enforcement: a CI-gated lint rule that disallows hardcoded color, spacing, font-size, radius, and z-index values OUTSIDE the design-tokens-spec scale. Tokens are the only legal source; bypass produces a CI failure.

## Three options considered

1. **Stylelint custom rule (RECOMMENDED).** Add stylelint to the dev-dependencies, write a custom rule that parses CSS for property values matching the disallowed primitives (hex colors not in the token set, pixel values not in the spacing scale, etc.). Runs in CI; PR fails if any disallowed value appears. No code refactor required.

2. **CSS Modules with TypeScript type generation.** Switch the 15k lines of CSS to CSS Modules, run a code generator to produce TypeScript interfaces from `tokens.css`, components import typed tokens via TS interface. Full compile-time enforcement (TS compiler errors). Requires migrating every CSS file to `.module.css` and updating every component to import classes by interface. Multi-week migration, breaks existing styling patterns.

3. **CSS-in-JS (vanilla-extract / Stitches).** TypeScript-typed CSS values, tokens are TS constants, consumers can't bypass at the type level. Full compile-time enforcement, strongest of the three. Requires rewriting all 15k lines of CSS. Breaks the project's "pure CSS, no Tailwind" convention.

Pattern-(c) strength: option 3 > option 2 > option 1. But option 1 is the only one that requires zero refactor of existing styling. Recommend option 1 for the v1.5 inaugural slice; revisit options 2/3 in v2.0 or as part of a larger styling rewrite.

## Option 1 design — stylelint custom rule

### Package additions

`desktop/package.json` devDependencies:

```json
"stylelint": "^16.10.0",
"stylelint-config-standard": "^36.0.0",
"stylelint-no-unsupported-browser-features": "^8.0.0"
```

### Configuration file

`desktop/.stylelintrc.json`:

```json
{
  "extends": ["stylelint-config-standard"],
  "plugins": ["./scripts/stylelint-vaak-tokens.js"],
  "rules": {
    "vaak-tokens/no-raw-color": "error",
    "vaak-tokens/no-raw-spacing": "error",
    "vaak-tokens/no-raw-font-size": "error",
    "vaak-tokens/no-raw-radius": "error",
    "vaak-tokens/no-raw-z-index": "error",
    "vaak-tokens/no-important-without-comment": "warn",
    "vaak-tokens/no-disable-without-justification": "error"
  }
}
```

**Corrigendum 2 (dev-challenger Finding 2):** The `no-disable-without-justification` rule above is part of the v1.5 ship — without it, the exemption mechanism described below is discipline-only and bypassable (same discipline-required failure mode the pattern-(c) work is meant to prevent). Including the rule in the config keeps the exemption mechanism enforced.

### Custom plugin shape

`desktop/scripts/stylelint-vaak-tokens.js`:

```js
const ALLOWED_HEX = new Set([
  // from design-tokens-spec-2026-05-13.md sec "Color expansion"
  // (parsed from tokens.css at lint-time so the source of truth stays single)
  // All values stored in canonical normalized form: lowercase 6-digit hex.
]);

const ALLOWED_PX = new Set([0, 2, 4, 8, 12, 16, 24, 32, 48]); // spacing scale

// Color value normalization (Corrigendum 3 — dev-challenger Finding 3):
// Before checking against ALLOWED_HEX, every parsed color value must be
// normalized to canonical lowercase 6-digit hex form. Required conversions:
//   #abc      → #aabbcc       (3-digit → 6-digit)
//   #ABCDEF   → #abcdef       (case fold)
//   rgb(170, 187, 204)     → #aabbcc   (rgb function → hex)
//   rgba(170, 187, 204, 1) → #aabbcc   (rgba with alpha=1 → hex)
//   hsl(...)               → #...      (hsl → hex via color-convert)
// Without normalization the allow-set check produces both false positives
// (#abc not in set but #aabbcc is) and false negatives (case mismatch).
// Use the `color` or `colord` npm package for canonical conversion.

// One rule per primitive. Each walks declaration values, regex-matches the
// primitive, NORMALIZES per above, checks against the allow-set. Failure
// reports file:line + the allowed alternatives.
```

Plugin reads `tokens.css` at lint-time to derive the allow-sets, so the spec
stays a single source of truth — token additions in `tokens.css` automatically
extend the allow-set without editing the plugin.

**Corrigendum 4 (dev-challenger Finding 4) — tokens.css missing/malformed
handling:** If `tokens.css` is absent or fails to parse, the plugin must fail
loudly with a named error (`VAAK_TOKENS_MISSING` or `VAAK_TOKENS_PARSE_ERROR`)
and abort the lint run. Silently treating as empty allow-set is the wrong
default — it would surface every CSS value as a violation, looking like a
regression. Failing loudly forces the maintainer to fix the token source of
truth before lint can proceed.

### CI integration

`package.json` scripts:

```json
"lint:styles": "stylelint 'src/**/*.css'",
"prebuild": "npm run lint:styles"
```

Adding to `prebuild` gates production builds. CI failures surface in the dev
loop, not just merge time.

### Exemption mechanism

Inevitable cases need raw values (one-off hero gradients, brand assets,
legacy CSS during migration). Standard stylelint disable comment:

```css
/* stylelint-disable-next-line vaak-tokens/no-raw-color */
background: linear-gradient(135deg, #ff6b9d 0%, #c084fc 100%);
```

Each disable comment requires a JUSTIFICATION comment in the next line per a
custom rule (`no-disable-without-comment`). Forces explicit documentation of
why the bypass is necessary. Same discipline as today's Rust `#[allow(...)]`
attributes in vaak-mcp.rs.

## Migration sequence (parallel to design-tokens-spec V2-first adoption)

**Corrigendum 1 (dev-challenger Finding 1) — Wave 0 sequencing reversed:**
the original spec proposed Wave 0 (warn-only plugin) BEFORE Wave 1
(tokens.css ships). That's bootstrap-broken: with no tokens.css yet, the
allow-set is empty and every CSS value warns. Output is noise, not signal —
we already know the existing CSS uses raw values; counting them doesn't
help. Reversing: Wave 1 ships tokens.css FIRST (populated with at least
the spacing + radius + z-index scales from the design-tokens spec), THEN
Wave 0 ships the plugin (warn-only) so the first lint output has a real
allow-set to compare against.

Wave 1: tokens.css ships per design-tokens-spec (dbc51f8) Phase 1. Populated
with at least one section (spacing scale or similar) so the plugin has a
non-empty allow-set to read at lint time.

Wave 0 (now post-Wave 1): ship the stylelint plugin + config in `desktop/`
with **WARN-only** mode for all rules. CI logs violations without failing
the build. With Wave 1's tokens populated, the warn output is actionable
signal — files with the highest raw-value count surface as sweep priorities.

Wave 2: V2 CSS (collaborate-v2.css) is rewritten using tokens per design-tokens-spec
Phase 2. Plugin rules flip from WARN to ERROR for V2 files only via stylelint's
file-pattern overrides. V2 stays strict; V1 stays permissive.

Wave 3: V1 sweep per design-tokens-spec Phase 3. Per-file ERROR enforcement as
each file is migrated. Sweep velocity is gated by the plugin's lint output.

Wave 4: V1 sweep complete. Plugin rules flip globally to ERROR. CI fails on
any future raw value. The pattern-(c) typed enforcement is fully in place.

## Acceptance test

Same pattern as the Preset enum spec's acceptance plan:

1. **Plugin unit tests.** Each rule has a fixture-based test set: cases that SHOULD lint clean, cases that SHOULD fail. Run on PR via Jest.
2. **Integration test against existing CSS.** Run the plugin in WARN-only mode against styles.css + collab.css; expected warning count matches dbc51f8's audit (98 hex, 104 px, etc.). Catches plugin regressions where the rule becomes too permissive.
3. **CI gate test.** A PR that adds a raw hex color outside the token set must fail CI lint. Verifies the prebuild gate fires.

## Pattern-(c) property demonstration

The Preset enum spec demonstrates pattern-(c) at the Rust type system: `Preset::AssemblyLine` is the only way to construct the value, every read goes through the enum, raw string construction is uncompilable post-migration.

Typed-CSS pattern-(c) at the build pipeline: token values are the only way to express a color/spacing/etc., every consumer goes through the token, raw value usage is unbuildable post-migration. Different mechanism, same architectural class — write-side primitive enforced at the read side.

Pattern-(c) is best-effort here (option 1), strict at option 2/3. Spec acknowledges this — option 1's pattern-(c) property is "every NEW CSS rule is type-checked at lint time; existing CSS can be migrated incrementally without fighting the type system." Strict enforcement is achievable but expensive (option 2/3 migration cost).

## Out of scope

- Token-value validation (e.g., ensuring `--space-3` is `8px` not `7px`). Token consistency is a design decision in design-tokens-spec, not a lint property.
- Runtime CSS-in-JS migration (option 3). Documented as future direction; not v1.5.
- ESLint integration. Style rules are CSS-layer; eslint operates on JS/TS. Different tool, different concern.
- Auto-fix. The plugin reports violations but doesn't auto-suggest token replacements — that's a separate code-mod that requires knowing which token to substitute, which is design intent not rule logic.

## Open questions for architect / evil-architect

1. **Wave 0 ship timing.** Should the stylelint plugin land BEFORE the design-tokens-spec unparks (so we have the lint baseline ready when tokens ship) or AFTER (so the allow-sets reflect agreed-upon tokens)? I lean BEFORE — the gap quantification is valuable independent of the eventual token decisions.

2. **Pattern-(c) honesty.** Should the spec language explicitly call out that this is best-effort pattern-(c), not strict pattern-(c) like the Preset enum? Avoiding the spec-overclaim issue dev-challenger found on the Preset enum spec (Finding 3 msg 624).

3. **Cross-platform path.** Vaak ships on Windows desktop today; future macOS/Linux/web targets. Does the stylelint config need platform-specific overrides for css custom properties that differ per OS, or is the token set platform-agnostic? Probably the latter, but worth confirming.

4. **Test fixture file location.** Plugin tests fixtures live in `desktop/scripts/stylelint-vaak-tokens.test.js` (co-located with plugin), or `desktop/__tests__/stylelint/` (with the rest of the test infrastructure)? Co-location is cleaner for plugin authors; centralized is easier for CI discovery. UX-lane judgment call probably.

5. **Sequencing with Preset enum PR.** Architect msg 622 said this is sequenced AFTER the Preset enum inaugural pattern lands. Should the Preset enum PR also surface ANY CSS that depends on the Preset wire format? I checked in msg 633 — frontend grep found no CSS that depends on preset names. So no cross-PR coupling.

Architect to resolve at least #1 and #2 before any code work; the others can lock in during implementation.

## Why this is worth shipping eventually

Design system drift is the structural reason "the UI feels fine, mostly fine, then suddenly wrong in one spot" (per design-tokens-spec dbc51f8 §"Why this is worth shipping"). The tokens-spec proposes WHAT to standardize on; this spec proposes HOW to enforce it. Without enforcement, drift recurs because new code can bypass the tokens without consequence. With enforcement, every new bypass is a CI failure — the team can't ship UI inconsistency by accident.

Same architectural argument as the Preset enum: making the wrong thing UNCOMPILABLE is the only mechanism that survives the team's eventual review fatigue. Today the team caught six write-without-reader bugs via adversarial discipline. That discipline doesn't scale forever; typed enforcement (Preset enum at compile time, stylelint at lint time, future option 2/3 at compile time) does.

## What this spec is NOT

- A token rename or expansion. Tokens are defined in dbc51f8.
- A CSS rewrite. Existing CSS stays as-is until migrated per the design-tokens-spec sweep.
- A typescript styling change. Components don't change.
- A build-tool migration. Vite stays Vite; this adds one plugin to the dev pipeline.
