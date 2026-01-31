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
      <div className="xp-bar-compact">
        <TierBadge tier={tier} level={level} size="sm" />
        <div className="xp-bar-track-wrap">
          <div
            className="xp-bar-track"
            style={{ background: `${tierColor}20` }}
          >
            <div
              className="xp-bar-fill"
              style={{
                width: `${progress}%`,
                background: `linear-gradient(90deg, ${tierColor} 0%, ${tierColor}CC 100%)`,
              }}
            />
          </div>
        </div>
        <span className="xp-bar-label-sm">
          {displayXP.toLocaleString()}/{xpToNextLevel.toLocaleString()} XP
        </span>
      </div>
    );
  }

  return (
    <div className="xp-bar-full">
      <div className="xp-bar-header">
        <TierBadge tier={tier} level={level} size="md" />
        <div className="xp-bar-lifetime">
          <div className="xp-bar-lifetime-label">Lifetime XP</div>
          <div className="xp-bar-lifetime-value" style={{ color: tierColor }}>
            {lifetimeXP.toLocaleString()}
          </div>
        </div>
      </div>

      {/* Level Progress */}
      <div className="xp-bar-progress-section">
        <div className="xp-bar-progress-header">
          <span className="xp-bar-progress-title">Level {level} Progress</span>
          <span className="xp-bar-progress-nums">
            {displayXP.toLocaleString()} / {xpToNextLevel.toLocaleString()} XP
          </span>
        </div>
        <div
          className="xp-bar-track xp-bar-track-lg"
          style={{ background: `${tierColor}20` }}
        >
          <div
            className="xp-bar-fill"
            style={{
              width: `${progress}%`,
              background: `linear-gradient(90deg, ${tierColor} 0%, ${tierColor}CC 100%)`,
            }}
          >
            <div className="xp-bar-shimmer" />
          </div>
        </div>
        <div className="xp-bar-remaining">
          {(xpToNextLevel - displayXP).toLocaleString()} XP to Level {level + 1}
        </div>
      </div>

      {/* Tier Progress */}
      {showTierProgress && tierProgress && tierProgress.tier_end_xp && (
        <div className="xp-bar-tier-progress">
          <div className="xp-bar-progress-header">
            <span className="xp-bar-progress-title">
              {tier.charAt(0).toUpperCase() + tier.slice(1)} Tier Progress
            </span>
            <span className="xp-bar-progress-nums">
              {Math.round(tierProgress.progress * 100)}%
            </span>
          </div>
          <div className="xp-bar-track" style={{ background: "var(--bg-tertiary)" }}>
            <div
              className="xp-bar-fill"
              style={{
                width: `${tierProgress.progress * 100}%`,
                background: `linear-gradient(90deg, ${tierColor}80 0%, ${tierColor} 100%)`,
              }}
            />
          </div>
          <div className="xp-bar-remaining">
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
    <button onClick={onClick} className="xp-indicator">
      <span style={{ fontSize: 18 }}>
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
      <div className="xp-indicator-info">
        <span className="xp-indicator-level">Lv.{level}</span>
        <div
          className="xp-indicator-bar"
          style={{ background: `${tierColor}30` }}
        >
          <div
            className="xp-indicator-fill"
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
