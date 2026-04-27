import { useLayoutEffect, useRef, useState } from "react";
import type { SessionBinding } from "../lib/collabTypes";

export interface AssemblyState {
  active: boolean;
  current_speaker: string | null;
  rotation_order: string[];
}

interface Props {
  state: AssemblyState | null;
  sessions: SessionBinding[] | undefined;
}

const ROLE_COLORS: Record<string, string> = {
  manager: "#9b59b6",
  architect: "#1da1f2",
  developer: "#17bf63",
  tester: "#f5a623",
  audience: "#e74c3c",
  user: "#e1e8ed",
};

const HASH_PALETTE = [
  "#e91e63", "#00bcd4", "#ff7043", "#8bc34a", "#7e57c2", "#26a69a",
  "#ec407a", "#42a5f5", "#ffa726", "#66bb6a", "#ef5350", "#ab47bc",
];

function colorFor(seat: string): string {
  const slug = seat.split(":")[0];
  if (ROLE_COLORS[slug]) return ROLE_COLORS[slug];
  for (const [prefix, color] of Object.entries(ROLE_COLORS)) {
    if (slug.startsWith(prefix)) return color;
  }
  let hash = 2166136261;
  for (let i = 0; i < slug.length; i++) {
    hash ^= slug.charCodeAt(i);
    hash = Math.imul(hash, 16777619) & 0xffffffff;
  }
  return HASH_PALETTE[(hash >>> 0) % HASH_PALETTE.length];
}

function isLive(seat: string, sessions: SessionBinding[] | undefined): boolean {
  if (!sessions) return false;
  const [role, instStr] = seat.split(":");
  const inst = Number(instStr);
  return sessions.some(
    (s) => s.role === role && s.instance === inst && s.status === "active"
  );
}

export function AssemblyBanner({ state, sessions }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chipRefs = useRef<Record<string, HTMLSpanElement | null>>({});
  const [micPos, setMicPos] = useState<{ left: number; top: number } | null>(null);

  const order = state?.rotation_order ?? [];
  const speaker = state?.current_speaker ?? null;

  useLayoutEffect(() => {
    if (!state?.active || !speaker || !containerRef.current) {
      setMicPos(null);
      return;
    }
    const chip = chipRefs.current[speaker];
    if (!chip) {
      setMicPos(null);
      return;
    }
    const cRect = containerRef.current.getBoundingClientRect();
    const chRect = chip.getBoundingClientRect();
    setMicPos({
      left: chRect.left - cRect.left + chRect.width / 2,
      top: chRect.top - cRect.top - 2,
    });
  }, [state?.active, speaker, order.join("|")]);

  if (!state?.active || order.length === 0) return null;

  return (
    <div
      className="al-banner"
      role="status"
      aria-label="Assembly Line — current speaker and queue"
      ref={containerRef}
    >
      <span className="al-banner-label" aria-hidden="true">Assembly:</span>
      {order.map((seat) => {
        const live = isLive(seat, sessions);
        const isActive = seat === speaker;
        const color = colorFor(seat);
        return (
          <span
            key={seat}
            ref={(el) => { chipRefs.current[seat] = el; }}
            className={`al-banner-seat${isActive ? " al-banner-seat-active" : ""}`}
            style={{
              background: isActive ? color : "transparent",
              borderColor: color,
              color: isActive ? "#fff" : color,
              opacity: live ? 1 : 0.5,
            }}
            title={
              isActive
                ? `Current speaker: ${seat}${live ? "" : " (disconnected)"}`
                : `${seat}${live ? "" : " (disconnected)"}`
            }
          >
            {seat}
          </span>
        );
      })}
      {micPos && (
        <span
          className="al-banner-floating-mic"
          aria-hidden="true"
          style={{ left: micPos.left, top: micPos.top }}
        >
          🎙
        </span>
      )}
    </div>
  );
}
