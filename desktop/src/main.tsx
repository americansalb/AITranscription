import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { OverlayApp } from "./OverlayApp";
import { ToastProvider } from "./components/Toast";
import "./styles.css";

// Check if this is the overlay window
const isOverlay = window.location.hash === "#/overlay";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {isOverlay ? (
      <OverlayApp />
    ) : (
      <ToastProvider>
        <App />
      </ToastProvider>
    )}
  </React.StrictMode>
);
