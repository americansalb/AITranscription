/**
 * ErrorBoundary — catches React rendering errors gracefully.
 * Prevents a single component crash from taking down the entire app.
 * Must be a class component (React limitation for error boundaries).
 */

import { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false, error: null };

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: { componentStack?: string | null }) {
    console.error("[ErrorBoundary] Caught:", error, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) return this.props.fallback;
      return (
        <div
          role="alert"
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            padding: "var(--space-8)",
            gap: "var(--space-3)",
            textAlign: "center",
          }}
        >
          <div style={{ fontSize: 48, opacity: 0.3 }}>{"\u26A0\uFE0F"}</div>
          <div style={{ fontSize: "var(--text-lg)", fontWeight: "var(--weight-semibold)", color: "var(--text-secondary)" }}>
            Something went wrong
          </div>
          <div style={{ fontSize: "var(--text-sm)", color: "var(--text-muted)", maxWidth: 400 }}>
            {this.state.error?.message || "An unexpected error occurred."}
          </div>
          <button
            className="btn btn-primary"
            onClick={() => this.setState({ hasError: false, error: null })}
          >
            Try Again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
