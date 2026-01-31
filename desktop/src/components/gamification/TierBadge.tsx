/**
 * TierBadge - Displays the user's prestige tier with appropriate styling
 */

import { useMemo } from "react";

export type PrestigeTier =
  | "bronze"
  | "silver"
  | "gold"
  | "platinum"
  | "diamond"
  | "master"
  | "legend";

interface TierBadgeProps {
  tier: PrestigeTier;
  level: number;
  size?: "sm" | "md" | "lg";
  showLevel?: boolean;
}

const TIER_COLORS: Record<PrestigeTier, string> = {
  bronze: "#CD7F32",
  silver: "#C0C0C0",
  gold: "#FFD700",
  platinum: "#E5E4E2",
  diamond: "#B9F2FF",
  master: "#9B30FF",
  legend: "rainbow",
};

const TIER_ICONS: Record<PrestigeTier, string> = {
  bronze: "🥉",
  silver: "🥈",
  gold: "🥇",
  platinum: "💎",
  diamond: "💠",
  master: "👑",
  legend: "🌟",
};

const TIER_GRADIENTS: Record<PrestigeTier, string> = {
  bronze: "linear-gradient(135deg, #CD7F32 0%, #8B4513 100%)",
  silver: "linear-gradient(135deg, #E8E8E8 0%, #A8A8A8 100%)",
  gold: "linear-gradient(135deg, #FFD700 0%, #FFA500 100%)",
  platinum: "linear-gradient(135deg, #E5E4E2 0%, #B8B8B8 100%)",
  diamond: "linear-gradient(135deg, #B9F2FF 0%, #87CEEB 100%)",
  master: "linear-gradient(135deg, #9B30FF 0%, #7B68EE 100%)",
  legend:
    "linear-gradient(135deg, #FF0000 0%, #FF7F00 14%, #FFFF00 28%, #00FF00 42%, #0000FF 57%, #4B0082 71%, #9400D3 85%, #FF0000 100%)",
};

export function TierBadge({
  tier,
  level,
  size = "md",
  showLevel = true,
}: TierBadgeProps) {
  const sizeStyles = useMemo(() => {
    switch (size) {
      case "sm":
        return { height: 24, padding: "0 8px", fontSize: 12, iconSize: 14, gap: 4 };
      case "lg":
        return { height: 40, padding: "0 16px", fontSize: 16, iconSize: 22, gap: 8 };
      default:
        return { height: 32, padding: "0 12px", fontSize: 14, iconSize: 18, gap: 6 };
    }
  }, [size]);

  const isLegend = tier === "legend";

  return (
    <div
      className={`tier-badge tier-badge-${size} ${isLegend ? "tier-badge-legend" : ""}`}
      style={{
        display: "inline-flex",
        alignItems: "center",
        borderRadius: 9999,
        fontWeight: 600,
        height: sizeStyles.height,
        padding: sizeStyles.padding,
        fontSize: sizeStyles.fontSize,
        gap: sizeStyles.gap,
        background: TIER_GRADIENTS[tier],
        color: isLegend || tier === "gold" ? "#1a1a1a" : "#fff",
        boxShadow: `0 2px 8px ${TIER_COLORS[tier]}40`,
        border: `1px solid ${TIER_COLORS[tier]}80`,
      }}
    >
      <span style={{ fontSize: sizeStyles.iconSize }}>{TIER_ICONS[tier]}</span>
      <span style={{ textTransform: "capitalize" }}>{tier}</span>
      {showLevel && (
        <span style={{ fontSize: "0.85em", opacity: 0.8, marginLeft: 2 }}>
          Lv.{level}
        </span>
      )}
    </div>
  );
}

export function TierIcon({
  tier,
  size = "md",
}: {
  tier: PrestigeTier;
  size?: "sm" | "md" | "lg";
}) {
  const fontSize = size === "sm" ? "1rem" : size === "lg" ? "2rem" : "1.5rem";

  return (
    <span
      style={{
        fontSize,
        filter:
          tier === "legend" ? "drop-shadow(0 0 4px rgba(255,255,255,0.8))" : "",
      }}
    >
      {TIER_ICONS[tier]}
    </span>
  );
}

export { TIER_COLORS, TIER_ICONS, TIER_GRADIENTS };
