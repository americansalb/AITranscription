interface TrendIndicatorProps {
  value: number;
  previousValue: number;
  format?: 'percentage' | 'absolute';
  showLabel?: boolean;
  size?: 'sm' | 'md' | 'lg';
}

/**
 * Trend indicator showing up/down arrows with percentage change
 */
export function TrendIndicator({
  value,
  previousValue,
  format = 'percentage',
  showLabel = true,
  size = 'md',
}: TrendIndicatorProps) {
  if (previousValue === 0) {
    if (value > 0) {
      return (
        <span className={`trend-indicator trend-up trend-${size}`} title="New this period!">
          <ArrowUp />
          {showLabel && <span className="trend-label">New</span>}
        </span>
      );
    }
    return null;
  }

  const percentChange = ((value - previousValue) / previousValue) * 100;
  const absoluteChange = value - previousValue;
  const isUp = percentChange > 0;
  const isSame = Math.abs(percentChange) < 1;

  if (isSame) {
    return (
      <span className={`trend-indicator trend-same trend-${size}`} title="About the same">
        <span className="trend-dash">-</span>
        {showLabel && <span className="trend-label">Same</span>}
      </span>
    );
  }

  const displayValue = format === 'percentage'
    ? `${Math.round(Math.abs(percentChange))}%`
    : Math.abs(absoluteChange).toLocaleString();

  const tooltip = isUp
    ? `${displayValue} more than before`
    : `${displayValue} less than before`;

  return (
    <span
      className={`trend-indicator ${isUp ? 'trend-up' : 'trend-down'} trend-${size}`}
      title={tooltip}
    >
      {isUp ? <ArrowUp /> : <ArrowDown />}
      {showLabel && <span className="trend-label">{displayValue}</span>}
    </span>
  );
}

function ArrowUp() {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 12 12"
      fill="none"
      className="trend-arrow"
    >
      <path
        d="M6 2.5L10 6.5L8.59 7.91L6 5.33L3.41 7.91L2 6.5L6 2.5Z"
        fill="currentColor"
      />
    </svg>
  );
}

function ArrowDown() {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 12 12"
      fill="none"
      className="trend-arrow"
    >
      <path
        d="M6 9.5L2 5.5L3.41 4.09L6 6.67L8.59 4.09L10 5.5L6 9.5Z"
        fill="currentColor"
      />
    </svg>
  );
}

interface TrendBadgeProps {
  thisWeek: number;
  lastWeek: number;
  label?: string;
}

/**
 * Compact trend badge for inline display
 */
export function TrendBadge({ thisWeek, lastWeek, label }: TrendBadgeProps) {
  if (lastWeek === 0) return null;

  const percentChange = ((thisWeek - lastWeek) / lastWeek) * 100;
  const rounded = Math.round(Math.abs(percentChange));

  if (rounded < 5) return null;

  const isUp = percentChange > 0;

  return (
    <span className={`trend-badge ${isUp ? 'trend-badge-up' : 'trend-badge-down'}`}>
      {isUp ? '↑' : '↓'} {rounded}%
      {label && <span className="trend-badge-label"> {label}</span>}
    </span>
  );
}
