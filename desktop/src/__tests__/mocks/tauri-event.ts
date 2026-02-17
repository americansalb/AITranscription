// Mock @tauri-apps/api/event for testing outside Tauri runtime
import { vi } from "vitest";

export const emit = vi.fn().mockResolvedValue(undefined);
export const listen = vi.fn().mockResolvedValue(vi.fn()); // returns unlisten fn
export const once = vi.fn().mockResolvedValue(vi.fn());
