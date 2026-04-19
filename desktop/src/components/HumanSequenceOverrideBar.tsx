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
  /**
   * pr-pipeline-unified-controls PR-3b (2026-04-19): when true, route button
   * handlers to the pipeline-equivalent Tauri commands (pipeline_advance,
   * pipeline_insert_self_next, end_discussion) instead of the sequence-side
   * (pass_turn, human_insert_next, end_sequence). The visible UI is identical
   * — only the backend dispatch differs. Defaults to false (sequence mode).
   */
  isPipelineMode?: boolean;
}

type BusyAction = null | "insert" | "end" | "pass";

export default function HumanSequenceOverrideBar({ turn, projectDir, isPipelineMode = false }: Props) {
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
    void invoke(isPipelineMode ? "pipeline_insert_self_next" : "human_insert_next", {}, "insert");
  };

  const passMyTurn = () => {
    void invoke(isPipelineMode ? "pipeline_advance" : "pass_turn", {}, "pass");
  };

  const endSession = () => {
    setConfirmingEnd(false);
    if (isPipelineMode) {
      // end_discussion's signature is (dir, reason: Option<String>) — pass null reason.
      void invoke("end_discussion", { reason: null }, "end");
    } else {
      void invoke("end_sequence", {}, "end");
    }
  };

  const isHumanTurn = turn.current_holder === "human:0";

  return (
    <div className="human-override-bar" role="region" aria-label="Human controls for active sequence">
      <div className="human-override-label">You</div>
      {isHumanTurn ? (
        <button
          type="button"
          className="human-override-btn human-override-btn-primary"
          onClick={passMyTurn}
          disabled={busy !== null}
          aria-label="End your current turn and advance to the next participant"
        >
          End my turn
        </button>
      ) : (
        <button
          type="button"
          className="human-override-btn"
          onClick={insertMeNext}
          disabled={busy !== null}
          aria-label="Jump to the front of the queue for your next turn"
        >
          Insert me next
        </button>
      )}
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
