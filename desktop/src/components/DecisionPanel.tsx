import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { BoardMessage, DecisionResolution, QuestionChoice } from "../lib/collabTypes";
import "./DecisionPanel.css";

/**
 * Decision Panel v1 — per the 6 adversarial flags (board msgs 4784/4787/4789/4811).
 *
 * Sources pending decisions from the existing board.jsonl message stream:
 *   to === "human" && type === "question" && metadata.choices.length > 0
 *
 * Joined against decisions.jsonl resolutions (Tauri command) so resolved
 * questions disappear from the panel while staying in the board scrollback.
 *
 * Six flags covered:
 *   1. Cross-session persistence  — board.jsonl (durable) + decisions.jsonl resolution log
 *   2. Multi-question dedup        — group by metadata.question_hash; UI fallback hashes subject+body
 *   3. "Other" -> directive        — resolve_decision_cmd emits type:directive with in_reply_to
 *   4. Cancellation                — kill icon (author_cancel); 24h stale auto-archives client-side
 *   5. Visibility                  — window-title (N) Vaak badge handled by parent CollabTab
 *   6. Per-decision attribution    — Posed-by chip(s) on each card; Recommended pill on option
 */

const STALE_THRESHOLD_MS = 24 * 60 * 60 * 1000; // 24h — flag #4 auto-archive

interface DecisionGroup {
  /** The first board message (oldest) carrying this question — its id is the decision_id. */
  primary: BoardMessage;
  /** Additional askers (later messages with matching hash). Merges multi-asker cards into one (flag #6). */
  askers: BoardMessage[];
  /** Stable group key (question_hash if provided, otherwise derived). */
  groupKey: string;
}

/**
 * Hash the question content for dedup. Agents *should* set metadata.question_hash;
 * if absent, derive a normalized fallback from subject+body (flag #2 — UI-side fallback).
 *
 * Hashing is stable per-content, not cryptographic — we just need a string
 * collision-resistant enough that two agents asking the literal same question
 * collapse into one card.
 */
function deriveHash(msg: BoardMessage): string {
  const explicit = msg.metadata?.question_hash;
  if (typeof explicit === "string" && explicit.length > 0) return explicit;
  const subject = (msg.subject || "").trim().toLowerCase();
  const body = (msg.body || "").trim().toLowerCase().replace(/\s+/g, " ");
  // Stable cheap hash — JS doesn't ship sha256 in the browser bundle without
  // a crypto.subtle round-trip; for dedup we just need a string that's
  // identical for identical normalized content.
  return `${subject}::${body}`;
}

function groupByHash(messages: BoardMessage[]): DecisionGroup[] {
  const map = new Map<string, DecisionGroup>();
  // Iterate in ascending id order so `primary` is the FIRST asker. messages
  // is already in append order from the board, but we don't assume — sort.
  const sorted = [...messages].sort((a, b) => a.id - b.id);
  for (const m of sorted) {
    const key = deriveHash(m);
    const existing = map.get(key);
    if (existing) {
      existing.askers.push(m);
    } else {
      map.set(key, { primary: m, askers: [], groupKey: key });
    }
  }
  return Array.from(map.values());
}

function isStale(msg: BoardMessage): boolean {
  try {
    const posedAt = new Date(msg.timestamp).getTime();
    if (Number.isNaN(posedAt)) return false;
    return Date.now() - posedAt > STALE_THRESHOLD_MS;
  } catch {
    return false;
  }
}

function formatRelative(iso: string): string {
  try {
    const t = new Date(iso).getTime();
    const diff = Math.floor((Date.now() - t) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  } catch {
    return iso;
  }
}

interface DecisionPanelProps {
  projectDir: string;
  messages: BoardMessage[];
  /** Bubble pending count to the parent so it can set window-title badge (flag #5). */
  onPendingCountChange?: (count: number) => void;
  /** Shared color resolver — passed in from CollabTab so the panel matches the rest of the UI. */
  getRoleColor: (slug: string) => string;
}

export function DecisionPanel({ projectDir, messages, onPendingCountChange, getRoleColor }: DecisionPanelProps) {
  const [resolutions, setResolutions] = useState<Map<number, DecisionResolution>>(new Map());
  const [otherInputs, setOtherInputs] = useState<Record<number, string>>({});
  const [submitting, setSubmitting] = useState<Record<number, boolean>>({});
  const [error, setError] = useState<string | null>(null);
  /** Inline confirm-state per decision id (F-DC-2 sister-fix). First dismiss
   * click sets to true; second click within the timeout commits; auto-resets
   * after a few seconds so accidental first-clicks don't linger. Replaces
   * the original window.confirm() native-modal stack call. */
  const [confirmingDismiss, setConfirmingDismiss] = useState<Record<number, boolean>>({});
  const confirmTimeoutsRef = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map());
  /** Track which stale-archive cancels have already fired so the effect
   * doesn't write a new cancel-line on every render (F-DC-4 sister-fix).
   * A Ref (not state) so we don't trigger re-renders just to remember which
   * IDs are already in-flight or done. */
  const staleArchiveFiredRef = useRef<Set<number>>(new Set());

  // Load resolutions on mount + whenever the message list grows (cheap — small file).
  // Using messages.length as the proxy is fine: any new resolution coming in
  // ALSO appends to board.jsonl (an answer or directive message), so length bumps.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await invoke<DecisionResolution[]>("list_decision_resolutions_cmd", {
          dir: projectDir,
        });
        if (cancelled) return;
        const m = new Map<number, DecisionResolution>();
        for (const r of list) m.set(r.decision_id, r);
        setResolutions(m);
      } catch (e) {
        if (!cancelled) {
          // Silent — empty file or never written yet is normal
          // eslint-disable-next-line no-console
          console.debug("[DecisionPanel] list_decision_resolutions failed:", e);
        }
      }
    })();
    return () => { cancelled = true; };
  }, [projectDir, messages.length]);

  // Derive pending groups: questions to human with choices, not yet resolved.
  // Auto-archive >24h stale (flag #4): we cancel them server-side on render so
  // they stop appearing. A board-state-resolved heuristic (subsequent
  // directive matching topic) is deferred to v2 — too easy to false-positive.
  const groups = useMemo(() => {
    const candidates = messages.filter(
      (m) =>
        m.to === "human" &&
        m.type === "question" &&
        Array.isArray(m.metadata?.choices) &&
        (m.metadata?.choices?.length ?? 0) > 0
    );
    const grouped = groupByHash(candidates);
    return grouped.filter((g) => !resolutions.has(g.primary.id));
  }, [messages, resolutions]);

  // Background stale-archive: cancel any group whose primary is >24h old.
  // Fire-and-forget; on next resolution refresh they'll filter out.
  // F-DC-4 sister-fix: track fired IDs in a Ref so we don't re-fire
  // cancel_decision_cmd on every render — server append-onlys each call,
  // so the original implementation wrote a new cancel line per render-tick.
  useEffect(() => {
    for (const g of groups) {
      if (!isStale(g.primary)) continue;
      if (staleArchiveFiredRef.current.has(g.primary.id)) continue;
      staleArchiveFiredRef.current.add(g.primary.id);
      invoke("cancel_decision_cmd", {
        dir: projectDir,
        decisionId: g.primary.id,
        reason: "stale_archive",
      }).catch(() => {
        // Failure → allow a future render to retry once.
        staleArchiveFiredRef.current.delete(g.primary.id);
      });
    }
  }, [groups, projectDir]);

  // Notify parent for window-title badge (flag #5).
  // Use groups.length not pendingQuestionCount because grouped dedup is the
  // true "decisions awaiting answer" count.
  useEffect(() => {
    onPendingCountChange?.(groups.length);
  }, [groups.length, onPendingCountChange]);

  async function handleAnswer(g: DecisionGroup, choice: QuestionChoice) {
    if (submitting[g.primary.id]) return;
    setSubmitting((s) => ({ ...s, [g.primary.id]: true }));
    setError(null);
    try {
      await invoke("resolve_decision_cmd", {
        dir: projectDir,
        decisionId: g.primary.id,
        optionId: choice.id,
        optionLabel: choice.label,
        otherText: null,
      });
      // Optimistically mark resolved so it disappears immediately, even before
      // the next board refresh lands. The resolutions Map is the source of truth.
      setResolutions((prev) => {
        const next = new Map(prev);
        next.set(g.primary.id, {
          decision_id: g.primary.id,
          kind: "resolve",
          option_id: choice.id,
          at: new Date().toISOString(),
          by: "human:0",
        });
        return next;
      });
    } catch (e) {
      setError(`Couldn't submit answer: ${e}`);
    } finally {
      setSubmitting((s) => ({ ...s, [g.primary.id]: false }));
    }
  }

  async function handleOther(g: DecisionGroup) {
    const text = (otherInputs[g.primary.id] || "").trim();
    if (!text || submitting[g.primary.id]) return;
    setSubmitting((s) => ({ ...s, [g.primary.id]: true }));
    setError(null);
    try {
      await invoke("resolve_decision_cmd", {
        dir: projectDir,
        decisionId: g.primary.id,
        optionId: null,
        optionLabel: null,
        otherText: text,
      });
      setOtherInputs((s) => { const c = { ...s }; delete c[g.primary.id]; return c; });
      setResolutions((prev) => {
        const next = new Map(prev);
        next.set(g.primary.id, {
          decision_id: g.primary.id,
          kind: "resolve",
          other_text: text,
          at: new Date().toISOString(),
          by: "human:0",
        });
        return next;
      });
    } catch (e) {
      setError(`Couldn't submit Other answer: ${e}`);
    } finally {
      setSubmitting((s) => ({ ...s, [g.primary.id]: false }));
    }
  }

  /**
   * Two-step inline dismiss confirm — F-DC-2 sister-fix. First click on the
   * × icon flips the card into "confirming" state for ~3 seconds; second
   * click within that window actually fires the cancel. Click anywhere else
   * (auto-timeout) resets without committing. Replaces native window.confirm()
   * modal-stack which violated ui-arch:1 msg 4985 craft principle 3.
   */
  function clearDismissTimer(id: number) {
    const t = confirmTimeoutsRef.current.get(id);
    if (t !== undefined) {
      clearTimeout(t);
      confirmTimeoutsRef.current.delete(id);
    }
  }

  async function commitCancel(g: DecisionGroup) {
    clearDismissTimer(g.primary.id);
    setConfirmingDismiss((s) => { const c = { ...s }; delete c[g.primary.id]; return c; });
    setSubmitting((s) => ({ ...s, [g.primary.id]: true }));
    setError(null);
    try {
      await invoke("cancel_decision_cmd", {
        dir: projectDir,
        decisionId: g.primary.id,
        reason: "author_cancel",
      });
      setResolutions((prev) => {
        const next = new Map(prev);
        next.set(g.primary.id, {
          decision_id: g.primary.id,
          kind: "cancel",
          reason: "author_cancel",
          at: new Date().toISOString(),
          by: "human:0",
        });
        return next;
      });
    } catch (e) {
      setError(`Couldn't dismiss: ${e}`);
    } finally {
      setSubmitting((s) => ({ ...s, [g.primary.id]: false }));
    }
  }

  function handleDismissClick(g: DecisionGroup) {
    if (submitting[g.primary.id]) return;
    if (confirmingDismiss[g.primary.id]) {
      // Second click within the timeout window — commit
      void commitCancel(g);
      return;
    }
    // First click — flip to confirming state, auto-reset after 3s
    setConfirmingDismiss((s) => ({ ...s, [g.primary.id]: true }));
    clearDismissTimer(g.primary.id);
    const timer = setTimeout(() => {
      setConfirmingDismiss((s) => { const c = { ...s }; delete c[g.primary.id]; return c; });
      confirmTimeoutsRef.current.delete(g.primary.id);
    }, 3000);
    confirmTimeoutsRef.current.set(g.primary.id, timer);
  }

  // Clean up any pending confirm timers on unmount.
  useEffect(() => {
    return () => {
      for (const t of confirmTimeoutsRef.current.values()) clearTimeout(t);
      confirmTimeoutsRef.current.clear();
    };
  }, []);

  return (
    <div className="decision-panel" aria-label="Pending decisions">
      <div className="decision-panel-header">
        <span className="decision-panel-title">Decisions</span>
        {groups.length > 0 && (
          <span className="decision-panel-count" aria-label={`${groups.length} pending`}>
            {groups.length}
          </span>
        )}
      </div>

      {error && <div className="decision-panel-error" role="alert">{error}</div>}

      {groups.length === 0 ? (
        <div className="decision-panel-empty">No pending decisions</div>
      ) : (
        <div className="decision-panel-cards">
          {groups.map((g) => {
            const choices = (g.primary.metadata?.choices || []) as QuestionChoice[];
            const allowOther = g.primary.metadata?.allow_other === true;
            const askedCount = g.askers.length + 1;
            // Per-decision attribution (flag #6): merge all askers into one chip list
            const askerRoles = [g.primary, ...g.askers].map((m) => m.from);
            const isSubmitting = submitting[g.primary.id] === true;
            return (
              <div key={g.primary.id} className="decision-card">
                <div className="decision-card-header">
                  <div className="decision-card-askers" aria-label="Posed by">
                    {askerRoles.map((from, i) => {
                      const role = from.split(":")[0];
                      return (
                        <span
                          key={`${from}-${i}`}
                          className="decision-card-asker-chip"
                          style={{ color: getRoleColor(role), borderColor: getRoleColor(role) }}
                          title={`Asked by ${from}`}
                        >
                          {from}
                        </span>
                      );
                    })}
                    {askedCount > 1 && (
                      <span className="decision-card-asked-nx" title={`Same question asked ${askedCount} times`}>
                        asked {askedCount}&times;
                      </span>
                    )}
                  </div>
                  <button
                    type="button"
                    className={`decision-card-dismiss${confirmingDismiss[g.primary.id] ? " confirming" : ""}`}
                    onClick={() => handleDismissClick(g)}
                    disabled={isSubmitting}
                    title={confirmingDismiss[g.primary.id] ? "Click again to confirm dismiss" : "Dismiss this decision"}
                    aria-label={confirmingDismiss[g.primary.id] ? "Click again to confirm dismiss" : "Dismiss this decision"}
                  >
                    {confirmingDismiss[g.primary.id] ? "Confirm?" : "×"}
                  </button>
                </div>
                {g.primary.subject && (
                  <div className="decision-card-subject">{g.primary.subject}</div>
                )}
                <div className="decision-card-body">{g.primary.body}</div>
                <div className="decision-card-meta">
                  <span className="decision-card-id">#{g.primary.id}</span>
                  <span className="decision-card-time">{formatRelative(g.primary.timestamp)}</span>
                </div>
                <div className="decision-card-choices">
                  {choices.map((c) => (
                    <button
                      key={c.id}
                      type="button"
                      className={`decision-card-choice${c.recommended ? " recommended" : ""}`}
                      onClick={() => handleAnswer(g, c)}
                      disabled={isSubmitting}
                    >
                      <span className="decision-card-choice-label">{c.label}</span>
                      {c.recommended && (
                        <span className="decision-card-choice-rec" title="Recommended by asker">
                          recommended
                        </span>
                      )}
                      {c.desc && <span className="decision-card-choice-desc">{c.desc}</span>}
                    </button>
                  ))}
                </div>
                {allowOther && (
                  <div className="decision-card-other">
                    <input
                      type="text"
                      className="decision-card-other-input"
                      placeholder="Other (free-form answer becomes a directive)..."
                      value={otherInputs[g.primary.id] || ""}
                      onChange={(e) =>
                        setOtherInputs((s) => ({ ...s, [g.primary.id]: e.target.value }))
                      }
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && !e.shiftKey) {
                          e.preventDefault();
                          void handleOther(g);
                        }
                      }}
                      disabled={isSubmitting}
                      aria-label="Free-form Other answer"
                    />
                    <button
                      type="button"
                      className="decision-card-other-submit"
                      onClick={() => handleOther(g)}
                      disabled={isSubmitting || !(otherInputs[g.primary.id] || "").trim()}
                    >
                      Send
                    </button>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
