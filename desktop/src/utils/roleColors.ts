/**
 * Shared role color utilities.
 *
 * Single source of truth for role → color mapping used by CollabTab,
 * DiscussionPanel, PipelineStepper, RolesTab, and any future component
 * (including Avatar Phase 2.A procedural generator) that needs role-colored UI.
 *
 * Phase 5a refactor per Light Mode spec v4 §5.7 + evil-arch msg 4541:
 * - CANONICAL_ROLE_COLORS holds un-dimmed source-of-truth (was pre-dimmed at module
 *   load, lost the canonical hex). Dimming is computed at call time per theme.
 * - Split API: getRoleColorText (high contrast, for text labels) vs
 *   getRoleColorAccent (low contrast, for borders/backgrounds/avatar fills).
 * - Theme parameter (default "dark" for backward compat). Light mode applies
 *   mix-toward-BLACK for darkening on light bg, with HSL desaturation on accents
 *   to reduce visual weight without losing WCAG contrast.
 * - getRoleColor() alias preserved as backward-compat wrapper for callers that
 *   haven't migrated to the split API yet.
 */

const DARK_SURFACE = { r: 0x15, g: 0x20, b: 0x2b }; // #15202b Vaak dark surface
const BLACK = { r: 0x00, g: 0x00, b: 0x00 };

function hexToRgb(hex: string): { r: number; g: number; b: number } | null {
  const m = /^#([0-9a-f]{6})$/i.exec(hex);
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return { r: (n >> 16) & 0xff, g: (n >> 8) & 0xff, b: n & 0xff };
}

function rgbToHex(r: number, g: number, b: number): string {
  return "#" + [r, g, b].map(v => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, "0")).join("");
}

/** Mix source hex with target rgb at given source-weight (0..1). Dark-mode
 * dim used 60/40 toward DARK_SURFACE. Light-mode darkening uses 60/40 toward BLACK. */
function mixToward(hex: string, target: { r: number; g: number; b: number }, sourceWeight: number): string {
  const rgb = hexToRgb(hex);
  if (!rgb) return hex;
  const w = sourceWeight;
  return rgbToHex(
    rgb.r * w + target.r * (1 - w),
    rgb.g * w + target.g * (1 - w),
    rgb.b * w + target.b * (1 - w),
  );
}

/** HSL desaturate: reduce S by `amount` (0..1). Preserves H + L.
 * Per Light Mode spec v4 §5.7 V-4 fix: accent in light mode needs visual hierarchy
 * separation from text. Same-darkness mix would collapse text and accent to identical
 * values. Desaturating the accent (while keeping text canonically saturated) recreates
 * the muted-vs-vivid Tier-3 affordance from dark mode on a light bg. */
function desaturate(hex: string, amount: number): string {
  const rgb = hexToRgb(hex);
  if (!rgb) return hex;
  const r = rgb.r / 255, g = rgb.g / 255, b = rgb.b / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0, s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case r: h = (g - b) / d + (g < b ? 6 : 0); break;
      case g: h = (b - r) / d + 2; break;
      case b: h = (r - g) / d + 4; break;
    }
    h /= 6;
  }
  s = Math.max(0, Math.min(1, s - amount));
  // HSL → RGB
  if (s === 0) {
    const v = Math.round(l * 255);
    return rgbToHex(v, v, v);
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const hue2rgb = (t: number) => {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  return rgbToHex(hue2rgb(h + 1 / 3) * 255, hue2rgb(h) * 255, hue2rgb(h - 1 / 3) * 255);
}

/** Canonical (un-dimmed) source-of-truth role colors. Tier 3 dim is applied at
 * call time via getRoleColorAccent per theme. Light Mode spec v4 §5.7. */
export const CANONICAL_ROLE_COLORS: Record<string, string> = {
  manager: "#9b59b6",
  architect: "#1da1f2",
  developer: "#17bf63",
  tester: "#f5a623",
  audience: "#e74c3c",
  user: "#e1e8ed",
};

/** Canonical hash palette for custom roles. */
export const CANONICAL_HASH_PALETTE = [
  "#e91e63", // pink
  "#00bcd4", // cyan
  "#ff7043", // deep orange
  "#8bc34a", // lime green
  "#7e57c2", // deep purple
  "#26a69a", // teal
  "#ec407a", // rose
  "#42a5f5", // sky blue
  "#ffa726", // amber
  "#66bb6a", // medium green
  "#ef5350", // coral
  "#ab47bc", // orchid
];

/** Backward-compat: pre-dimmed dark-mode ROLE_COLORS / HASH_PALETTE exports for
 * any callers that still import these directly. New code should use the
 * getRoleColorAccent(slug, theme) API. */
export const ROLE_COLORS: Record<string, string> = Object.fromEntries(
  Object.entries(CANONICAL_ROLE_COLORS).map(([k, v]) => [k, mixToward(v, DARK_SURFACE, 0.6)])
);
export const HASH_PALETTE = CANONICAL_HASH_PALETTE.map(v => mixToward(v, DARK_SURFACE, 0.6));

/** FNV-1a hash for deterministic color assignment */
export function hashSlug(slug: string): number {
  let hash = 2166136261;
  for (let i = 0; i < slug.length; i++) {
    hash ^= slug.charCodeAt(i);
    hash = Math.imul(hash, 16777619) & 0xffffffff;
  }
  return hash >>> 0;
}

function lookupCanonical(slug: string): string {
  if (CANONICAL_ROLE_COLORS[slug]) return CANONICAL_ROLE_COLORS[slug];
  for (const [prefix, color] of Object.entries(CANONICAL_ROLE_COLORS)) {
    if (slug.startsWith(prefix)) return color;
  }
  return CANONICAL_HASH_PALETTE[hashSlug(slug) % CANONICAL_HASH_PALETTE.length];
}

export type Theme = "light" | "dark";

/** Text color for role labels — high visual weight, meets WCAG 4.5:1 body text.
 * Dark mode: canonical hex. Light mode: darkened toward black for contrast on
 * light bg. Per Light Mode spec v4 §5.7.
 *
 * sourceWeight=0.6 means 60% canonical + 40% BLACK per mixToward signature.
 * Spec v4 §5.7 example: architect #1da1f2 + 40% black → #116191 (verified
 * by hand: 29*0.6 + 0*0.4 = 17.4 → 0x11). Prior aa97198 used 0.4 (interpreting
 * spec "0.4" as source weight) which produced strongly-darkened #0C4061 —
 * tester msg 4581 caught the spec/implementation semantic mismatch. */
export function getRoleColorText(slug: string, theme: Theme = "dark"): string {
  const canonical = lookupCanonical(slug);
  if (theme === "light") return mixToward(canonical, BLACK, 0.6);
  return canonical;
}

/** Accent color for role borders, backgrounds, avatar fills — low visual weight,
 * meets WCAG 3:1 UI threshold. Distinct from text variant in both themes to
 * preserve the Tier-3 dim affordance. Per Light Mode spec v4 §5.7 V-4 fix
 * (HSL desaturate prevents text/accent collapse in light mode). */
export function getRoleColorAccent(slug: string, theme: Theme = "dark"): string {
  const canonical = lookupCanonical(slug);
  if (theme === "light") return desaturate(mixToward(canonical, BLACK, 0.6), 0.3);
  return mixToward(canonical, DARK_SURFACE, 0.6); // existing dark-mode Tier 3 dim
}

/** Backward-compat alias: callers that haven't migrated to the split API still
 * get the accent variant (matches pre-Phase-5a behavior — dark-mode dimmed). */
export function getRoleColor(slug: string, theme: Theme = "dark"): string {
  return getRoleColorAccent(slug, theme);
}

/** Format-specific discussion mode colors */
export const MODE_COLORS: Record<string, string> = {
  pipeline: "#22c55e",
  delphi: "#818cf8",
  oxford: "#f87171",
  red_team: "#fb923c",
  continuous: "#fbbf24",
};

/** Get the display color for a discussion mode */
export function getModeColor(mode: string | null): string {
  return (mode && MODE_COLORS[mode]) || "#9b59b6";
}
