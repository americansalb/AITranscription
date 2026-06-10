// Decision Dock — the signature element (§4.2). One active card rendered as
// selectable options with consequences in the labels; queued cards name their
// blocker; resolved cards show what was chosen and when.
import { useState } from "react";
import { useUi2Store } from "../store/store";
import { ClampedBody } from "./FeedRow";
import type { BoardMessage, DecisionCardState } from "../store/types";

interface Choice {
  id: string;
  label: string;
}

function choicesOf(msg: BoardMessage): Choice[] {
  const raw = (msg.metadata as { choices?: unknown }).choices;
  if (!Array.isArray(raw)) return [];
  return raw
    .map((c, i) => {
      if (typeof c === "string") return { id: String(i), label: c };
      if (c && typeof c === "object") {
        const o = c as { id?: unknown; label?: unknown; text?: unknown };
        const label = typeof o.label === "string" ? o.label : typeof o.text === "string" ? o.text : "";
        return label ? { id: String(o.id ?? i), label } : null;
      }
      return null;
    })
    .filter((c): c is Choice => c !== null);
}

function ActiveCard({ card }: { card: DecisionCardState }) {
  const resolveCard = useUi2Store((s) => s.resolveCard);
  const [other, setOther] = useState("");
  const [busy, setBusy] = useState(false);
  const choices = choicesOf(card.msg);

  const answer = async (choiceId: string, text: string) => {
    setBusy(true);
    try {
      await resolveCard(card.msg.id, choiceId, text);
    } catch {
      // store surfaced it in the error bar; without this catch the rethrow
      // escapes as an unhandled rejection (review msg 309 LOW-A)
    } finally {
      setBusy(false);
    }
  };

  // no aria-live here — the dock region announces; nesting double-announces
  return (
    <article className="ui2-dock-card ui2-dock-active">
      <h3>{card.msg.subject || `Decision #${card.msg.id}`}</h3>
      <ClampedBody body={card.msg.body} />
      <div className="ui2-dock-options">
        {choices.map((c) => (
          <button
            key={c.id}
            type="button"
            disabled={busy}
            onClick={() => void answer(c.id, c.label)}
          >
            {c.label}
          </button>
        ))}
        <form
          className="ui2-dock-other"
          onSubmit={(e) => {
            e.preventDefault();
            if (other.trim()) void answer("other", other.trim());
          }}
        >
          <input
            value={other}
            onChange={(e) => setOther(e.target.value)}
            placeholder="Other…"
            aria-label="Other answer"
            disabled={busy}
          />
        </form>
      </div>
    </article>
  );
}

export function DecisionDock() {
  const dock = useUi2Store((s) => s.dock);
  const active = dock.find((c) => c.status === "active");
  const queued = dock.filter((c) => c.status === "queued");
  const resolved = dock.filter((c) => c.status === "resolved").slice(-3);

  return (
    <aside
      className={`ui2-dock${active ? " ui2-dock-has-active" : ""}`}
      role="region"
      aria-label="Decisions awaiting you"
      aria-live="polite"
    >
      <h2>Decisions</h2>
      {!active && queued.length === 0 && <p className="ui2-meta">Nothing waiting on you.</p>}
      {active && <ActiveCard card={active} />}
      {queued.map((c) => (
        <article key={c.msg.id} className="ui2-dock-card ui2-dock-queued">
          <h3>{c.msg.subject || `Decision #${c.msg.id}`}</h3>
          <p className="ui2-meta">blocked by #{c.blockedBy}</p>
        </article>
      ))}
      {resolved.length > 0 && (
        <div className="ui2-dock-resolved">
          {resolved.map((c) => (
            <p key={c.msg.id} className="ui2-meta">
              ✓ #{c.msg.id} — {c.resolvedChoice}
              {c.resolvedAt ? ` · ${new Date(c.resolvedAt).toLocaleTimeString()}` : ""}
            </p>
          ))}
        </div>
      )}
    </aside>
  );
}
