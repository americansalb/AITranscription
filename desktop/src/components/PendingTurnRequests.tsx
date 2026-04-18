import { useState } from "react";
import { getRoleColor } from "../utils/roleColors";
import type { SequenceTurnState } from "./SequenceBanner";

/**
 * Shows pending "request turn" entries from non-current-holder agents.
 * Agents request via a dedicated MCP call (developer:0 owns the agent-side
 * plumbing); the backend appends to `turn.pending_requests`. Here the
 * human operator sees the list and either accepts (inserts the requester
 * at position 0 of remaining queue) or dismisses (drops the request).
 *
 * Hidden when: no active sequence, OR no pending requests.
 *
 * Per manager msg 417 PR-F scope. Final piece of the sequential-turn UX.
 */

interface Props {
  turn: SequenceTurnState | null | undefined;
  projectDir: string | null;
}

type BusyKey = string | null;

function splitRoleInstance(id: string): { role: string; instance: string } {
  const idx = id.indexOf(":");
  if (idx === -1) return { role: id, instance: "0" };
  return { role: id.slice(0, idx), instance: id.slice(idx + 1) };
}

function formatAge(iso: string): string {
  const parsed = Date.parse(iso);
  if (!Number.isFinite(parsed)) return "";
  const ms = Date.now() - parsed;
  if (ms < 60_000) return `${Math.max(0, Math.floor(ms / 1000))}s ago`;
  if (ms < 3_600_000) return `${Math.floor(ms / 60_000)}m ago`;
  return `${Math.floor(ms / 3_600_000)}h ago`;
}

export default function PendingTurnRequests({ turn, projectDir }: Props) {
  const [busy, setBusy] = useState<BusyKey>(null);

  if (!turn || !projectDir) return null;
  const requests = turn.pending_requests ?? [];
  if (requests.length === 0) return null;

  const invoke = async (command: string, requester: string, actionKey: string) => {
    if (!window.__TAURI__) return;
    setBusy(actionKey);
    try {
      const mod = await import("@tauri-apps/api/core");
      await mod.invoke(command, { projectDir, requester });
    } catch (e) {
      console.error(`[PendingTurnRequests] ${command} failed:`, e);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div
      className="pending-turn-requests"
      role="region"
      aria-label={`${requests.length} pending turn request${requests.length === 1 ? "" : "s"}`}
    >
      <div className="pending-turn-header">
        <span className="pending-turn-hand" aria-hidden="true">
          ✋
        </span>
        <span className="pending-turn-title">
          {requests.length} turn request{requests.length === 1 ? "" : "s"}
        </span>
      </div>
      <ul className="pending-turn-list" role="list">
        {requests.map((req) => {
          const { role, instance } = splitRoleInstance(req.requester);
          const color = getRoleColor(role);
          const acceptKey = `accept:${req.requester}`;
          const dismissKey = `dismiss:${req.requester}`;
          return (
            <li key={req.requester} className="pending-turn-item">
              <span className="pending-turn-dot" style={{ background: color }} />
              <div className="pending-turn-info">
                <div className="pending-turn-who">
                  <span className="pending-turn-role">{role}</span>
                  <span className="pending-turn-instance">:{instance}</span>
                  <span className="pending-turn-age">{formatAge(req.requested_at)}</span>
                </div>
                {req.reason && <div className="pending-turn-reason">{req.reason}</div>}
              </div>
              <div className="pending-turn-actions">
                <button
                  type="button"
                  className="pending-turn-accept"
                  onClick={() => invoke("accept_turn_request", req.requester, acceptKey)}
                  disabled={busy !== null}
                  aria-label={`Accept turn request from ${req.requester} — insert at front of queue`}
                >
                  Accept
                </button>
                <button
                  type="button"
                  className="pending-turn-dismiss"
                  onClick={() => invoke("dismiss_turn_request", req.requester, dismissKey)}
                  disabled={busy !== null}
                  aria-label={`Dismiss turn request from ${req.requester}`}
                >
                  Dismiss
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
