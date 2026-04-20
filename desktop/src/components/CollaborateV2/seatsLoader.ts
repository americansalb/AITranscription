import type { Seat, SeatsFile } from "./types";

// P1: one-time read of .vaak/v2/seats.json on mount.
// No live subscriptions yet — that's P3a.
// Returns an empty roster if the file doesn't exist, matching the §18 P1
// "static roster with static state" deliverable.
export async function loadSeatsOnce(projectDir: string): Promise<Seat[]> {
  if (!window.__TAURI__) return [];
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const payload = await invoke<SeatsFile>("get_v2_seats", { projectDir });
    return payload?.seats ?? [];
  } catch {
    return [];
  }
}
