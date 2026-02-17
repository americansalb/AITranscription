import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@tauri-apps/api/event": resolve(__dirname, "src/__tests__/mocks/tauri-event.ts"),
      "@tauri-apps/api/core": resolve(__dirname, "src/__tests__/mocks/tauri-core.ts"),
      "@tauri-apps/plugin-clipboard-manager": resolve(__dirname, "src/__tests__/mocks/tauri-core.ts"),
      "@tauri-apps/plugin-dialog": resolve(__dirname, "src/__tests__/mocks/tauri-core.ts"),
      "@tauri-apps/plugin-global-shortcut": resolve(__dirname, "src/__tests__/mocks/tauri-core.ts"),
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/__tests__/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
