import { useState } from "react";
import type { SequenceTurnState } from "./SequenceBanner";

/**
 * Persistent human-override controls for an active sequence. Lives at the
 * top of CollabTab (which is the human's view per manager msg 398), making
 * sequence-level actions always reachable without expanding the moderator
 * panel. Collapsed panel is for granular reorder/insert; this bar is for
 * the two high-value human moves:
 *
 *  - "Insert me next" — jump the human to position 1 in the remaining queue.
 *  - "End session" — terminate the sequence. Destructive; inline confirm.
 *
 * Per feedback_dont_overgate_moderator_ux and feedback_human_authority_vs_agent_audit:
 * human actions do NOT require a reason field. Optional-reason + single-button
 * confirm is the right UX when the actor is the human.
 */

interface Props {
  turn: SequenceTurnState | null | undefined;
  projectDir: string | null;
}

type BusyAction = null | "insert" | "end";

export default function HumanSequenceOverrideBar({ turn, projectDir }: Props) {
  const [busy, setBusy] = useState<BusyAction>(null);
  const [confirmingEnd, setConfirmingEnd] = useState(false);

  if (!turn || !projectDir) return null;

  const invoke = async (command: string, args: Record<string, unknown>, action: BusyAction) => {
    if (!window.__TAURI__) return;
    setBusy(action);
    try {
      const mod = await import("@tauri-apps/api/core");
      await mod.invoke(command, { projectDir, ...args });
    } catch (e) {
      console.error(`[HumanSequenceOverrideBar] ${command} failed:`, e);
    } finally {
      setBusy(null);
    }
  };

  const insertMeNext = () => {
    void invoke("human_insert_next", {}, "insert");
  };

  const endSession = () => {
    setConfirmingEnd(false);
    void invoke("end_sequence", {}, "end");
  };

  return (
    <div className="human-override-bar" role="region" aria-label="Human controls for active sequence">
      <div className="human-override-label">You</div>
      <button
        type="button"
        className="human-override-btn"
        onClick={insertMeNext}
        disabled={busy !== null}
        aria-label="Jump to the front of the queue for your next turn"
      >
        Insert me next
      </button>
      {!confirmingEnd ? (
        <button
          type="button"
          className="human-override-btn human-override-btn-danger"
          onClick={() => setConfirmingEnd(true)}
          disabled={busy !== null}
          aria-label="End the current sequence"
        >
          End session
        </button>
      ) : (
        <span className="human-override-confirm">
          <span className="human-override-confirm-prompt">End this session?</span>
          <button
            type="button"
            className="human-override-btn human-override-btn-danger"
            onClick={endSession}
            disabled={busy !== null}
            autoFocus
          >
            End
          </button>
          <button
            type="button"
            className="human-override-btn-subtle"
            onClick={() => setConfirmingEnd(false)}
            disabled={busy !== null}
          >
            Cancel
          </button>
        </span>
      )}
    </div>
  );
}
