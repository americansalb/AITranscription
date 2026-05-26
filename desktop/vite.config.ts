import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { execSync } from "node:child_process";

// SHA-11.5b: frontend fingerprint protocol (tester msg 1300 + evil-arch msg 1295).
// Symmetric to SHA-11.5 Rust-side static byte arrays — TS commits had no
// post-build verification path because comments strip in vite minification.
// At build time, capture git HEAD short-sha + ISO timestamp and inject as
// global constants. The bundled JS contains the literal strings; runtime
// `window.__VAAK_FRONTEND_FP` exposes them for in-browser inspection +
// `findstr` grep of dist/assets/*.js for the SHA.
function frontendFingerprint() {
  let sha = "unknown";
  try {
    sha = execSync("git rev-parse --short=7 HEAD", { encoding: "utf8" }).trim();
  } catch {
    // git unavailable or not a repo — fingerprint becomes "unknown".
  }
  return {
    sha,
    builtAt: new Date().toISOString(),
  };
}

const fp = frontendFingerprint();

export default defineConfig({
  plugins: [react()],

  define: {
    __VAAK_FRONTEND_FP_SHA__: JSON.stringify(fp.sha),
    __VAAK_FRONTEND_FP_BUILT_AT__: JSON.stringify(fp.builtAt),
    // VAAK_FP_FRONTEND literal embedded for grep parity with Rust binaries.
    // Format mirrors VAAK_FP:<sha>:SHA-X.Y:<file>:<feature> Rust convention.
    __VAAK_FP_FRONTEND__: JSON.stringify(
      `VAAK_FP:${fp.sha}:SHA-11.5b:frontend:vite_build`,
    ),
  },

  // Prevent vite from obscuring Rust errors
  clearScreen: false,

  // Tauri expects a fixed port
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
