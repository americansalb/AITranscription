/**
 * Procedural avatar generator — Phase 2.A.
 *
 * Per character-avatar-system-spec-2026-05-17.md v6.9 §3.1 + §3.2.1 + §3.3.
 * Same slug → same avatar across sessions. No I/O, no network — pure function.
 *
 * Design-system contract (spec §3.3):
 * - 64×64 viewBox (aspect locked)
 * - silhouette-first geometric primitives (circle bg + 1-2 overlay shapes)
 * - ≤4 distinct colors per avatar (Phase 2.A uses exactly 2: bg + fg)
 * - colors come from getRoleColorAccent (bg) + getRoleColorText (fg) per
 *   Light Mode spec v5 §5.7 split API. Theme-aware from day 1.
 * - shape is silhouette-first; recognizable at 20px reduction (no fine detail).
 *
 * Phase 2.B will consume this via <Avatar> component in role cards (48px/28px).
 * Phase 2.C consumes at 24px in rotation strip. Phase 2.D consumes at 20px in
 * message header (smallest target — drove the silhouette-first constraint).
 */

import { getRoleColorAccent, getRoleColorText, hashSlug, type Theme } from "./roleColors";

/** Distinct silhouette variants. Hash(slug) % VARIANT_COUNT selects one.
 * Adding variants here is safe — existing slugs may rotate to a new shape but
 * the determinism property (same slug → same shape after the rotation) holds. */
const VARIANT_COUNT = 8;

/** Foreground silhouette primitives. Each renders into a 64×64 viewBox over a
 * full-bleed background circle. Shapes chosen for 20px legibility: no detail
 * smaller than ~8px in 64px source → ~2.5px at 20px target (above retina-screen
 * legibility floor). */
function variantShape(variant: number, fill: string): string {
  switch (variant % VARIANT_COUNT) {
    case 0: // vertical bar — recognizable as "I"
      return `<rect x="26" y="14" width="12" height="36" fill="${fill}" rx="2" />`;
    case 1: // upward triangle — recognizable as "▲"
      return `<polygon points="32,14 50,46 14,46" fill="${fill}" />`;
    case 2: // diamond — recognizable as "◆"
      return `<polygon points="32,12 52,32 32,52 12,32" fill="${fill}" />`;
    case 3: // horizontal bar — recognizable as "—"
      return `<rect x="14" y="26" width="36" height="12" fill="${fill}" rx="2" />`;
    case 4: // two stacked bars — recognizable as "="
      return `<rect x="16" y="18" width="32" height="10" fill="${fill}" rx="2" /><rect x="16" y="36" width="32" height="10" fill="${fill}" rx="2" />`;
    case 5: // plus/cross — recognizable as "+"
      return `<rect x="26" y="14" width="12" height="36" fill="${fill}" rx="2" /><rect x="14" y="26" width="36" height="12" fill="${fill}" rx="2" />`;
    case 6: // inset circle — recognizable as "●"
      return `<circle cx="32" cy="32" r="14" fill="${fill}" />`;
    case 7: // downward triangle — recognizable as "▼"
      return `<polygon points="32,50 50,18 14,18" fill="${fill}" />`;
    default:
      return `<circle cx="32" cy="32" r="14" fill="${fill}" />`;
  }
}

/** Generate a procedural avatar SVG string for the given role slug + theme.
 *
 * Returns a complete inline-ready `<svg>` string with 64×64 viewBox.
 * Consumer responsibility (spec §3.3.1 Scale propagation):
 *   - apply CSS `width`/`height` at render site for surface-appropriate sizing
 *   - apply `border-radius: 50%` + `object-fit: cover` for circular crop
 *   - apply `aria-label` per spec §3.5 a11y contract: alt="${role title} (${role slug}:${instance}) avatar"
 */
export function generateAvatar(slug: string, theme: Theme = "dark"): string {
  const bg = getRoleColorAccent(slug, theme);
  const fg = getRoleColorText(slug, theme);
  const variant = hashSlug(slug);
  const shape = variantShape(variant, fg);
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64" role="img"><circle cx="32" cy="32" r="32" fill="${bg}" />${shape}</svg>`;
}

/** Generate a data URL for the avatar (useful for `<img src=...>` consumers
 * that prefer not to inline raw SVG via dangerouslySetInnerHTML). */
export function generateAvatarDataUrl(slug: string, theme: Theme = "dark"): string {
  const svg = generateAvatar(slug, theme);
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}
