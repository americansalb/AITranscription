// Engine Room (§4.4) — the full, unabridged board. Closed by default.
// Nothing is hidden from audit; it is hidden from default attention.
import { useMemo, useState } from "react";
import { Virtuoso } from "react-virtuoso";
import { useUi2Store } from "../store/store";
import type { BoardMessage } from "../store/types";

function when(iso: string): string {
  const t = Date.parse(iso);
  // locale time like the feed; full ISO stays available in raw mode
  return Number.isFinite(t) ? new Date(t).toLocaleTimeString() : iso;
}

// stable fallback — an inline [] would make every store snapshot unequal
// and loop the render (zustand compares with Object.is)
const NO_MESSAGES: BoardMessage[] = [];

export function EngineRoom() {
  const open = useUi2Store((s) => s.engineRoomOpen);
  const setEngineRoom = useUi2Store((s) => s.setEngineRoom);
  const messages = useUi2Store((s) => (s.project ? s.project.messages : NO_MESSAGES));
  const violations = useUi2Store((s) => s.feed.protocolViolations);
  const [seat, setSeat] = useState("all");
  const [type, setType] = useState("all");
  const [raw, setRaw] = useState(false);

  const seats = useMemo(() => [...new Set(messages.map((m) => m.from))].sort(), [messages]);
  const types = useMemo(() => [...new Set(messages.map((m) => m.type))].sort(), [messages]);
  const filtered = useMemo(
    () =>
      messages.filter(
        (m) => (seat === "all" || m.from === seat) && (type === "all" || m.type === type),
      ),
    [messages, seat, type],
  );

  if (!open) {
    return (
      <button type="button" className="ui2-engine-toggle" onClick={() => setEngineRoom(true)}>
        ▸ Engine Room · {messages.length} messages
        {violations > 0 ? ` · ${violations} protocol` : ""}
      </button>
    );
  }

  return (
    <section className="ui2-engine" role="region" aria-label="Engine room — full board">
      <header className="ui2-engine-header">
        <button type="button" onClick={() => setEngineRoom(false)}>
          ▾ Engine Room
        </button>
        <select value={seat} onChange={(e) => setSeat(e.target.value)} aria-label="Filter by seat">
          <option value="all">all seats</option>
          {seats.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
        <select value={type} onChange={(e) => setType(e.target.value)} aria-label="Filter by type">
          <option value="all">all types</option>
          {types.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
        <label className="ui2-engine-raw">
          <input type="checkbox" checked={raw} onChange={(e) => setRaw(e.target.checked)} /> raw
          JSONL
        </label>
        <span className="ui2-meta">
          {filtered.length}/{messages.length}
        </span>
      </header>
      {/* virtualized — the board grows unbounded; an inline map here is the
          CollabTab timeline lesson reborn (review msg 281 MED-1) */}
      <Virtuoso
        className="ui2-engine-list"
        data={filtered}
        computeItemKey={(_, m) => m.id}
        initialItemCount={Math.min(filtered.length, 30)}
        itemContent={(_, m) =>
          raw ? (
            <pre className="ui2-engine-rawrow">{JSON.stringify(m)}</pre>
          ) : (
            <div className="ui2-engine-row">
              <span className="ui2-meta">
                #{m.id} · {when(m.timestamp)} · {m.from} → {m.to} · {m.type}
              </span>
              <strong>{m.subject}</strong>
              <p>{m.body}</p>
            </div>
          )
        }
      />
    </section>
  );
}
