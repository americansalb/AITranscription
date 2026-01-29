import { useEffect, useState, useRef } from 'react';

interface AnimatedCounterProps {
  value: number;
  duration?: number; // Animation duration in ms
  format?: (value: number) => string;
  className?: string;
  prefix?: string;
  suffix?: string;
}

/**
 * Animated counter that rolls up numbers with easing
 */
export function AnimatedCounter({
  value,
  duration = 1500,
  format,
  className = '',
  prefix = '',
  suffix = '',
}: AnimatedCounterProps) {
  const [displayValue, setDisplayValue] = useState(0);
  const previousValue = useRef(0);
  const animationRef = useRef<number | null>(null);

  useEffect(() => {
    const startValue = previousValue.current;
    const endValue = value;
    const startTime = performance.now();

    // Easing function: ease-out-cubic
    const easeOutCubic = (t: number): number => {
      return 1 - Math.pow(1 - t, 3);
    };

    const animate = (currentTime: number) => {
      const elapsed = currentTime - startTime;
      const progress = Math.min(elapsed / duration, 1);
      const easedProgress = easeOutCubic(progress);

      const currentValue = startValue + (endValue - startValue) * easedProgress;
      setDisplayValue(Math.round(currentValue));

      if (progress < 1) {
        animationRef.current = requestAnimationFrame(animate);
      } else {
        previousValue.current = endValue;
      }
    };

    animationRef.current = requestAnimationFrame(animate);

    return () => {
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current);
      }
    };
  }, [value, duration]);

  const formattedValue = format ? format(displayValue) : displayValue.toLocaleString();

  return (
    <span className={`animated-counter ${className}`}>
      {prefix}
      {formattedValue}
      {suffix}
    </span>
  );
}

/**
 * Format duration in hours and minutes
 */
export function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);

  if (hours === 0) {
    return `${mins}m`;
  }
  if (mins === 0) {
    return `${hours}h`;
  }
  return `${hours}h ${mins}m`;
}

/**
 * Format large numbers with K/M suffix
 */
export function formatLargeNumber(num: number): string {
  if (num < 1000) return num.toString();
  if (num < 10000) return num.toLocaleString();
  if (num < 1000000) return `${(num / 1000).toFixed(1)}K`;
  return `${(num / 1000000).toFixed(1)}M`;
}
