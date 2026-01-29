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
  const sizeClasses = useMemo(() => {
    switch (size) {
      case "sm":
        return {
          container: "h-6 px-2 text-xs gap-1",
          icon: "text-sm",
        };
      case "lg":
        return {
          container: "h-10 px-4 text-base gap-2",
          icon: "text-xl",
        };
      default:
        return {
          container: "h-8 px-3 text-sm gap-1.5",
          icon: "text-lg",
        };
    }
  }, [size]);

  const isLegend = tier === "legend";

  return (
    <div
      className={`tier-badge tier-badge-${tier} inline-flex items-center rounded-full font-semibold ${sizeClasses.container}`}
      style={{
        background: TIER_GRADIENTS[tier],
        color: isLegend || tier === "gold" ? "#1a1a1a" : "#fff",
        boxShadow: `0 2px 8px ${TIER_COLORS[tier]}40`,
        border: `1px solid ${TIER_COLORS[tier]}80`,
      }}
    >
      <span className={sizeClasses.icon}>{TIER_ICONS[tier]}</span>
      <span className="capitalize">{tier}</span>
      {showLevel && (
        <span
          className="ml-1 opacity-80"
          style={{
            fontSize: "0.85em",
          }}
        >
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
