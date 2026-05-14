import ReactDOM from "react-dom/client";
import App from "./App";
import { OverlayApp } from "./OverlayApp";
import { TranscriptApp } from "./TranscriptApp";
import { ScreenReaderApp } from "./ScreenReaderApp";
import { QueueApp } from "./QueueApp";
import { CollaborateV2App } from "./components/CollaborateV2/CollaborateV2App";
import { ToastProvider } from "./components/Toast";
import { ErrorBoundary } from "./components/ErrorBoundary";
// tokens.css must load BEFORE styles.css so design tokens cascade properly
// into the rest of the stylesheet (Wave 1 of typed-css-spec 584568b).
import "./styles/tokens.css";
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
const isCollaborateV2 = hash === "#/collaborate-v2";

// Disabled StrictMode to prevent duplicate event listener registration.
// ToastProvider wraps the entire route switch so any route — current or
// future — can call useToast without recreating the c43f917 regression
// (TranscriptApp had no provider in scope after CollabTab started calling
// useToast in 8f2b97a).
ReactDOM.createRoot(document.getElementById("root")!).render(
  <ToastProvider>
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
    ) : isCollaborateV2 ? (
      <ErrorBoundary fallbackLabel="The Collaborate v2 window encountered an error.">
        <CollaborateV2App />
      </ErrorBoundary>
    ) : (
      <ErrorBoundary fallbackLabel="The main application encountered an error.">
        <App />
      </ErrorBoundary>
    )}
  </ToastProvider>
);
