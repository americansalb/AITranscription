// Liveness derivation — cognition ≠ connection (warm-zombie bug class,
// recorded 2026-06-04). Derived from the session records agents write;
// no UI-side tracker. One source per fact:
//   connection = last_heartbeat · cognition = last_working_at
import type { ParsedProject, SeatDot, SeatLiveness } from "./types";

export const WORKING_FRESH_MS = 5 * 60 * 1000;
export const ALIVE_FRESH_MS = 3 * 60 * 1000;

function age(iso: string | null | undefined, now: number): number {
  if (!iso) return Number.POSITIVE_INFINITY;
  const t = Date.parse(iso);
  return Number.isFinite(t) ? now - t : Number.POSITIVE_INFINITY;
}

export function seatLiveness(
  lastHeartbeat: string | null | undefined,
  lastWorkingAt: string | null | undefined,
  now: number,
): SeatLiveness {
  const alive = age(lastHeartbeat, now) <= ALIVE_FRESH_MS;
  const working = age(lastWorkingAt, now) <= WORKING_FRESH_MS;
  if (!alive) return "dead";
  return working ? "working" : "warm-zombie";
}

export function deriveSeatDots(project: ParsedProject, now: number): SeatDot[] {
  const dots: SeatDot[] = [];
  const bindings = project.sessions ?? [];
  for (const status of project.role_statuses ?? []) {
    const roleSlug = (status as { role?: string }).role ?? "";
    const title = (status as { title?: string }).title ?? roleSlug;
    const seats = bindings.filter((b) => b.role === roleSlug);
    if (seats.length === 0) {
      dots.push({ role: roleSlug, instance: 0, title, liveness: "vacant", lastWorkingAt: null });
      continue;
    }
    for (const b of seats) {
      dots.push({
        role: roleSlug,
        instance: b.instance,
        title,
        liveness: seatLiveness(b.last_heartbeat, b.last_working_at ?? null, now),
        lastWorkingAt: b.last_working_at ?? null,
      });
    }
  }
  return dots;
}
