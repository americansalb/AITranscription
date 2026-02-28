/**
 * ErrorBanner — consistent error display component.
 * Shows user-friendly error message with optional retry action.
 * Replaces the inconsistent error handling patterns from the desktop app.
 */

interface ErrorBannerProps {
  message: string;
  onRetry?: () => void;
  onDismiss?: () => void;
}

export function ErrorBanner({ message, onRetry, onDismiss }: ErrorBannerProps) {
  return (
    <div
      role="alert"
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-3)",
        padding: "var(--space-3) var(--space-4)",
        background: "var(--error-muted)",
        borderRadius: "var(--radius-sm)",
        border: "1px solid rgba(239, 68, 68, 0.3)",
      }}
    >
      <span style={{ color: "var(--error)", fontSize: "var(--text-sm)", flex: 1 }}>
        {message}
      </span>
      {onRetry && (
        <button
          className="btn btn-ghost"
          onClick={onRetry}
          style={{ fontSize: "var(--text-xs)", color: "var(--error)" }}
        >
          Retry
        </button>
      )}
      {onDismiss && (
        <button
          className="btn btn-ghost"
          onClick={onDismiss}
          aria-label="Dismiss error"
          style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)", padding: "2px" }}
        >
          {"\u2715"}
        </button>
      )}
    </div>
  );
}
