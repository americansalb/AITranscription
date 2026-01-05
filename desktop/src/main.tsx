import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { FloatingOverlay } from "./components/FloatingOverlay";
import "./styles.css";

// Simple hash-based routing for multi-window support
function Router() {
  const hash = window.location.hash;

  // The overlay window uses #/overlay route
  if (hash === "#/overlay") {
    return <FloatingOverlay />;
  }

  // Default: main app
  return <App />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Router />
  </React.StrictMode>
);
