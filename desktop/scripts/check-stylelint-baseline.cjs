#!/usr/bin/env node
// Wave 2 stylelint baseline ratchet.
//
// Reads the locked warning baseline from `desktop/scripts/stylelint-baseline.txt`,
// runs `stylelint --formatter json` against the same glob as `npm run lint:styles`
// (`src/**/*.css`), counts total warnings, and exits non-zero if the count
// exceeds the baseline.
//
// Why warn-only stylelint isn't enough: a 3102-warning wall produces no signal
// when a new violation is added — devs tune it out. This script converts the
// warn-only output into an effective regression gate without paying the cost
// of promoting individual rules to ERROR severity.
//
// To update the baseline (after a legitimate cleanup that lowers the count):
//   npm run lint:styles -- --formatter json | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const r=JSON.parse(s);let c=0;for(const f of r)c+=f.warnings.length;console.log(c)})"
// then write that integer into `desktop/scripts/stylelint-baseline.txt`.

const { spawnSync } = require("child_process");
const { readFileSync, existsSync } = require("fs");
const { resolve, join } = require("path");

const SCRIPT_DIR = __dirname;
const PROJECT_DIR = resolve(SCRIPT_DIR, "..");
const BASELINE_FILE = join(SCRIPT_DIR, "stylelint-baseline.txt");

if (!existsSync(BASELINE_FILE)) {
  console.error(`[ratchet] baseline file missing at ${BASELINE_FILE}`);
  process.exit(2);
}

const baselineRaw = readFileSync(BASELINE_FILE, "utf8").trim();
const baseline = parseInt(baselineRaw, 10);
if (!Number.isInteger(baseline) || baseline < 0) {
  console.error(`[ratchet] baseline file does not contain a valid non-negative integer: ${JSON.stringify(baselineRaw)}`);
  process.exit(2);
}

// shell:true handles Windows .cmd resolution; explicit pipe captures both
// stdout and stderr. Stylelint emits its JSON report to one or the other
// depending on version + exit-code path; we coalesce and pull the JSON
// substring out by `[`-anchor since DeprecationWarnings can lead the stream.
const result = spawnSync(
  `npx stylelint "src/**/*.css" --formatter json`,
  {
    cwd: PROJECT_DIR,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    shell: true,
  }
);

const combined = (result.stdout || "") + (result.stderr || "");
// Stylelint JSON output always begins with `[{`; the deprecation warning
// noise begins with `[stylelint:` or similar. Anchor on `[{` to skip noise.
const jsonStart = combined.indexOf("[{");
const jsonEnd = combined.lastIndexOf("}]");
if (jsonStart === -1 || jsonEnd === -1 || jsonEnd <= jsonStart) {
  console.error("[ratchet] could not locate JSON report in stylelint output (status=" + result.status + ")");
  console.error("[ratchet] first 500 chars of combined output:", combined.slice(0, 500));
  process.exit(2);
}
const jsonText = combined.slice(jsonStart, jsonEnd + 2);

let report;
try {
  report = JSON.parse(jsonText);
} catch (err) {
  console.error("[ratchet] failed to parse stylelint JSON output:", err.message);
  process.exit(2);
}

let warningCount = 0;
let errorCount = 0;
for (const fileReport of report) {
  for (const w of fileReport.warnings || []) {
    if (w.severity === "error") {
      errorCount++;
    } else {
      warningCount++;
    }
  }
}

if (errorCount > 0) {
  console.error(`[ratchet] FAIL: stylelint reported ${errorCount} error(s) — fix before checking baseline.`);
  process.exit(1);
}

if (warningCount > baseline) {
  console.error(
    `[ratchet] FAIL: stylelint warning count ${warningCount} exceeds baseline ${baseline} (regression of ${warningCount - baseline}).`
  );
  console.error(`[ratchet] Either fix the new violations or update ${BASELINE_FILE} after deliberate baseline-raising review.`);
  process.exit(1);
}

if (warningCount < baseline) {
  console.log(
    `[ratchet] OK and baseline can be tightened: warnings ${warningCount} < baseline ${baseline}. Consider updating ${BASELINE_FILE} to ${warningCount}.`
  );
} else {
  console.log(`[ratchet] OK: warnings ${warningCount} == baseline ${baseline}.`);
}

process.exit(0);
