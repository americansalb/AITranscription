/**
 * Shared role color utilities.
 *
 * Single source of truth for role → color mapping used by CollabTab,
 * DiscussionPanel, PipelineStepper, and any future component that
 * needs to display role-colored UI elements.
 */

/** Named colors for built-in roles */
export const ROLE_COLORS: Record<string, string> = {
  manager: "#9b59b6",
  architect: "#1da1f2",
  developer: "#17bf63",
  tester: "#f5a623",
  audience: "#e74c3c",
  user: "#e1e8ed",
};

/** Palette for dynamically-created roles — deterministic via FNV-1a slug hash */
export const HASH_PALETTE = [
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

/** FNV-1a hash for deterministic color assignment */
export function hashSlug(slug: string): number {
  let hash = 2166136261;
  for (let i = 0; i < slug.length; i++) {
    hash ^= slug.charCodeAt(i);
    hash = Math.imul(hash, 16777619) & 0xffffffff;
  }
  return hash >>> 0;
}

/**
 * Get the display color for a role slug.
 *
 * - Built-in roles (manager, architect, developer, tester, audience, user)
 *   return their named color.
 * - Roles starting with a built-in prefix (e.g., "developer-challenger")
 *   return the parent color.
 * - Custom roles get a deterministic color from HASH_PALETTE via FNV-1a.
 */
export function getRoleColor(slug: string): string {
  if (ROLE_COLORS[slug]) return ROLE_COLORS[slug];
  for (const [prefix, color] of Object.entries(ROLE_COLORS)) {
    if (slug.startsWith(prefix)) return color;
  }
  return HASH_PALETTE[hashSlug(slug) % HASH_PALETTE.length];
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
