/**
 * AchievementCard - Achievement display with clear earned/locked states
 */

import { useMemo } from "react";
import { GamificationAchievement, AchievementRarity } from "../../lib/api";

export type { AchievementRarity };
export type Achievement = GamificationAchievement;

interface AchievementCardProps {
  achievement: GamificationAchievement;
  compact?: boolean;
  onClick?: () => void;
}

const RARITY_CONFIG: Record<AchievementRarity, { accent: string; glow: string; label: string; dot: string; bg: string; text: string }> = {
  common:    { accent: "#71717a", glow: "rgba(113,113,122,0.15)", label: "Common",    dot: "#71717a", bg: "rgba(113,113,122,0.15)", text: "#71717a" },
  rare:      { accent: "#3b82f6", glow: "rgba(59,130,246,0.18)",  label: "Rare",      dot: "#3b82f6", bg: "rgba(59,130,246,0.18)",  text: "#3b82f6" },
  epic:      { accent: "#a855f7", glow: "rgba(168,85,247,0.20)",  label: "Epic",      dot: "#a855f7", bg: "rgba(168,85,247,0.20)",  text: "#a855f7" },
  legendary: { accent: "#f59e0b", glow: "rgba(245,158,11,0.25)",  label: "Legendary", dot: "#f59e0b", bg: "rgba(245,158,11,0.25)",  text: "#f59e0b" },
};

// Category-based SVG icons (clean, consistent style)
const CATEGORY_ICONS: Record<string, JSX.Element> = {
  volume: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"/></svg>,
  streak: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z"/></svg>,
  speed: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>,
  context: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>,
  formality: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/></svg>,
  learning: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z"/><path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z"/></svg>,
  temporal: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>,
  records: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 9H4.5a2.5 2.5 0 0 1 0-5C7.357 4 9 7 12 7s4.643-3 7.5-3a2.5 2.5 0 0 1 0 5H18"/><path d="M12 7v10"/><path d="M8 17h8l-4 4-4-4z"/></svg>,
  combo: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>,
  special: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="8" r="6"/><path d="M15.477 12.89 17 22l-5-3-5 3 1.523-9.11"/></svg>,
  consistency: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="4" width="18" height="18" rx="2" ry="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/><path d="m9 16 2 2 4-4"/></svg>,
};

const DEFAULT_ICON = <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/></svg>;

function formatVal(v: number): string {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`.replace(".0M", "M");
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`.replace(".0K", "K");
  return v.toLocaleString();
}

export function AchievementCard({ achievement, compact: _compact, onClick }: AchievementCardProps) {
  const cfg = RARITY_CONFIG[achievement.rarity];
  const locked = !achievement.is_unlocked;
  const hidden = achievement.is_hidden && locked;
  const pct = useMemo(() => Math.min(achievement.progress * 100, 100), [achievement.progress]);

  const categoryIcon = hidden ? null : (CATEGORY_ICONS[achievement.category] || DEFAULT_ICON);

  return (
    <div
      className={`ach-card ${locked ? "ach-locked" : "ach-earned"}`}
      onClick={onClick}
      style={{
        "--ach-accent": cfg.accent,
        "--ach-glow": cfg.glow,
      } as React.CSSProperties}
    >
      {/* Legendary shimmer */}
      {achievement.rarity === "legendary" && !locked && <div className="ach-shimmer" />}

      <div className="ach-top">
        <span className="ach-icon-svg">
          {hidden
            ? <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
            : categoryIcon
          }
        </span>
        <span className="ach-rarity-label" style={{ color: cfg.dot }}>
          <span className="ach-rarity-dot" style={{ background: cfg.dot }} />
          {cfg.label}
        </span>
      </div>

      <div className="ach-name">
        {!locked && <span className="ach-check-badge">✓</span>}
        {hidden ? "???" : achievement.name}
      </div>
      <div className="ach-desc">{hidden ? "Hidden achievement" : achievement.description}</div>

      {!hidden && (
        <div className="ach-progress-wrap">
          <div className="ach-progress-bar">
            <div
              className={`ach-progress-fill ${!locked ? "ach-progress-complete" : ""}`}
              style={{ width: `${pct}%` }}
            />
          </div>
          <div className="ach-progress-label">
            <span>{formatVal(achievement.current_value)} / {formatVal(achievement.threshold)}</span>
            <span className="ach-xp">+{achievement.xp_reward} XP</span>
          </div>
        </div>
      )}
    </div>
  );
}

// Badge for toast notifications
export function AchievementBadge({ achievement }: { achievement: Achievement }) {
  const cfg = RARITY_CONFIG[achievement.rarity];
  const icon = CATEGORY_ICONS[achievement.category] || DEFAULT_ICON;
  return (
    <div className="ach-badge" style={{ borderColor: cfg.accent, background: cfg.glow }}>
      <span className="ach-badge-icon">{icon}</span>
      <span style={{ color: cfg.accent, fontWeight: 600 }}>{achievement.name}</span>
    </div>
  );
}

export { RARITY_CONFIG as RARITY_COLORS };
const RARITY_LABELS: Record<AchievementRarity, string> = { common: "Common", rare: "Rare", epic: "Epic", legendary: "Legendary" };
export { RARITY_LABELS };
export function getIconSvg() { return <span />; }
