/**
 * LevelUpModal - Celebration modal when user levels up
 */

import { useEffect, useState } from "react";
import { TierBadge, TIER_COLORS, PrestigeTier } from "./TierBadge";

interface LevelUpModalProps {
  isOpen: boolean;
  onClose: () => void;
  newLevel: number;
  tier: PrestigeTier;
  tierChanged?: boolean;
  newTier?: PrestigeTier;
  xpGained: number;
}

export function LevelUpModal({
  isOpen,
  onClose,
  newLevel,
  tier,
  tierChanged = false,
  newTier,
  xpGained,
}: LevelUpModalProps) {
  const [showContent, setShowContent] = useState(false);
  const [showConfetti, setShowConfetti] = useState(false);

  useEffect(() => {
    if (isOpen) {
      // Stagger animations
      setTimeout(() => setShowContent(true), 100);
      setTimeout(() => setShowConfetti(true), 300);
    } else {
      setShowContent(false);
      setShowConfetti(false);
    }
  }, [isOpen]);

  if (!isOpen) return null;

  const displayTier = newTier || tier;
  const tierColor = TIER_COLORS[displayTier];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/80 backdrop-blur-sm"
        onClick={onClose}
      />

      {/* Confetti */}
      {showConfetti && <LevelUpConfetti tierColor={tierColor} />}

      {/* Modal */}
      <div
        className={`relative z-10 w-full max-w-md mx-4 rounded-2xl overflow-hidden transition-all duration-500 transform ${
          showContent ? "scale-100 opacity-100" : "scale-90 opacity-0"
        }`}
        style={{
          background: `linear-gradient(180deg, ${tierColor}20 0%, #1a1a1a 30%)`,
          border: `2px solid ${tierColor}`,
          boxShadow: `0 0 40px ${tierColor}40`,
        }}
      >
        {/* Glow effect */}
        <div
          className="absolute top-0 left-1/2 -translate-x-1/2 w-48 h-48 rounded-full blur-3xl"
          style={{ background: `${tierColor}30` }}
        />

        <div className="relative p-8 text-center">
          {/* Header */}
          <div className="mb-6">
            {tierChanged ? (
              <>
                <div className="text-sm text-gray-400 mb-2 uppercase tracking-wider">
                  Tier Promotion!
                </div>
                <h2
                  className="text-3xl font-bold mb-2"
                  style={{ color: tierColor }}
                >
                  Welcome to {displayTier.charAt(0).toUpperCase() + displayTier.slice(1)}!
                </h2>
              </>
            ) : (
              <>
                <div className="text-sm text-gray-400 mb-2 uppercase tracking-wider">
                  Level Up!
                </div>
                <h2 className="text-4xl font-bold text-white mb-2">
                  Level {newLevel}
                </h2>
              </>
            )}
          </div>

          {/* Level badge */}
          <div className="flex justify-center mb-6">
            <div
              className={`relative transition-all duration-700 ${
                showContent ? "scale-100" : "scale-0"
              }`}
            >
              <TierBadge tier={displayTier} level={newLevel} size="lg" />

              {/* Pulse ring */}
              <div
                className="absolute inset-0 rounded-full animate-ping"
                style={{
                  border: `2px solid ${tierColor}`,
                  animationDuration: "1.5s",
                }}
              />
            </div>
          </div>

          {/* XP gained */}
          <div className="mb-6">
            <div className="text-sm text-gray-400 mb-1">XP Earned</div>
            <div
              className="text-2xl font-bold"
              style={{ color: tierColor }}
            >
              +{xpGained.toLocaleString()} XP
            </div>
          </div>

          {/* Tier progress message */}
          {tierChanged && newTier && (
            <div
              className="mb-6 p-3 rounded-lg text-sm"
              style={{ background: `${tierColor}20` }}
            >
              <p style={{ color: tierColor }}>
                You've ascended to a new prestige tier! Your dedication is legendary.
              </p>
            </div>
          )}

          {/* Close button */}
          <button
            onClick={onClose}
            className="px-8 py-3 rounded-lg font-semibold text-white transition-all hover:scale-105"
            style={{
              background: `linear-gradient(135deg, ${tierColor} 0%, ${tierColor}CC 100%)`,
              boxShadow: `0 4px 15px ${tierColor}40`,
            }}
          >
            Awesome!
          </button>
        </div>
      </div>
    </div>
  );
}

// Simple confetti animation
function LevelUpConfetti({ tierColor }: { tierColor: string }) {
  const [particles, setParticles] = useState<
    { id: number; x: number; y: number; rotation: number; scale: number; color: string }[]
  >([]);

  useEffect(() => {
    const colors = [tierColor, "#fff", "#ffd700", "#ff6b6b", "#4ecdc4"];
    const newParticles = Array.from({ length: 50 }, (_, i) => ({
      id: i,
      x: Math.random() * 100,
      y: -10 - Math.random() * 20,
      rotation: Math.random() * 360,
      scale: 0.5 + Math.random() * 0.5,
      color: colors[Math.floor(Math.random() * colors.length)],
    }));
    setParticles(newParticles);
  }, [tierColor]);

  return (
    <div className="absolute inset-0 pointer-events-none overflow-hidden">
      {particles.map((p) => (
        <div
          key={p.id}
          className="absolute w-3 h-3 rounded-sm"
          style={{
            left: `${p.x}%`,
            top: `${p.y}%`,
            backgroundColor: p.color,
            transform: `rotate(${p.rotation}deg) scale(${p.scale})`,
            animation: `confetti-fall 3s ease-out forwards`,
            animationDelay: `${Math.random() * 0.5}s`,
          }}
        />
      ))}
      <style>{`
        @keyframes confetti-fall {
          0% {
            opacity: 1;
            transform: translateY(0) rotate(0deg);
          }
          100% {
            opacity: 0;
            transform: translateY(100vh) rotate(720deg);
          }
        }
      `}</style>
    </div>
  );
}

// Toast notification for achievement unlocks
export function AchievementUnlockToast({
  achievement,
  onClose,
}: {
  achievement: {
    name: string;
    icon: string;
    rarity: string;
    xp_reward: number;
  };
  onClose: () => void;
}) {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    setVisible(true);
    const timer = setTimeout(() => {
      setVisible(false);
      setTimeout(onClose, 300);
    }, 4000);
    return () => clearTimeout(timer);
  }, [onClose]);

  const rarityColors: Record<string, { border: string; bg: string; text: string }> = {
    common: { border: "#6b7280", bg: "rgba(107, 114, 128, 0.2)", text: "#9ca3af" },
    rare: { border: "#3b82f6", bg: "rgba(59, 130, 246, 0.2)", text: "#60a5fa" },
    epic: { border: "#a855f7", bg: "rgba(168, 85, 247, 0.2)", text: "#c084fc" },
    legendary: { border: "#f59e0b", bg: "rgba(245, 158, 11, 0.2)", text: "#fbbf24" },
  };

  const colors = rarityColors[achievement.rarity] || rarityColors.common;

  return (
    <div
      className={`fixed bottom-4 right-4 z-50 flex items-center gap-3 px-4 py-3 rounded-xl transition-all duration-300 ${
        visible ? "translate-x-0 opacity-100" : "translate-x-full opacity-0"
      }`}
      style={{
        background: colors.bg,
        border: `2px solid ${colors.border}`,
        boxShadow: `0 4px 20px ${colors.border}40`,
      }}
    >
      <span className="text-2xl">{achievement.icon}</span>
      <div>
        <div className="text-xs text-gray-400 uppercase tracking-wide">
          Achievement Unlocked!
        </div>
        <div className="font-semibold text-white">{achievement.name}</div>
        <div className="text-sm" style={{ color: colors.text }}>
          +{achievement.xp_reward} XP
        </div>
      </div>
      <button
        onClick={() => {
          setVisible(false);
          setTimeout(onClose, 300);
        }}
        className="ml-2 p-1 hover:bg-white/10 rounded transition-colors"
      >
        <svg className="w-4 h-4 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
        </svg>
      </button>
    </div>
  );
}
