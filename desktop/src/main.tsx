import ReactDOM from "react-dom/client";
import App from "./App";
import { OverlayApp } from "./OverlayApp";
import { TranscriptApp } from "./TranscriptApp";
import { ScreenReaderApp } from "./ScreenReaderApp";
import { QueueApp } from "./QueueApp";
import { CollaborateV2App } from "./components/CollaborateV2/CollaborateV2App";
import { lazy, Suspense } from "react";
// UI2 ("One Window" decree, board msg 210) — lazy so the old surface pays
// zero cost until cutover. Lives at #/ui2 inside the existing main window.
const Ui2App = lazy(() => import("./ui2/Ui2App"));
import { ToastProvider } from "./components/Toast";
import { ErrorBoundary } from "./components/ErrorBoundary";
// tokens.css must load BEFORE styles.css so design tokens cascade properly
// into the rest of the stylesheet (Wave 1 of typed-css-spec 584568b).
import "./styles/tokens.css";
import "./styles.css";

// SHA-11.5b: frontend fingerprint protocol. vite.config.ts injects these at
// build time (define plugin); they survive minification as bare string
// literals. Exposing on `window` makes them inspectable from devtools +
// grep-able in dist/assets/*.js. Symmetric to the Rust VAAK_FP statics.
declare const __VAAK_FRONTEND_FP_SHA__: string;
declare const __VAAK_FRONTEND_FP_BUILT_AT__: string;
declare const __VAAK_FP_FRONTEND__: string;
(window as unknown as Record<string, unknown>).__VAAK_FRONTEND_FP = {
  sha: __VAAK_FRONTEND_FP_SHA__,
  built_at: __VAAK_FRONTEND_FP_BUILT_AT__,
  marker: __VAAK_FP_FRONTEND__,
};
// Console-log for terminal-tail visibility in dev mode.
// eslint-disable-next-line no-console
console.info(__VAAK_FP_FRONTEND__, "built_at=" + __VAAK_FRONTEND_FP_BUILT_AT__);

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

// Trial-period switch (cutover card #380, human chose "trial — both UIs
// live"): Ctrl+Shift+U toggles between the old surface and One Window.
// Hash is only read at boot, so a reload is required; remove at §6 cutover.
// Capture phase + e.code: deeper handlers can't swallow it, and the physical
// key matches regardless of keyboard layout (human report msg 420).
document.addEventListener(
  "keydown",
  (e) => {
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.code === "KeyU" || e.key === "U" || e.key === "u")) {
      e.preventDefault();
      e.stopPropagation();
      window.location.hash = window.location.hash === "#/ui2" ? "" : "#/ui2";
      window.location.reload();
    }
  },
  true,
);

// Check window type from hash
const hash = window.location.hash;
const isOverlay = hash === "#/overlay";
const isTranscript = hash === "#/transcript";
const isScreenReader = hash === "#/screen-reader";
const isQueue = hash === "#/queue";
const isCollaborateV2 = hash === "#/collaborate-v2";
const isUi2 = hash === "#/ui2";

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
    ) : isUi2 ? (
      <ErrorBoundary fallbackLabel="The One Window surface encountered an error.">
        <Suspense fallback={null}>
          <Ui2App />
        </Suspense>
      </ErrorBoundary>
    ) : (
      <ErrorBoundary fallbackLabel="The main application encountered an error.">
        <App />
      </ErrorBoundary>
    )}
  </ToastProvider>
);
