/**
 * XPBar - Displays XP progress within current level with tier badge
 */

import { useMemo, useEffect, useState } from "react";
import { TierBadge, TIER_COLORS, PrestigeTier } from "./TierBadge";

interface XPBarProps {
  currentXP: number;
  xpToNextLevel: number;
  level: number;
  tier: PrestigeTier;
  lifetimeXP: number;
  compact?: boolean;
  showTierProgress?: boolean;
  tierProgress?: {
    xp_in_tier: number;
    tier_start_xp: number;
    tier_end_xp: number | null;
    progress: number;
  };
  animated?: boolean;
}

export function XPBar({
  currentXP,
  xpToNextLevel,
  level,
  tier,
  lifetimeXP,
  compact = false,
  showTierProgress = false,
  tierProgress,
  animated = true,
}: XPBarProps) {
  const [displayXP, setDisplayXP] = useState(currentXP);
  const progress = useMemo(() => {
    return Math.min((displayXP / xpToNextLevel) * 100, 100);
  }, [displayXP, xpToNextLevel]);

  // Animate XP changes
  useEffect(() => {
    if (!animated) {
      setDisplayXP(currentXP);
      return;
    }

    const diff = currentXP - displayXP;
    if (diff === 0) return;

    const steps = 20;
    const stepSize = diff / steps;
    let current = displayXP;
    let step = 0;

    const interval = setInterval(() => {
      step++;
      current += stepSize;
      if (step >= steps) {
        setDisplayXP(currentXP);
        clearInterval(interval);
      } else {
        setDisplayXP(Math.round(current));
      }
    }, 30);

    return () => clearInterval(interval);
  }, [currentXP, animated]);

  const tierColor = TIER_COLORS[tier];

  if (compact) {
    return (
      <div className="xp-bar-compact flex items-center gap-2">
        <TierBadge tier={tier} level={level} size="sm" />
        <div className="flex-1">
          <div
            className="h-2 rounded-full overflow-hidden"
            style={{ background: `${tierColor}20` }}
          >
            <div
              className="h-full rounded-full transition-all duration-300"
              style={{
                width: `${progress}%`,
                background: `linear-gradient(90deg, ${tierColor} 0%, ${tierColor}CC 100%)`,
              }}
            />
          </div>
        </div>
        <span className="text-xs text-gray-400">
          {displayXP.toLocaleString()}/{xpToNextLevel.toLocaleString()} XP
        </span>
      </div>
    );
  }

  return (
    <div className="xp-bar-full p-4 rounded-xl bg-gray-800/50 border border-gray-700">
      <div className="flex items-center justify-between mb-3">
        <TierBadge tier={tier} level={level} size="md" />
        <div className="text-right">
          <div className="text-sm text-gray-400">Lifetime XP</div>
          <div className="text-lg font-bold" style={{ color: tierColor }}>
            {lifetimeXP.toLocaleString()}
          </div>
        </div>
      </div>

      {/* Level Progress */}
      <div className="mb-2">
        <div className="flex justify-between text-sm mb-1">
          <span className="text-gray-400">Level {level} Progress</span>
          <span className="text-gray-300">
            {displayXP.toLocaleString()} / {xpToNextLevel.toLocaleString()} XP
          </span>
        </div>
        <div
          className="h-3 rounded-full overflow-hidden"
          style={{ background: `${tierColor}20` }}
        >
          <div
            className="h-full rounded-full transition-all duration-500 relative overflow-hidden"
            style={{
              width: `${progress}%`,
              background: `linear-gradient(90deg, ${tierColor} 0%, ${tierColor}CC 100%)`,
            }}
          >
            {/* Shimmer effect */}
            <div
              className="absolute inset-0 opacity-30"
              style={{
                background:
                  "linear-gradient(90deg, transparent 0%, white 50%, transparent 100%)",
                animation: "shimmer 2s infinite",
              }}
            />
          </div>
        </div>
        <div className="text-xs text-gray-500 mt-1">
          {(xpToNextLevel - displayXP).toLocaleString()} XP to Level {level + 1}
        </div>
      </div>

      {/* Tier Progress */}
      {showTierProgress && tierProgress && tierProgress.tier_end_xp && (
        <div className="mt-4 pt-3 border-t border-gray-700">
          <div className="flex justify-between text-sm mb-1">
            <span className="text-gray-400">
              {tier.charAt(0).toUpperCase() + tier.slice(1)} Tier Progress
            </span>
            <span className="text-gray-300">
              {Math.round(tierProgress.progress * 100)}%
            </span>
          </div>
          <div className="h-2 rounded-full overflow-hidden bg-gray-700">
            <div
              className="h-full rounded-full transition-all duration-500"
              style={{
                width: `${tierProgress.progress * 100}%`,
                background: `linear-gradient(90deg, ${tierColor}80 0%, ${tierColor} 100%)`,
              }}
            />
          </div>
          <div className="text-xs text-gray-500 mt-1">
            {(
              tierProgress.tier_end_xp - tierProgress.xp_in_tier
            ).toLocaleString()}{" "}
            XP to next tier
          </div>
        </div>
      )}
    </div>
  );
}

// Mini XP indicator for header/nav
export function XPIndicator({
  level,
  tier,
  progress,
  onClick,
}: {
  level: number;
  tier: PrestigeTier;
  progress: number;
  onClick?: () => void;
}) {
  const tierColor = TIER_COLORS[tier];

  return (
    <button
      onClick={onClick}
      className="xp-indicator flex items-center gap-2 px-3 py-1.5 rounded-lg bg-gray-800/50 hover:bg-gray-700/50 transition-colors border border-gray-700"
    >
      <span className="text-lg">
        {tier === "bronze"
          ? "🥉"
          : tier === "silver"
          ? "🥈"
          : tier === "gold"
          ? "🥇"
          : tier === "platinum"
          ? "💎"
          : tier === "diamond"
          ? "💠"
          : tier === "master"
          ? "👑"
          : "🌟"}
      </span>
      <div className="flex flex-col items-start">
        <span className="text-xs text-gray-400">Lv.{level}</span>
        <div
          className="w-16 h-1.5 rounded-full overflow-hidden"
          style={{ background: `${tierColor}30` }}
        >
          <div
            className="h-full rounded-full"
            style={{
              width: `${progress * 100}%`,
              background: tierColor,
            }}
          />
        </div>
      </div>
    </button>
  );
}
