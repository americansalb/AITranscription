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
  /** Instance number — pass for instance-runtime surfaces with an ACTIVE seat
   * (CollabTab roster active rows, rotation strip current speaker, message header).
   * Omit for role-definition surfaces (RolesTab) AND for VACANT cards in the
   * roster: passing `instance={0}` to a vacant card produces misleading alt
   * text "title (slug:0) avatar" for screen readers, falsely claiming an
   * instance exists. Pass `undefined` for vacant cards per F-EA-VACANT-SENTINEL-CLASS
   * (evil-architect:0 msg 4665). */
  instance?: number;
  /** Optional avatar_url override; null/undefined falls through to procedural. */
  avatarUrl?: string | null;
  /** Pixel size; consumer responsibility per spec §3.3.1 scale propagation. */
  sizePx: number;
  /** Theme parameter. Defaults to "dark" until Light Mode Phase 3 ThemeContext ships. */
  theme?: Theme;
  className?: string;
}

/** Parse a "slug:instance" seat string (or bare "slug") into Avatar props.
 *
 * Extracted from Phase 2.C parsing logic (ProtocolPanel.tsx 81711a0) because
 * Phase 2.D message-header avatars + future Phase 2.E/F consumers all need
 * the same normalization. Per ui-architect:1 msg 4719 helper-extraction
 * recommendation (pattern appearing in 3+ places).
 *
 * Sentinel-class discipline (F-EA-VACANT-SENTINEL-CLASS + F-EA-EMPTY-STRING-
 * INSTANCE-CLAMP + F-EA-NAN-INSTANCE-CLAMP + F-EA-EMPTY-ROLE-CLAMP from
 * Phase 2.B Part 2 + Phase 2.C sister-fix cycles):
 *   - Empty slug ("", ":0") → returns slug:"" (caller decides to render or not)
 *   - Empty instance ("developer:") → instance:undefined → role-definition alt
 *   - Non-numeric instance ("developer:abc") → instance:undefined → role-definition
 *   - Valid "slug:N" → instance:N → instance-runtime alt
 *   - Bare "slug" → instance:undefined → role-definition alt
 */
export function parseSeatInstance(seat: string): { slug: string; instance: number | undefined } {
  const [slug, instanceStr] = seat.split(":");
  const instanceNum = instanceStr ? Number(instanceStr) : NaN;
  const instance = Number.isInteger(instanceNum) ? instanceNum : undefined;
  return { slug: slug || "", instance };
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
