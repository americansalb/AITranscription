import ReactDOM from "react-dom/client";
import App from "./App";
import { OverlayApp } from "./OverlayApp";
import { TranscriptApp } from "./TranscriptApp";
import { ScreenReaderApp } from "./ScreenReaderApp";
import { QueueApp } from "./QueueApp";
import { ToastProvider } from "./components/Toast";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./styles.css";

// Block browser-style shortcuts that conflict with app functionality.
// Cmd+R / Ctrl+R refreshes the webview, re-initializing all listeners and
// replaying cached speak events — which looks like a false screen reader trigger.
document.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey) {
    if (e.key === "r" || e.key === "R") {
      e.preventDefault();
    }
  }
});

// Check window type from hash
const hash = window.location.hash;
const isOverlay = hash === "#/overlay";
const isTranscript = hash === "#/transcript";
const isScreenReader = hash === "#/screen-reader";
const isQueue = hash === "#/queue";

// Disabled StrictMode to prevent duplicate event listener registration
ReactDOM.createRoot(document.getElementById("root")!).render(
  <>
    {isOverlay ? (
      <OverlayApp />
    ) : isTranscript ? (
      <ErrorBoundary fallbackLabel="The Claude integration panel encountered an error.">
        <TranscriptApp />
      </ErrorBoundary>
    ) : isScreenReader ? (
      <ErrorBoundary fallbackLabel="The screen reader settings encountered an error.">
        <ScreenReaderApp />
      </ErrorBoundary>
    ) : isQueue ? (
      <QueueApp />
    ) : (
      <ErrorBoundary fallbackLabel="The main application encountered an error.">
        <ToastProvider>
          <App />
        </ToastProvider>
      </ErrorBoundary>
    )}
  </>
);
