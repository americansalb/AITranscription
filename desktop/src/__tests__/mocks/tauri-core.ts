// Mock @tauri-apps/api/core for testing outside Tauri runtime
import { vi } from "vitest";

export const invoke = vi.fn().mockResolvedValue(undefined);
