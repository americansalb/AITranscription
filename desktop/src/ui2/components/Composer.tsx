// Composer — local state only; typing re-renders nothing outside this bar
// (§3.6 / §7 keystroke→paint bar). One input, @target, send.
import { useState } from "react";
import { useUi2Store } from "../store/store";

function parseTarget(text: string): { to: string; body: string } {
  const m = /^@(\S+)\s+([\s\S]+)$/.exec(text.trim());
  return m ? { to: m[1], body: m[2] } : { to: "all", body: text.trim() };
}

export function Composer() {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!text.trim() || busy) return;
    const { to, body } = parseTarget(text);
    setBusy(true);
    try {
      // store action read imperatively — subscribing would re-render per keystroke
      await useUi2Store.getState().sendMessage(to, body);
      setText("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="ui2-composer"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="Message the room, or @role to direct…"
        aria-label="Compose message"
        disabled={busy}
      />
      <button type="submit" disabled={busy || !text.trim()}>
        Send
      </button>
    </form>
  );
}
