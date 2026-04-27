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
  if (!state?.active || !state.rotation_order?.length) return null;

  const order = state.rotation_order;
  const speaker = state.current_speaker;
  const speakerIdx = speaker ? order.indexOf(speaker) : -1;
  const nextUp =
    speakerIdx >= 0 ? order[(speakerIdx + 1) % order.length] : order[0];

  const queue: string[] = [];
  if (speakerIdx >= 0) {
    for (let i = 2; i < order.length; i++) {
      queue.push(order[(speakerIdx + i) % order.length]);
    }
  } else {
    queue.push(...order.slice(1));
  }

  const speakerLive = speaker ? isLive(speaker, sessions) : false;
  const speakerColor = speaker ? colorFor(speaker) : "#657786";
  const nextLive = nextUp ? isLive(nextUp, sessions) : false;
  const nextColor = nextUp ? colorFor(nextUp) : "#657786";

  return (
    <div
      className="al-banner"
      role="status"
      aria-label="Assembly Line — current speaker and queue"
    >
      <span className="al-banner-mic" aria-hidden="true">🎙</span>
      {speaker ? (
        <span
          className="al-banner-speaker"
          style={{
            background: speakerColor,
            opacity: speakerLive ? 1 : 0.5,
          }}
          title={`Current speaker: ${speaker}${speakerLive ? "" : " (disconnected)"}`}
        >
          {speaker}
        </span>
      ) : (
        <span className="al-banner-speaker al-banner-speaker-empty" title="No current speaker">
          (none)
        </span>
      )}
      {nextUp && nextUp !== speaker && (
        <>
          <span className="al-banner-arrow" aria-hidden="true">→</span>
          <span
            className="al-banner-next"
            style={{
              borderColor: nextColor,
              color: nextColor,
              opacity: nextLive ? 1 : 0.5,
            }}
            title={`Next: ${nextUp}${nextLive ? "" : " (disconnected)"}`}
          >
            {nextUp}
          </span>
        </>
      )}
      {queue.length > 0 && (
        <span className="al-banner-queue-sep" aria-hidden="true">·</span>
      )}
      {queue.map((seat) => {
        const live = isLive(seat, sessions);
        return (
          <span
            key={seat}
            className="al-banner-chip"
            style={{
              borderColor: colorFor(seat),
              color: colorFor(seat),
              opacity: live ? 0.85 : 0.5,
            }}
            title={`Queued: ${seat}${live ? "" : " (disconnected)"}`}
          >
            {seat}
          </span>
        );
      })}
    </div>
  );
}
