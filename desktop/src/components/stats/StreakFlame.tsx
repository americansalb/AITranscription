import { useMemo } from 'react';

interface StreakFlameProps {
  currentStreak: number;
  bestStreak: number;
  hasActivityToday?: boolean;
}

/**
 * Duolingo-style streak display with animated flame
 */
export function StreakFlame({
  currentStreak,
  bestStreak,
  hasActivityToday = true,
}: StreakFlameProps) {
  // Determine flame intensity based on streak length
  const intensity = useMemo(() => {
    if (currentStreak === 0) return 'none';
    if (currentStreak < 3) return 'low';
    if (currentStreak < 7) return 'medium';
    if (currentStreak < 14) return 'high';
    if (currentStreak < 30) return 'intense';
    return 'legendary';
  }, [currentStreak]);

  const isAtRisk = !hasActivityToday && currentStreak > 0;
  const isNewRecord = currentStreak > 0 && currentStreak >= bestStreak;

  return (
    <div className={`streak-flame-container ${isAtRisk ? 'at-risk' : ''}`}>
      <div className={`streak-flame flame-${intensity}`}>
        <FlameIcon intensity={intensity} />
        <div className="streak-count">{currentStreak}</div>
      </div>

      <div className="streak-info">
        <div className="streak-label">
          {currentStreak === 0 ? (
            'Start your streak today!'
          ) : currentStreak === 1 ? (
            '1 day streak!'
          ) : (
            `${currentStreak} day streak!`
          )}
        </div>

        {bestStreak > 0 && currentStreak > 0 && (
          <div className="streak-best">
            {isNewRecord ? (
              <span className="new-record">New record!</span>
            ) : (
              <>Best: {bestStreak} days</>
            )}
          </div>
        )}

        {isAtRisk && (
          <div className="streak-warning">
            Transcribe today to keep your streak!
          </div>
        )}
      </div>
    </div>
  );
}

interface FlameIconProps {
  intensity: 'none' | 'low' | 'medium' | 'high' | 'intense' | 'legendary';
}

function FlameIcon({ intensity }: FlameIconProps) {
  if (intensity === 'none') {
    return (
      <svg
        className="flame-svg flame-empty"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      >
        <path d="M12 2C8.5 6 4 9 4 14a8 8 0 0 0 16 0c0-5-4.5-8-8-12z" />
      </svg>
    );
  }

  return (
    <svg className="flame-svg" viewBox="0 0 24 24" fill="currentColor">
      <defs>
        <linearGradient id="flameGradient" x1="0%" y1="100%" x2="0%" y2="0%">
          <stop offset="0%" stopColor="#f97316" />
          <stop offset="50%" stopColor="#ef4444" />
          <stop offset="100%" stopColor="#fbbf24" />
        </linearGradient>
      </defs>
      <path
        d="M12 2C8.5 6 4 9 4 14a8 8 0 0 0 16 0c0-5-4.5-8-8-12z"
        fill="url(#flameGradient)"
      />
      {/* Inner flame for intense/legendary */}
      {(intensity === 'intense' || intensity === 'legendary') && (
        <path
          d="M12 8C10 10 8 12 8 15a4 4 0 0 0 8 0c0-3-2-5-4-7z"
          fill="#fbbf24"
          className="inner-flame"
        />
      )}
    </svg>
  );
}

interface StreakBadgeProps {
  streak: number;
  compact?: boolean;
}

/**
 * Compact streak badge for inline display
 */
export function StreakBadge({ streak, compact = false }: StreakBadgeProps) {
  if (streak === 0) return null;

  return (
    <span className={`streak-badge ${compact ? 'streak-badge-compact' : ''}`}>
      <span className="streak-badge-flame">🔥</span>
      <span className="streak-badge-count">{streak}</span>
      {!compact && <span className="streak-badge-label">day{streak !== 1 ? 's' : ''}</span>}
    </span>
  );
}
