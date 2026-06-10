// §7 derivation-cost measurement: deriveFeed + deriveDock over a 5,000-message
// board. This measures the store derivation (the algorithmic risk), not paint —
// full render timing stays a LATER.md item until profiled in the real webview.
import { describe, expect, it } from "vitest";
import { deriveFeed, reconcile } from "../digest";
import { deriveDock } from "../dock";
import type { BoardMessage } from "../types";

function synthesizeBoard(n: number): BoardMessage[] {
  const messages: BoardMessage[] = [];
  for (let i = 1; i <= n; i++) {
    const minute = String(Math.floor(i / 60) % 60).padStart(2, "0");
    const second = String(i % 60).padStart(2, "0");
    const timestamp = `2026-06-09T${String(8 + (Math.floor(i / 3600) % 12)).padStart(2, "0")}:${minute}:${second}Z`;
    const base = { id: i, subject: `msg ${i}`, body: "x".repeat(80), timestamp };
    switch (i % 10) {
      case 0:
        messages.push({ ...base, from: "human:0", to: "all", type: "directive", metadata: {} });
        break;
      case 1:
        messages.push({ ...base, from: "code-interpreter:0", to: "all", type: "status", metadata: {} });
        break;
      case 2:
        messages.push({
          ...base,
          from: "code-interpreter:0",
          to: "human:0",
          type: "question",
          metadata: { choices: [{ id: "a", label: "yes" }] },
        });
        break;
      case 3:
        messages.push({
          ...base,
          from: "system",
          to: "all",
          type: "moderation",
          metadata:
            i % 100 === 3
              ? { debate_id: i, oxford_event: "initiated" }
              : { debate_id: i - (i % 100) + 3, oxford_event: "phase_transition" },
        });
        break;
      case 4:
        messages.push({ ...base, from: "tech-leader:0", to: "human:0", type: "review", metadata: {} });
        break;
      default:
        messages.push({ ...base, from: "developer:0", to: "all", type: "status", metadata: {} });
    }
  }
  return messages;
}

describe("derivation cost on a 5k board", () => {
  it("derives + reconciles 5,000 messages well under the 1s initial-render bar", () => {
    const messages = synthesizeBoard(5000);
    const t0 = performance.now();
    const feed = deriveFeed(messages, null);
    const dock = deriveDock(messages, feed.classified);
    const derivationMs = performance.now() - t0;
    const ok = reconcile(messages, feed);
    const reconcileMs = performance.now() - t0 - derivationMs;

    // eslint-disable-next-line no-console
    console.info(
      `[perf] 5k board: derive=${derivationMs.toFixed(1)}ms reconcile=${reconcileMs.toFixed(1)}ms ` +
        `rows=${feed.rows.length} cards=${dock.length} violations=${feed.protocolViolations}`,
    );
    expect(ok).toBe(true);
    expect(feed.rows.length).toBeGreaterThan(0);
    // generous CI-safe bound; typical local runs are far lower. Budget context:
    // derivation must leave nearly all of the 1s §7 bar for paint.
    expect(derivationMs).toBeLessThan(250);
  });
});
