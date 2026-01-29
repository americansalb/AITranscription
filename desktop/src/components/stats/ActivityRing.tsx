import { useEffect, useState } from 'react';

interface Ring {
  current: number;
  goal: number;
  color: string;
  label: string;
  icon?: string;
}

interface ActivityRingProps {
  rings: Ring[];
  size?: number;
  strokeWidth?: number;
  animated?: boolean;
}

/**
 * Apple Health-style concentric activity rings
 */
export function ActivityRing({
  rings,
  size = 120,
  strokeWidth = 10,
  animated = true,
}: ActivityRingProps) {
  const [animatedProgress, setAnimatedProgress] = useState<number[]>(
    animated ? rings.map(() => 0) : rings.map(r => Math.min(r.current / r.goal, 1))
  );

  useEffect(() => {
    if (!animated) {
      setAnimatedProgress(rings.map(r => Math.min(r.current / r.goal, 1)));
      return;
    }

    // Animate each ring sequentially
    const targetProgress = rings.map(r => Math.min(r.current / r.goal, 1));
    const duration = 1200;
    const startTime = performance.now();

    const animate = (currentTime: number) => {
      const elapsed = currentTime - startTime;
      const progress = Math.min(elapsed / duration, 1);

      // Ease-out cubic
      const eased = 1 - Math.pow(1 - progress, 3);

      setAnimatedProgress(targetProgress.map(target => target * eased));

      if (progress < 1) {
        requestAnimationFrame(animate);
      }
    };

    requestAnimationFrame(animate);
  }, [rings, animated]);

  const center = size / 2;
  const gap = strokeWidth + 4; // Gap between rings

  return (
    <div className="activity-ring-container" style={{ width: size, height: size }}>
      <svg
        viewBox={`0 0 ${size} ${size}`}
        className="activity-ring-svg"
      >
        {rings.map((ring, index) => {
          const radius = center - strokeWidth / 2 - (index * gap);
          const circumference = 2 * Math.PI * radius;
          const progress = animatedProgress[index] || 0;
          const strokeDashoffset = circumference * (1 - progress);

          return (
            <g key={index}>
              {/* Background ring */}
              <circle
                cx={center}
                cy={center}
                r={radius}
                fill="none"
                stroke="var(--bg-tertiary)"
                strokeWidth={strokeWidth}
                opacity={0.3}
              />
              {/* Progress ring */}
              <circle
                cx={center}
                cy={center}
                r={radius}
                fill="none"
                stroke={ring.color}
                strokeWidth={strokeWidth}
                strokeLinecap="round"
                strokeDasharray={circumference}
                strokeDashoffset={strokeDashoffset}
                transform={`rotate(-90 ${center} ${center})`}
                className="activity-ring-progress"
                style={{
                  filter: progress >= 1 ? `drop-shadow(0 0 6px ${ring.color})` : 'none',
                }}
              />
            </g>
          );
        })}
      </svg>

      {/* Center content */}
      <div className="activity-ring-center">
        {rings.length === 1 ? (
          <>
            <div className="ring-percentage">
              {Math.round(animatedProgress[0] * 100)}%
            </div>
            <div className="ring-label">{rings[0].label}</div>
          </>
        ) : (
          <div className="ring-multi-center">
            {rings[0].icon || '📊'}
          </div>
        )}
      </div>
    </div>
  );
}

interface TodayProgressProps {
  wordsToday: number;
  wordsGoal: number;
  transcriptionsToday: number;
  transcriptionsGoal: number;
  minutesActive: number;
  minutesGoal: number;
}

/**
 * Today's progress with three activity rings
 */
export function TodayProgress({
  wordsToday,
  wordsGoal,
  transcriptionsToday,
  transcriptionsGoal,
  minutesActive,
  minutesGoal,
}: TodayProgressProps) {
  const rings: Ring[] = [
    {
      current: wordsToday,
      goal: wordsGoal,
      color: '#6366f1', // Indigo
      label: 'Words',
      icon: '📝',
    },
    {
      current: transcriptionsToday,
      goal: transcriptionsGoal,
      color: '#22c55e', // Green
      label: 'Transcriptions',
      icon: '🎤',
    },
    {
      current: minutesActive,
      goal: minutesGoal,
      color: '#f59e0b', // Amber
      label: 'Minutes',
      icon: '⏱️',
    },
  ];

  return (
    <div className="today-progress">
      <h3 className="today-progress-title">Today's Progress</h3>
      <div className="today-progress-content">
        <ActivityRing rings={rings} size={140} strokeWidth={12} />
        <div className="today-progress-legend">
          {rings.map((ring, index) => {
            const progress = Math.min(ring.current / ring.goal, 1);
            const percentage = Math.round(progress * 100);
            return (
              <div key={index} className="legend-item">
                <span
                  className="legend-dot"
                  style={{ backgroundColor: ring.color }}
                />
                <span className="legend-label">{ring.label}</span>
                <span className="legend-value">
                  {ring.current} / {ring.goal}
                </span>
                <span className="legend-percentage">({percentage}%)</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

interface SingleRingProps {
  current: number;
  goal: number;
  label: string;
  color?: string;
  size?: number;
}

/**
 * Single progress ring for focused display
 */
export function SingleRing({
  current,
  goal,
  label,
  color = '#6366f1',
  size = 80,
}: SingleRingProps) {
  return (
    <ActivityRing
      rings={[{ current, goal, color, label }]}
      size={size}
      strokeWidth={8}
    />
  );
}
