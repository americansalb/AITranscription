// One feed row, all variants. Collapsed digests expand per-row, never sticky.
import { useState } from "react";
import { useUi2Store } from "../store/store";
import type { BoardMessage, FeedRow as FeedRowType } from "../store/types";

function when(iso: string): string {
  const t = Date.parse(iso);
  return Number.isFinite(t) ? new Date(t).toLocaleTimeString() : "";
}

/** Clamped body with an expand affordance — a decision surface must never
 * hide the question behind an unopenable clamp (review msg 281 MED-2). */
export function ClampedBody({ body }: { body: string }) {
  const [showAll, setShowAll] = useState(false);
  const long = body.split("\n").length > 6 || body.length > 600;
  return (
    <>
      <p className={`ui2-body${showAll || !long ? "" : " ui2-clamp"}`}>{body}</p>
      {long && (
        <button type="button" className="ui2-clamp-toggle" onClick={() => setShowAll(!showAll)}>
          {showAll ? "show less" : "show all"}
        </button>
      )}
    </>
  );
}

function ExpandedEvents({ events }: { events: BoardMessage[] }) {
  return (
    <ul className="ui2-expanded-events">
      {events.map((m) => (
        <li key={m.id}>
          <span className="ui2-meta">
            {when(m.timestamp)} · {m.from} → {m.to}
          </span>
          <span className="ui2-event-subject">{m.subject || m.type}</span>
        </li>
      ))}
    </ul>
  );
}

export function FeedRowView({ row }: { row: FeedRowType }) {
  const expanded = useUi2Store((s) => s.expandedRows.has(row.key));
  const toggleRow = useUi2Store((s) => s.toggleRow);

  if (row.kind === "message") {
    return (
      <article className={`ui2-row ui2-msg ui2-voice-${row.voice}`}>
        <header className="ui2-meta">
          {row.voice === "human" ? "you" : "relay"} · {when(row.msg.timestamp)}
        </header>
        {row.msg.subject && <h3>{row.msg.subject}</h3>}
        <p className="ui2-body">{row.msg.body}</p>
      </article>
    );
  }

  if (row.kind === "card") {
    return (
      <article className="ui2-row ui2-card-inline">
        <header className="ui2-meta">decision · {when(row.msg.timestamp)}</header>
        <h3>{row.msg.subject || "Decision"}</h3>
        <ClampedBody body={row.msg.body} />
        <span className="ui2-meta">answer in the Decision Dock →</span>
      </article>
    );
  }

  if (row.kind === "discussion") {
    const verdictLine = row.verdict ? row.verdict.subject || "closed" : null;
    return (
      <article className="ui2-row ui2-digest">
        <button
          type="button"
          className="ui2-digest-toggle"
          aria-expanded={expanded}
          onClick={() => toggleRow(row.key)}
        >
          <span aria-hidden="true">{expanded ? "▾" : "▸"}</span> 🗩 {row.label} ·{" "}
          {row.eventCount} events
          {verdictLine && <span className="ui2-verdict"> · {verdictLine}</span>}
        </button>
        {expanded && <ExpandedEvents events={row.events} />}
      </article>
    );
  }

  // burst
  const label = row.key.startsWith("muted-catchup")
    ? `caught up: ${row.count} events while muted · expand`
    : `⚙ ${row.count} engine events${row.protocolViolations > 0 ? ` · ${row.protocolViolations} protocol` : ""}`;
  return (
    <article className="ui2-row ui2-digest">
      <button
        type="button"
        className="ui2-digest-toggle"
        aria-expanded={expanded}
        onClick={() => toggleRow(row.key)}
      >
        <span aria-hidden="true">{expanded ? "▾" : "▸"}</span> {label}
      </button>
      {expanded && row.events.length > 0 && <ExpandedEvents events={row.events} />}
    </article>
  );
}
