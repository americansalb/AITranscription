/**
 * Phase 2.A SVG fixture generator.
 *
 * Per character-avatar-system-spec-2026-05-17.md v6.9 §3.2.1 + ui-architect:1
 * msg 4601 (Phase 2.A authorization) + evil-architect msg 4568 fixture contract:
 * iterate 18 role slugs × 2 themes (dark + light) → 36 panels in a single SVG
 * file. UI-arch + tester + evil-arch + dev-challenger use this file for visual
 * ratification (Ruling 13 gate #3) without needing a running Vaak instance.
 *
 * Output: .vaak/diagnostics/avatar-phase-2A-fixtures.svg
 *
 * Run: `npx tsx desktop/scripts/generate-avatar-fixtures.ts` (from project root)
 *
 * Determinism contract: re-running this script produces byte-identical output
 * (modulo trivial whitespace). Same slug → same avatar by definition.
 */

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { generateAvatar } from "../src/utils/proceduralAvatar.ts";
import { CANONICAL_ROLE_COLORS, CANONICAL_HASH_PALETTE, type Theme } from "../src/utils/roleColors.ts";

/** Synthetic slugs for the 12 HASH_PALETTE indices. Real custom roles use real
 * slugs; for fixture purposes we craft slugs that deterministically hash to each
 * palette index 0-11 so the fixture exercises every palette color. */
function craftHashSlug(index: number): string {
  // hashSlug FNV-1a mod 12 must equal index. Brute-force a short tag with
  // a numeric suffix until the hash lands on the right index.
  // Mirror of roleColors.ts hashSlug — kept inline to avoid pulling the import.
  function hashSlug(s: string): number {
    let h = 2166136261;
    for (let i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i);
      h = Math.imul(h, 16777619) & 0xffffffff;
    }
    return h >>> 0;
  }
  for (let i = 0; i < 10000; i++) {
    const candidate = `custom-${index}-${i}`;
    if (hashSlug(candidate) % CANONICAL_HASH_PALETTE.length === index) return candidate;
  }
  // Fallback (should not hit for index 0-11 in <10K tries):
  return `custom-${index}`;
}

const CANONICAL_SLUGS = Object.keys(CANONICAL_ROLE_COLORS);
const HASH_SLUGS = CANONICAL_HASH_PALETTE.map((_, i) => craftHashSlug(i));
const ALL_SLUGS = [...CANONICAL_SLUGS, ...HASH_SLUGS];
const THEMES: Theme[] = ["dark", "light"];

const PANEL_W = 96;
const PANEL_H = 96;
const COLS = 9; // 18 slugs → 2 rows per theme; total 4 rows in landscape grid
const ROWS_PER_THEME = Math.ceil(ALL_SLUGS.length / COLS);
const SECTION_HEADER_H = 32;
const SECTION_H = SECTION_HEADER_H + ROWS_PER_THEME * PANEL_H;
const TOTAL_W = COLS * PANEL_W;
const TOTAL_H = THEMES.length * SECTION_H + 32; // +32 for top title

function panelLabel(slug: string): string {
  // Truncate long slugs so they fit in 96px-wide panel at 10px font
  return slug.length > 14 ? slug.slice(0, 13) + "…" : slug;
}

function buildSection(theme: Theme, yOffset: number): string {
  const sectionBg = theme === "light" ? "#f7f7f7" : "#15202b";
  const sectionFg = theme === "light" ? "#000000" : "#e1e8ed";
  let svg = `<g transform="translate(0,${yOffset})">`;
  svg += `<rect x="0" y="0" width="${TOTAL_W}" height="${SECTION_H}" fill="${sectionBg}" />`;
  svg += `<text x="12" y="22" font-family="sans-serif" font-size="14" font-weight="bold" fill="${sectionFg}">Theme: ${theme} (${ALL_SLUGS.length} roles)</text>`;
  ALL_SLUGS.forEach((slug, i) => {
    const col = i % COLS;
    const row = Math.floor(i / COLS);
    const px = col * PANEL_W;
    const py = SECTION_HEADER_H + row * PANEL_H;
    const avatar = generateAvatar(slug, theme);
    // Inline the avatar SVG inside a nested <svg> for positioning.
    const inner = avatar.replace(/^<svg[^>]*>/, "").replace(/<\/svg>$/, "");
    svg += `<g transform="translate(${px + 16},${py + 8})">${inner}</g>`;
    svg += `<text x="${px + PANEL_W / 2}" y="${py + 88}" font-family="monospace" font-size="9" fill="${sectionFg}" text-anchor="middle">${panelLabel(slug)}</text>`;
  });
  svg += `</g>`;
  return svg;
}

function buildFixture(): string {
  const head = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${TOTAL_W} ${TOTAL_H}" width="${TOTAL_W}" height="${TOTAL_H}">`;
  const title = `<text x="12" y="22" font-family="sans-serif" font-size="16" font-weight="bold" fill="#888">Phase 2.A Procedural Avatar Fixtures — 18 roles × 2 themes (Ruling 13 gate #3)</text>`;
  let body = "";
  THEMES.forEach((theme, idx) => {
    body += buildSection(theme, 32 + idx * SECTION_H);
  });
  return `${head}${title}${body}</svg>\n`;
}

const __dirname = dirname(fileURLToPath(import.meta.url));
const outPath = resolve(__dirname, "../../.vaak/diagnostics/avatar-phase-2A-fixtures.svg");
mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, buildFixture(), "utf8");
console.log(`Wrote ${outPath}`);
console.log(`Panels: ${ALL_SLUGS.length * THEMES.length} (${ALL_SLUGS.length} slugs × ${THEMES.length} themes)`);
console.log(`Slugs: ${ALL_SLUGS.join(", ")}`);
