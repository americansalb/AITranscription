/**
 * Shared Avatar component — Phase 2.B Part 2.
 *
 * Per ui-architect:1 msg 4656 directive: extract the procedural-default +
 * avatar_url-override + onError-fallback + spec-§3.5 alt-text triple into
 * a single consumer used by RolesTab, CollabTab roster, popovers (Phase 2.C-D-E).
 *
 * Surface-context alt-text split (per dev-challenger:0 msg 4652 + evil-arch
 * msg 4654 spec §3.5 v6.10-pending amendment):
 *   - Role-definition surface (RolesTab, no instance number): "${title} (${slug}) avatar"
 *   - Instance-runtime surface (CollabTab roster, rotation strip, etc.): "${title} (${slug}:${instance}) avatar"
 *
 * Privacy forward-flag (per evil-arch msg 4654 F-EA-AVATAR-URL-PASSIVE-EXFIL):
 * `<img src={avatarUrl}>` rendering of third-party URLs leaks viewer IP/UA to
 * the URL host on every render. Spec v6.10 §4.1 amendment pending (proxy-and-cache
 * via Tauri sidecar). Until then, callers passing avatar_url should be aware that
 * non-bunny-CDN hosts represent passive-exfil surface for project.json sharing.
 */

import { generateAvatarDataUrl } from "../utils/proceduralAvatar";
import type { Theme } from "../utils/roleColors";

export interface AvatarProps {
  slug: string;
  title?: string;
  /** Instance number — pass for instance-runtime surfaces (CollabTab roster,
   * rotation strip, message header). Omit for role-definition surfaces (RolesTab). */
  instance?: number;
  /** Optional avatar_url override; null/undefined falls through to procedural. */
  avatarUrl?: string | null;
  /** Pixel size; consumer responsibility per spec §3.3.1 scale propagation. */
  sizePx: number;
  /** Theme parameter. Defaults to "dark" until Light Mode Phase 3 ThemeContext ships. */
  theme?: Theme;
  className?: string;
}

export function Avatar({ slug, title, instance, avatarUrl, sizePx, theme = "dark", className }: AvatarProps) {
  const proceduralSrc = generateAvatarDataUrl(slug, theme);
  const displayTitle = title || slug;
  const altText = instance !== undefined
    ? `${displayTitle} (${slug}:${instance}) avatar`
    : `${displayTitle} (${slug}) avatar`;
  return (
    <img
      src={avatarUrl || proceduralSrc}
      alt={altText}
      width={sizePx}
      height={sizePx}
      style={{ borderRadius: "50%", objectFit: "cover", display: "block" }}
      loading="lazy"
      className={className}
      onError={(e) => {
        // avatar_url load failure (HTTPS 404, decode error, etc.) →
        // fall back to procedural per spec §4 fallback contract.
        const img = e.currentTarget;
        if (img.src !== proceduralSrc) img.src = proceduralSrc;
      }}
    />
  );
}
