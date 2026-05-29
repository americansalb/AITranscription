# Busy-Aware FRESH-Relaunch — Supervisor Spec (v1)

**Author:** developer:0  **Date:** 2026-05-29  **Status:** DRAFT for tester acceptance + evil-architect review
**Supersedes:** architect 502/511 "--resume relaunch" approach (corrected by 516/524/526/528).

## Problem
Team seats stop calling `project_wait` and go offline. Root driver = **context fatigue**:
hours of looping → degraded output → malformed/rejected tool-calls ("court/55") → drop.
Three drop modes, distinguished by what they look like to an external observer:

| Drop mode | Process | Stop-hook fires? | Layer-1 wrapper relaunches? | Covered by |
|---|---|---|---|---|
| Clean voluntary stop (turn ends, no tool call) | alive→exits turn | YES (blocks→project_wait) | n/a | Stop-hook (82a8afd) |
| Degraded-output-ENDS-turn ("court" as final text) | alive→exits turn | YES (live-confirmed, dev-challenger 526) | n/a | Stop-hook |
| Process death (crash/window-close/sidecar death) | exits | n/a | YES (--resume/fresh) | Layer-1 wrapper |
| **Malformed/rejected LOOP (never ends turn)** | **alive, looping** | **NO** | **NO (process alive)** | **THIS SPEC (Layer-2 supervisor)** |
| Hung (alive, stuck, no progress) | alive | NO | NO | THIS SPEC |

The alive-but-looping/hung case falls through both the Stop-hook and the wrapper. The
**Layer-2 supervisor** (`vaak-mcp.rs run_supervise`) is the only cover.

## FINAL CLOSED gate (architect 563 — supersedes the single-cap clause-3 below)
The adversarial loop 528→547→549/551→559→561→563 converged on a **two-tier cap** for clause-3,
because the bare in-flight-tool marker had a deep-think gap (547: a pure-reasoning turn before
the first tool call has no marker → killed mid-think) and a turn-active-only fix re-introduced
the long-build-kill (559) and a hung-child trap (561). Final, provably-complete form:

**SPARE iff** `(turn_active AND age < SHORT_CAP ~15min)` **OR** `(live long-op AND age < LONG_CAP ~45-60min)`.
**RELAUNCH-FRESH iff** liveness STALE (successful project_wait only) AND no recent successful
work AND NOT spared. Resolves every quadrant with BOTH clocks bounded (nothing spared forever,
nothing legit killed):
- pure deep-think < short cap → SPARE (turn_active young)
- legit long build, live op < long cap → SPARE (in-flight known-long marker young)
- wedge: turn-active past short cap, no live long-op → RELAUNCH (fast)
- hung build: live op past long cap → RELAUNCH (kill child)
- idle-dropped: turn-inactive + stale → RELAUNCH
- genuine work: recent successful work / fresh wait → SPARE

PRODUCER: `turn_active_started_at_ms` stamped at UserPromptSubmit (covers thinking AND tools),
cleared on a successful project_wait. `in_flight_*` marker (Pre/Post) supplies the long-op
start-time + known-long-tool class. IMPLEMENTED + 18→ tests. FOLLOW-UP: an un-forgeable
live-child-PID check (needs the claude pid in seat.json — linchpin below) strengthens the long
tier; until then the in-flight marker gates it.

---
### (historical) Locked relaunch trigger (architect 528, ratifying evil-arch 524 + dev-challenger 526)
**RELAUNCH-FRESH iff ALL THREE:**
1. **Liveness STALE** — `last_successful_wait_at_ms` older than threshold. Liveness is
   refreshed ONLY by a *successful* `project_wait`, never by a rejected/malformed call.
2. **No recent SUCCESSFUL work** — no completed edit/post recently (NOT raw tool emission).
3. **No tool call IN-FLIGHT** — superseded by the two-tier turn_active + long-op form above.

### Four-quadrant correctness (this is the acceptance contract)
- **Q1 Malformed-LOOP** (court/55): stale + no success + no in-flight (immediately-erroring emission) → **RELAUNCH**. ✓ catches the bug.
- **Q2 Blocked on long op** (5-min cargo build / deep-think): in-flight child pid present → **SPARE**. ✓ no mid-write corruption.
- **Q3 Genuinely working**: recent successful work → **SPARE**. ✓
- **Q4 Genuinely idle-dropped**: stale + no success + no in-flight → **RELAUNCH**. ✓

## Why today's signal is wrong (verified in code)
`run_keep_alive` (vaak-mcp.rs:19280, the Pre/PostToolUse heartbeat hook) stamps
`last_alive_at_ms` (19320) + `last_active_at_ms` (19325) **purely on `tool_name`
presence** — it reads only `tool_name` + `session_id`, NEVER a success/result field.
So a parseable-but-rejected call (valid tool_name → mic-gate "not your turn" / tool
error) refreshes liveness → a wedged seat reads ALIVE → spared. This is evil-arch 524's
hole, confirmed.

**OPEN live-probe (before code-lock):** confirm per-path stamp-on-reject behavior:
- (i) unparseable court/55 (no valid tool_name) — does the harness reject it before
  PreToolUse (no stamp → already caught)?
- (ii) parseable-but-rejected — PreToolUse fires → stamps → spared (the hole)?
- Note vaak-mcp.rs:15287 comment: pure `project_wait` may not fire Pre/Post hooks at all.

## Clause-1 mechanism: success-gated heartbeat
Add field **`last_successful_wait_at_ms`**, stamped ONLY inside `project_wait`'s
server-side success path (after it returns OK to the caller) — never in the
success-blind hook. Supervisor staleness keys off THIS field, not `last_alive_at_ms`.
A malformed/rejected loop never reaches `project_wait`'s success path → never stamps →
correctly goes stale → relaunched. Forge-proof.

## Clause-3 mechanism: in-flight marker (FINAL, architect 537 — supersedes child-pid idea)
The external supervisor sees only files, not CC-process internals, so a 5-min build
running inside a seat is invisible to it. Make it observable via a **Pre/PostToolUse
marker** (NOT child-pid, which isn't cleanly observable from outside):
- **PreToolUse** stamps `{in_flight_tool, in_flight_started_at_ms}` into the seat file.
- **PostToolUse** clears them.
- Supervisor reads **in-flight = an unmatched start** (start with no clear).
- **CLOCK (evil-arch 533):** a marker OLDER than `max-op-duration` (~15min, generous for
  builds/deep-think) = presumed **HUNG** → relaunch. Younger = legit op → spare. Without
  the clock a never-returning op is spared forever (hung-op trap).

Relaunch-fresh iff: (1) stale [successful project_wait only] AND (2) no recent successful
work AND (3) no in-flight marker younger than max-op-duration.

**IMPLEMENTED (additive/inert):** `run_keep_alive` (vaak-mcp.rs) now stamps the marker on
PreToolUse and clears on PostToolUse (idle tools excluded). No consumer reads it yet.

### CORRECTNESS DEPENDENCY (decisive — blocks clause-1 AND clause-3)
Both clauses hinge on **whether PreToolUse fires for rejected/malformed calls**:
- If PreToolUse does NOT fire for rejected calls → a malformed-LOOP stamps nothing
  (no in-flight marker, no liveness refresh) → correctly relaunched. Gate sound.
- If PreToolUse DOES fire for rejected calls → the loop re-stamps a YOUNG in-flight
  marker every iteration → clause-3 would SPARE the very wedge it must catch, AND the
  success-blind `last_active_at_ms` would also refresh. Then clause-1's success-gated
  `last_successful_wait_at_ms` is the ONLY correct signal, and clause-3 needs extra
  logic (e.g. count consecutive rejected Pre-without-Post, or ignore re-stamps).
**The live-probe (below) is therefore decisive, not a detail.**

### PROBE RESULTS (claude-code-guide, 2026-05-29)
- **HIGH — a rejected/blocked call CONTINUES the turn** (model retries), does NOT fire the
  Stop-hook. Confirms the alive-looping wedge is real and only the supervisor catches it.
- **HIGH — a tool that ERRORS fires `PostToolUseFailure`, NOT `PostToolUse`.** The marker
  must clear on BOTH (fixed in code: clear on `hook_event.starts_with("PostToolUse")`).
- **HIGH — a PreToolUse-blocked call fires NEITHER Post event** (tool never executes), so a
  marker stamped at its Pre would not be cleared by a Post → relies on the age-cap / overwrite.
- **LOW / UNDOCUMENTED — whether PreToolUse fires for malformed/schema-invalid calls.** So we
  CANNOT assume "Pre doesn't fire on malformed." Design must be robust either way.

### RESOLUTION (robust to the undocumented Pre-firing)
1. **clause-1 is the load-bearing signal.** `last_successful_wait_at_ms` is stamped ONLY in
   project_wait's success path — unreachable by any rejected/malformed/looping call, whether
   or not Pre fires. Supervisor staleness AND grace-recovery key off THIS field (never
   `last_alive_at_ms`/`last_active_at_ms`). A rejected-loop that advances last_alive does NOT
   advance last_successful_wait → correctly stays stale → killed (closes evil-arch 524's
   grace-recovery hole too).
2. **clause-3 spare is narrowed** so a rejected-loop can't forge a young marker into a spare:
   spare on in-flight ONLY IF the marker is (a) for a KNOWN-LONG tool (`Bash` — the build/
   long-op case; most wedge-loops are project_wait/project_send/MCP loops, not Bash), AND
   (b) younger than max-op-duration, AND (c) `in_flight_started_at_ms` is STABLE across the
   supervisor's pre/post-grace reads (a re-stamping loop moves it; a genuine build keeps it
   fixed). A non-Bash or moving or over-cap marker does NOT spare → relaunch.

## Wiring reality (build-truth — discovered 2026-05-29, MUST fix or marker is INERT)
- `run_keep_alive` (`--keep-alive`) is **dormant**: it is NOT registered in the committed
  `.claude/settings.json` (which wires only turn-gate=Pre, file-op-claim=Post,
  keep-alive-stop=Stop). No `--install-hooks` impl exists in vaak-mcp.rs despite
  launch-team.ps1:182 referencing it. So the marker code won't fire until `--keep-alive`
  is registered as a Pre **and** Post hook.
- The existing **PreToolUse matcher excludes `Bash`** (`Read|Edit|Write|Grep|Glob|WebFetch|
  WebSearch|NotebookEdit`). The #1 long-op (cargo build) IS Bash — so the in-flight hook
  MUST use a matcher that includes Bash (or `*`), else clause-3 never sees a build and
  long-build seats get killed mid-op (the corruption the gate exists to prevent).
- `run_keep_alive` depends on `VAAK_ROLE`/`VAAK_INSTANCE`/`VAAK_PROJECT_DIR` env vars set
  only by the wrappers → it correctly no-ops for bare-claude sessions. The whole stack
  assumes wrapper-launched seats (architect 511), so this is fine.
- **Action (coordinate with ui-architect, hook lane):** add a Pre+Post `--keep-alive`
  hook with a Bash-inclusive matcher to settings.json. Activation gate = next CC start.

### >>> HARD DEPENDENCY #2 — the supervisor has NO killable PID (verified, pre-existing) <<<
The supervisor's kill path reads `seat.json:pid` and calls `is_process_alive(pid)` /
`kill_process_tree(pid)`. **Nothing writes `pid` into the per-seat session file.** Verified
against the live runtime — every `.vaak/sessions/<role>-<inst>.json` has keys
`[cc_session_source, last_active_at_ms, last_alive_at_ms, session_id, tool_count_since_fresh]`,
**no `pid`**. `run_keep_alive` writes `session_id` (CC UUID), not a process id; the only code
writing `"pid"` (vaak-mcp.rs:1351) writes it into `sidecar-events.jsonl`, a log. So
`supervise_initial_decide` always hits the no-PID branch → **Skip → the Layer-2 supervisor
has NEVER been able to kill anything.** This is pre-existing (the old gate read `pid` too) and
independent of the new gate logic — the whole auto-recovery is inert without it.

**FIX (the linchpin) — SAFER design, NO launcher change.** The wrapper ALREADY writes
`wrapper_pid` (its own `$PID`). The supervisor resolves the killable `claude` PID ITSELF:
- `find_child_pid_by_name(wrapper_pid, "claude")` via `CreateToolhelp32Snapshot` — mirror the
  existing `get_parent_pid()` (vaak-mcp.rs:16582); `PROCESSENTRY32` exposes `th32ParentProcessID`
  + `szExeFile`, so finding the claude child of the wrapper is ~20 lines on proven machinery.
- `kill_process_tree(claude_pid)` (taskkill `/F /T`) kills claude + the build subtree but SPARES
  the wrapper (claude's parent) → the wrapper's loop relaunches (fresh, via force_fresh).
- clause-3 live-child = `has_live_child(claude_pid)` (any process whose parent is claude_pid =
  a real running build/bash) — the un-forgeable long-tier guard.
This needs NO `Start-Process -PassThru` (rejected: fragile arg-quoting on the long prompt could
break ALL seat launches) and NO `get_parent_pid()` in the hook (rejected: the hook's parent is
the `.cmd`/`cmd.exe` shim, not claude). Refactor: resolve `claude_pid` in `run_supervise`
(impure) before the pure gate helper; inject it into the state. Pure gate logic is DONE + 21
tests; this impure PID-resolution is best implemented with a live wrapper tree to verify the
snapshot picks the right PID. Until it lands, the gate LOGIC is correct but the runtime kill is
a no-op.

## TWO hard runtime dependencies before this feature recovers ANY seat
1. **In-flight hook wired** (settings.json Pre+Post `--keep-alive`, Bash matcher) — else clause-3
   blind → long-builds killed once a PID exists.
2. **Killable `pid` in seat.json** (wrapper captures claude child PID) — else the kill is a no-op.
The gate decision logic (clauses 1-3, force_fresh, 18 unit tests) is DONE + passing; these two
wiring deps are what make it actually fire. Both are the inert-fix/build-truth class.

## Fresh-vs-resume: OPTION 2 (`force_fresh` flag) — implementer's decision
The supervisor (which makes the kill decision) writes **`force_fresh: true`** into
`seat.json` BEFORE the kill. Both wrappers (`launch-seat.ps1` app-path + `launch-team.ps1`
manual-path) just read `force_fresh` and launch fresh (project_join briefing + board
snapshot) instead of `--resume`. Rationale:
- `--resume` RELOADS the bloated transcript (verified) → re-degrades → treadmill. FRESH
  resets the fatigue. Resume is correct ONLY for an early clean crash (healthy context).
- One brain: the watchdog already owns the kill decision + idle/working signal; let it
  own fresh-vs-resume too. Wrapper stays dumb.
- **Auto-unifies the wrapper divergence** (tester 520): `launch-seat.ps1` is time-only
  168h, `launch-team.ps1` is tool_count≥200 OR 4h — they drifted. With `force_fresh`
  both just read one flag → identical policy, nothing to keep in sync.

Wrapper resume policy after this change: `--resume` iff (no `force_fresh`) AND (early/healthy:
attempt low + recent + session healthy); otherwise FRESH. Supervisor sets `force_fresh`
on any three-clause relaunch (= fatigue/wedge).

## Watchdog location
`vaak-mcp.rs run_supervise` (dedicated `--supervise` process — robust, non-CC-seat per
evil-arch 507 #2). It already detects staleness + kills the pid tree; this spec extends it
with the three-clause gate + `force_fresh` write. The kill→exit→wrapper-relaunch split is
preserved (supervisor turns hang→exit; wrapper owns relaunch).

## Escape sentinel (SHIPPED 2026-05-29, uncommitted)
`.vaak/seats-paused` (or `.vaak/allow-stop`) — honored by all four surfaces so a
human-ordered pause is never fought: Stop-hook (keep-alive-stop.py) / launch-team.ps1 /
launch-seat.ps1 / supervisor (run_supervise skips the whole kill-scan while present).

## Activation gates (THREE — for the human's relaunch)
1. **Stop-hook + seats-paused-in-hook**: arms on next Claude Code start (settings.json read at startup).
2. **Supervisor (run_supervise) changes**: `npm run build-sidecar` (release binary). seats-paused already built + copied 2026-05-29.
3. **`launch-seat.ps1` changes**: Tauri `cargo build` of vaak.exe — the wrapper is `include_str!`'d into launcher.rs:243. (launch-team.ps1 is read at launch, no build.)

## Acceptance — tester's suite is CANONICAL (tester msg 544)
The authority is **`.vaak/design-notes/2026-05-29-seat-durability-acceptance-suite.md`**
(tester-owned, FIVE quadrants). This spec defers to it. Quadrants:
- **Q1 Malformed-LOOP** → RELAUNCH (make-or-break)
- **Q2 Long-build (fresh in-flight marker)** → SPARE (make-or-break)
- **Q3 Genuinely working** → SPARE
- **Q4 Idle-dropped** → RELAUNCH
- **Q5 HUNG op (in-flight marker OLDER than max-op-duration)** → RELAUNCH after cap, NOT
  spared forever (evil-arch 533 / architect 537) — boundary: just-under→SPARE, just-over→RELAUNCH.
- **T0 escape-sentinel FIRST**: seats-paused AND allow-stop, all layers — supervisor kills nothing while present.
- **T3.1 (tester make-or-break)**: the supervisor's staleness MUST read `last_successful_wait_at_ms`,
  NOT the success-blind `last_alive_at_ms`; a parseable-REJECTED project_wait must NOT stamp
  `last_successful_wait_at_ms`; a malformed call must create NO in-flight marker → reads relaunchable.
Unit-test the decision helper (pure function, like `supervise_initial_decide`/
`supervise_post_grace_decide`) with synthetic seat.json states per quadrant; no spawning needed.

## Implementation order
1. Live-probe the stamp-on-reject boundary (close the OPEN item).
2. Stamp `last_successful_wait_at_ms` in project_wait success path.
3. Extend `run_supervise` decision helpers: three-clause gate keyed off the new field +
   in-flight detection; write `force_fresh` before kill; honor seats-paused (done).
4. Wrappers read `force_fresh` → fresh launch; unify resume policy across both.
5. build-sidecar + Tauri cargo build; tester four-quadrant acceptance.
6. Land as ONE coherent commit (incl. seats-paused + ui-arch's keep-alive-stop.py change).
