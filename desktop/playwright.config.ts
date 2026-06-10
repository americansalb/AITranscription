import { defineConfig } from "@playwright/test";

// Browser-mode e2e for ui2 (register ②③). Runs real Chromium against the
// vite dev server; Tauri IPC mocked at window.__TAURI_INTERNALS__ — see
// e2e/tauriMock.ts for the recorded limitation.
export default defineConfig({
  testDir: "./e2e",
  timeout: 60_000,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:14211",
    viewport: { width: 1280, height: 900 },
  },
  // production bundle, not the dev server: §7 numbers measured on minified
  // React in production mode (dev mode missed the 1s bar by ~20% — that is
  // dev-server overhead, not the shipped surface). Dedicated port so a
  // running Tauri dev server on 1420 is never silently reused.
  webServer: {
    command: "npm run build && npm run preview -- --port 14211 --strictPort",
    url: "http://localhost:14211",
    reuseExistingServer: true,
    timeout: 180_000,
  },
});
