/**
 * LoadingSpinner — consistent loading state with optional timeout.
 * Replaces the desktop pattern of spinners that spin forever without timeout.
 */

import { useEffect, useState } from "react";

interface LoadingSpinnerProps {
  label?: string;
  /** If set, shows a warning after this many seconds */
  timeoutSeconds?: number;
  timeoutMessage?: string;
}

export function LoadingSpinner({
  label = "Loading...",
  timeoutSeconds,
  timeoutMessage = "This is taking longer than expected.",
}: LoadingSpinnerProps) {
  const [timedOut, setTimedOut] = useState(false);

  useEffect(() => {
    if (!timeoutSeconds) return;
    const timer = setTimeout(() => setTimedOut(true), timeoutSeconds * 1000);
    return () => clearTimeout(timer);
  }, [timeoutSeconds]);

  return (
    <div className="loading-overlay" role="status" aria-label={label}>
      <div className="spinner" />
      <span>{label}</span>
      {timedOut && (
        <span style={{ fontSize: "var(--text-xs)", color: "var(--warning)" }}>
          {timeoutMessage}
        </span>
      )}
    </div>
  );
}
