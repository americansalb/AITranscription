import { useEffect, useRef, useState } from "react";

/**
 * Human-balance-adjust modal (replaces window.prompt for the per-card +/-
 * buttons). Per ui-architect msg 626 + architect msg 629:
 *   - Denomination selector (copper / silver / gold) so the human can express
 *     intent in the unit they're thinking in. Submission multiplies into raw
 *     copper for the Tauri command (1 silver = 100c, 1 gold = 10_000c).
 *   - Reason datalist with common reasons; free-form still accepted.
 *   - Escape closes; click on the backdrop closes; click in the dialog does
 *     NOT bubble.
 *
 * Owns its own form state; resets on open. Caller controls visibility via the
 * `open` + `onClose` props.
 */

export type AdjustDirection = "credit" | "debit";

const COMMON_REASONS = [
  "bounty payout",
  "correction",
  "test",
  "demonstration",
  "manual reward",
  "manual clawback",
];

export function AdjustBalanceModal(props: {
  open: boolean;
  seat: string;
  direction: AdjustDirection;
  onClose: () => void;
  onSubmit: (amountCopper: number, reason: string) => Promise<void>;
}) {
  const { open, seat, direction, onClose, onSubmit } = props;
  const [amount, setAmount] = useState<string>("");
  const [denomination, setDenomination] = useState<"copper" | "silver" | "gold">("copper");
  const [reason, setReason] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const amountRef = useRef<HTMLInputElement>(null);

  // Reset state every time the modal opens.
  useEffect(() => {
    if (open) {
      setAmount("");
      setDenomination("copper");
      setReason("");
      setError(null);
      setBusy(false);
      // Focus the amount field after the render commits.
      const t = setTimeout(() => amountRef.current?.focus(), 0);
      return () => clearTimeout(t);
    }
  }, [open]);

  // Escape to close.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  const multiplier = denomination === "gold" ? 10_000 : denomination === "silver" ? 100 : 1;
  const parsed = parseInt(amount.trim(), 10);
  const valid = !Number.isNaN(parsed) && parsed > 0 && reason.trim().length > 0;
  const finalCopper = valid ? parsed * multiplier * (direction === "credit" ? 1 : -1) : 0;

  const verbLabel = direction === "credit" ? "Add" : "Remove";
  const accent = direction === "credit" ? "#17bf63" : "#d97706";

  const submit = async () => {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(finalCopper, reason.trim());
      onClose();
    } catch (e: any) {
      setError(String(e?.message ?? e ?? "Adjust failed"));
      setBusy(false);
    }
  };

  return (
    <div
      className="abm-backdrop"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <div
        className="abm-dialog"
        role="dialog"
        aria-labelledby="abm-title"
        onClick={(e) => e.stopPropagation()}
        style={{ borderTop: `3px solid ${accent}` }}
      >
        <h3 id="abm-title" className="abm-title">
          {verbLabel} balance — <span className="abm-seat">{seat}</span>
        </h3>

        <label className="abm-label">
          Denomination
          <div className="abm-denoms" role="radiogroup" aria-label="denomination">
            {(["copper", "silver", "gold"] as const).map((d) => (
              <label key={d} className={`abm-denom-pill${denomination === d ? " abm-denom-pill-active" : ""}`}>
                <input
                  type="radio"
                  name="abm-denom"
                  value={d}
                  checked={denomination === d}
                  onChange={() => setDenomination(d)}
                />
                <span className={`coin-icon coin-icon-${d}`} aria-hidden="true" />
                <span className="abm-denom-label">{d}</span>
              </label>
            ))}
          </div>
        </label>

        <label className="abm-label">
          Amount ({denomination})
          <input
            ref={amountRef}
            className="abm-input"
            type="number"
            min={1}
            step={1}
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && valid) submit();
            }}
            placeholder={`e.g. 5 (= ${5 * multiplier} copper)`}
          />
        </label>

        <label className="abm-label">
          Reason <span className="abm-required">*</span>
          <input
            className="abm-input"
            type="text"
            list="abm-reasons"
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && valid) submit();
            }}
            placeholder="Audit trail — required, non-empty"
          />
          <datalist id="abm-reasons">
            {COMMON_REASONS.map((r) => (
              <option key={r} value={r} />
            ))}
          </datalist>
        </label>

        <div className="abm-preview">
          {valid ? (
            <span>
              {verbLabel}{" "}
              <strong style={{ color: accent }}>
                {Math.abs(finalCopper).toLocaleString()} copper
              </strong>{" "}
              ({parsed} {denomination}) to <strong>{seat}</strong>
            </span>
          ) : (
            <span className="abm-preview-hint">Fill amount + reason to enable submit.</span>
          )}
        </div>

        {error && <div className="abm-error">{error}</div>}

        <div className="abm-actions">
          <button type="button" className="abm-btn abm-btn-cancel" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className="abm-btn abm-btn-submit"
            onClick={submit}
            disabled={!valid || busy}
            style={{ background: accent, color: "white", borderColor: accent }}
          >
            {busy ? "..." : verbLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
