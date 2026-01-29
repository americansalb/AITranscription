/**
 * Gamification components barrel export
 */

export { TierBadge, TierIcon, TIER_COLORS, TIER_ICONS, TIER_GRADIENTS } from "./TierBadge";
export type { PrestigeTier } from "./TierBadge";

export { XPBar, XPIndicator } from "./XPBar";

export {
  AchievementCard,
  AchievementBadge,
  RARITY_COLORS,
  RARITY_LABELS,
  getIconSvg,
} from "./AchievementCard";
export type { Achievement, AchievementRarity } from "./AchievementCard";

export { AchievementBrowser } from "./AchievementBrowser";

export { LevelUpModal, AchievementUnlockToast } from "./LevelUpModal";
