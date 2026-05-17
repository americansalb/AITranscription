/**
 * Stats radar chart — Phase 2.E (per character-avatar-system-spec v6.9 §3.4
 * + ui-architect:1 msg 4725).
 *
 * Hand-rolled SVG, no chart-library dependency (consistent with proceduralAvatar.ts).
 * 6 axes at 60° intervals:
 *   - TD (top, 270° / 12 o'clock)
 *   - AR (top-right, 330° / 2 o'clock)
 *   - CP (bottom-right, 30° / 4 o'clock)
 *   - DO (bottom, 90° / 6 o'clock)
 *   - PD (bottom-left, 150° / 8 o'clock)
 *   - JA (top-left, 210° / 10 o'clock)
 *
 * Stat values 1-10 → radial distance from center (proportional).
 * Filled polygon connects the 6 points.
 * Color: role-color via getRoleColor (existing Tier 3 dim palette).
 *
 * Defensive default per F-EA-VACANT-SENTINEL-CLASS discipline broadened:
 * missing stats prop renders mid-scale (5/10 per axis) with reduced opacity
 * to visually distinguish "unknown" from "explicitly mid". Never null-deref.
 */

import { getRoleColor } from "../utils/roleColors";

export interface StatsRadarProps {
  slug: string;
  stats?: { td: number; ar: number; cp: number; do: number; pd: number; ja: number };
  /** Canvas size in pixels — Phase 2.E spec calls for 120×120. */
  sizePx?: number;
}

const AXES: Array<{ key: "td" | "ar" | "cp" | "do" | "pd" | "ja"; label: string; angleDeg: number }> = [
  { key: "td", label: "TD", angleDeg: 270 }, // top
  { key: "ar", label: "AR", angleDeg: 330 }, // top-right
  { key: "cp", label: "CP", angleDeg: 30 },  // bottom-right
  { key: "do", label: "DO", angleDeg: 90 },  // bottom
  { key: "pd", label: "PD", angleDeg: 150 }, // bottom-left
  { key: "ja", label: "JA", angleDeg: 210 }, // top-left
];

function polar(cx: number, cy: number, r: number, angleDeg: number): { x: number; y: number } {
  const rad = (angleDeg * Math.PI) / 180;
  return { x: cx + r * Math.cos(rad), y: cy + r * Math.sin(rad) };
}

export function StatsRadar({ slug, stats, sizePx = 120 }: StatsRadarProps) {
  const cx = sizePx / 2;
  const cy = sizePx / 2;
  const maxR = sizePx * 0.4; // leave room for axis labels at the edge
  const labelR = sizePx * 0.47;
  const roleColor = getRoleColor(slug);
  // Defensive fallback per spec §3.4 + F-EA-VACANT-SENTINEL-CLASS discipline:
  // missing stats render mid-scale (5/10) at reduced opacity.
  const hasStats = !!stats;
  const safeStats = stats || { td: 5, ar: 5, cp: 5, do: 5, pd: 5, ja: 5 };

  // Build the filled polygon points (per-axis value 1-10 → proportional radius)
  const polygonPoints = AXES.map(({ key, angleDeg }) => {
    const v = Math.max(1, Math.min(10, safeStats[key]));
    const r = (v / 10) * maxR;
    const { x, y } = polar(cx, cy, r, angleDeg);
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");

  // Concentric ring guides at 25/50/75/100% for scale reference
  const rings = [0.25, 0.5, 0.75, 1.0];

  return (
    <svg
      width={sizePx}
      height={sizePx}
      viewBox={`0 0 ${sizePx} ${sizePx}`}
      role="img"
      aria-label={`${slug} 6-axis stats radar: ${
        AXES.map(({ key, label }) => `${label} ${safeStats[key]}/10`).join(", ")
      }${hasStats ? "" : " (defaults — no stats configured)"}`}
    >
      {/* Concentric ring guides */}
      {rings.map((frac, i) => (
        <circle
          key={i}
          cx={cx}
          cy={cy}
          r={frac * maxR}
          fill="none"
          stroke="rgba(255,255,255,0.08)"
          strokeWidth={1}
        />
      ))}
      {/* Axis lines from center to each axis end */}
      {AXES.map(({ angleDeg, key }) => {
        const end = polar(cx, cy, maxR, angleDeg);
        return (
          <line
            key={key}
            x1={cx}
            y1={cy}
            x2={end.x}
            y2={end.y}
            stroke="rgba(255,255,255,0.1)"
            strokeWidth={1}
          />
        );
      })}
      {/* Filled stat polygon */}
      <polygon
        points={polygonPoints}
        fill={roleColor}
        fillOpacity={hasStats ? 0.4 : 0.15}
        stroke={roleColor}
        strokeOpacity={hasStats ? 0.9 : 0.4}
        strokeWidth={1.5}
        strokeLinejoin="round"
      />
      {/* Axis labels */}
      {AXES.map(({ key, label, angleDeg }) => {
        const pos = polar(cx, cy, labelR, angleDeg);
        return (
          <text
            key={key}
            x={pos.x}
            y={pos.y + 3}
            textAnchor="middle"
            fontFamily="sans-serif"
            fontSize={9}
            fontWeight={600}
            fill="rgba(255,255,255,0.55)"
          >
            {label}
          </text>
        );
      })}
    </svg>
  );
}
