import { Component, type ReactNode, type ErrorInfo } from "react";

interface Props {
  children: ReactNode;
  fallbackLabel?: string;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("[ErrorBoundary] Caught error:", error, errorInfo);
  }

  handleReload = () => {
    this.setState({ hasError: false, error: null });
  };

  render() {
    if (this.state.hasError) {
      return (
        <div style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          height: "100%",
          minHeight: 200,
          gap: 16,
          padding: 32,
          color: "#a1a1aa",
          textAlign: "center",
        }}>
          <div style={{ fontSize: 32 }}>Something went wrong</div>
          <div style={{ fontSize: 14, maxWidth: 400, lineHeight: 1.5 }}>
            {this.props.fallbackLabel || "An unexpected error occurred."}
            {this.state.error && (
              <div style={{
                marginTop: 8,
                padding: "8px 12px",
                background: "rgba(239, 68, 68, 0.1)",
                border: "1px solid rgba(239, 68, 68, 0.3)",
                borderRadius: 6,
                fontSize: 12,
                color: "#ef4444",
                fontFamily: "monospace",
                wordBreak: "break-word",
              }}>
                {this.state.error.message}
              </div>
            )}
          </div>
          <button
            onClick={this.handleReload}
            style={{
              padding: "10px 24px",
              background: "rgba(99, 102, 241, 0.2)",
              color: "#818cf8",
              border: "1px solid rgba(99, 102, 241, 0.4)",
              borderRadius: 8,
              fontSize: 14,
              fontWeight: 600,
              cursor: "pointer",
            }}
          >
            Try Again
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
