// Top strip: project identity · derived liveness dots · mute (§4.3).
// Dots encode cognition vs connection — a warm zombie never renders healthy.
import { useUi2Store } from "../store/store";
import type { SeatDot } from "../store/types";

function dotTitle(d: SeatDot): string {
  const age = d.lastWorkingAt
    ? `last worked ${new Date(d.lastWorkingAt).toLocaleTimeString()}`
    : "no work recorded";
  return `${d.title} (${d.role}:${d.instance}) — ${d.liveness}, ${age}`;
}

function LivenessDots({ dots }: { dots: SeatDot[] }) {
  const seated = dots.filter((d) => d.liveness !== "vacant");
  const vacantCount = dots.length - seated.length;
  return (
    <div className="ui2-dots" role="group" aria-label="Seat liveness">
      {seated.map((d) => (
        <span
          key={`${d.role}:${d.instance}`}
          className={`ui2-dot ui2-dot-${d.liveness}`}
          title={dotTitle(d)}
          aria-label={dotTitle(d)}
        />
      ))}
      {vacantCount > 0 && <span className="ui2-dots-vacant">+{vacantCount} vacant</span>}
    </div>
  );
}

export function TopStrip() {
  const dots = useUi2Store((s) => s.dots);
  const muted = useUi2Store((s) => s.mutedAtId !== null);
  const toggleMute = useUi2Store((s) => s.toggleMute);
  const projectName = useUi2Store((s) => s.project?.config?.name ?? "Vaak");

  return (
    <header className={`ui2-topstrip${muted ? " ui2-muted" : ""}`}>
      <span className="ui2-project-name">{projectName}</span>
      {muted && <span className="ui2-mute-caption">room muted</span>}
      <LivenessDots dots={dots} />
      <button
        type="button"
        className="ui2-mute-btn"
        aria-pressed={muted}
        onClick={() => void toggleMute()}
      >
        {muted ? "Unmute room" : "Mute all"}
      </button>
    </header>
  );
}
