// Trial spot-audit (cutover card caveat, msg 388): zero Engine-Room data loss
// vs the old UI, verified against the LIVE board — every message in
// .vaak/board.jsonl must land in exactly one feed row or the engine-only set
// (reconcile() invariant from digest.ts), run on real data, not fixtures.
// Run: node e2e/spot-audit-live-board.mjs   (from desktop/)
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const here = dirname(fileURLToPath(import.meta.url));
// Resolve the ACTIVE section's board — collab::active_board_path semantics
// (collab.rs:1582). The root board.jsonl is the "default" section only;
// auditing it for a non-default section passes vacuously (correction to the
// first run of this audit, msg 392 vs dev-challenger's msg 395).
const vaakDir = resolve(here, "../../.vaak");
let section = "default";
try {
  section = readFileSync(resolve(vaakDir, "active-section"), "utf8").trim() || "default";
} catch {}
const boardPath =
  section === "default"
    ? resolve(vaakDir, "board.jsonl")
    : resolve(vaakDir, "sections", section, "board.jsonl");
console.error(`[spot-audit] section=${section} board=${boardPath}`);

// bundle the pure derivation modules (TS) into an importable data-url module
const bundle = await build({
  entryPoints: [resolve(here, "../src/ui2/store/digest.ts")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "neutral",
});
const mod = await import(
  "data:text/javascript;base64," + Buffer.from(bundle.outputFiles[0].text).toString("base64")
);

const lines = readFileSync(boardPath, "utf8").split("\n").filter((l) => l.trim());
const messages = [];
for (const line of lines) {
  try {
    messages.push(JSON.parse(line));
  } catch {
    console.error("UNPARSEABLE LINE (would be lost!):", line.slice(0, 120));
  }
}

const feed = mod.deriveFeed(messages, null);
const ok = mod.reconcile(messages, feed);

// independent recount — don't trust reconcile() to audit itself
const seen = new Set();
let dupes = 0;
const collect = (m) => {
  if (seen.has(m.id)) dupes++;
  seen.add(m.id);
};
for (const row of feed.rows) {
  if (row.kind === "message" || row.kind === "card") collect(row.msg);
  else for (const m of row.events) collect(m);
}
for (const m of feed.engineOnly) collect(m);
const missing = messages.filter((m) => !seen.has(m.id)).map((m) => m.id);

console.log(
  JSON.stringify(
    {
      boardLines: lines.length,
      parsedMessages: messages.length,
      feedRows: feed.rows.length,
      engineOnly: feed.engineOnly.length,
      protocolViolations: feed.protocolViolations,
      reconcileInvariant: ok,
      independentRecount: { accounted: seen.size, duplicates: dupes, missingIds: missing },
    },
    null,
    2,
  ),
);
process.exit(ok && missing.length === 0 && dupes === 0 ? 0 : 1);
