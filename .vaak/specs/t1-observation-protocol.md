# T1 Observation Protocol

**Version:** 0.2 (draft — incorporates evil-architect:0 msg 1542 adversarial review)
**Author:** tester:1 (measurement); adversarial co-author: evil-architect:0
**Date:** 2026-04-20 (UTC)
**Status:** DRAFT — awaiting final sign-off from evil-architect:0 and architect:0
**Blocks:** T1 briefing-rule fix ship

---

## Purpose

Pre-register success/failure criteria for the T1 briefing-rule fix (~1 LOC in `desktop/src/utils/briefingGenerator.ts` instructing all agents to re-enter `project_wait` after every broadcast).

Without pre-registration, the team risks declaring T1 "worked" or "failed" based on confirmation bias instead of data. Per evil-architect:0 msg 1391 ATTACK 3 + msg 1430 ATTACK 10: correlational evidence with small n cannot be interpreted as causal without pre-registered thresholds.

This doc exists so the measurement is made BEFORE the fix ships, not rationalized AFTER.

---

## Scope and statistical-power disclaimer

**This protocol is a go/no-go heuristic, NOT a statistical test.**

Under the baseline rate of 12.5% and the planned 5-round / ~40-stage window, the protocol has roughly:
- ~51% probability of observing zero dev-challenger:0 skips even if T1 has no effect (under H0, ~5 participations × 0.875^5 ≈ 51%).
- ~40% statistical power to distinguish a true 5% rate from a true 12.5% rate.

**A WORKED verdict after 5 rounds means "consistent with T1 hypothesis, continue to T2 if drop-outs return" — NOT "T1 proven effective."** A FAILED verdict after 5 rounds means "no observed improvement, escalate to T2" — NOT "T1 definitively useless."

This tradeoff is intentional: 5 rounds ships fast feedback at the cost of rigor. If more certainty is required later, §Post-verdict surveillance extends the observation window.

---

## Hypothesis under test

**H:** Agents that exit `project_wait` after broadcasting are reaped by Claude Code's idle-detector (Windows Job Object) before they can respawn back into `project_wait`. Adding a briefing rule that explicitly requires re-entering `project_wait` after every broadcast reduces the drop-out rate by keeping the MCP sidecar blocked in a syscall Claude Code recognizes as "active."

**Null (H0):** T1 has no effect. Drop-outs continue at the baseline rate.

**Alternative failure (H-regress):** T1 introduces a new failure mode (e.g., quality regression, rate-limit exhaustion, message-ordering bugs — see §Secondary metrics and §Tripwire).

---

## Baseline (pre-T1)

Measured from `.vaak/sections/new-vaak/board.jsonl` as of 2026-04-20T05:00Z:

| Pipeline | Date/Time | Stages attempted | Auto-skips | Skipped role |
|---|---|---|---|---|
| Prior run | 2026-04-19 ~18:48Z | 8 | 1 | dev-challenger:0 |
| Pipeline A round 1 | 2026-04-20 01:24Z | 8 | 1 | dev-challenger:0 |
| Pipeline A round 2 | 2026-04-20 01:47Z | 8 | 1 | dev-challenger:0 |
| Pipeline B round 1 | 2026-04-20 04:43Z | 8 | 1 | dev-challenger:0 |

**Totals:** 32 stage-attempts · 4 auto-skips · **aggregate rate = 12.5%**.

**Per-role breakdown:** dev-challenger:0 — 4/4 skips (100% of all skips). Every other role — 0 skips.

**Key empirical observation:** the drop-out failure is 1-role-specific, not population-distributed. Any success criterion based on aggregate rate alone could mask whether the actual failure mode was fixed.

---

## Round validity rules

Per evil-architect:0 msg 1542 axis 1 gameability findings, rounds are not all created equal. The following rules pre-commit to which rounds count:

### V1. No manual intervention between rounds (addresses G1)

A round is INVALID for PRIMARY if any participating role was restarted, re-launched, buzzed, or otherwise externally revived between T1-deploy time and the round's start. This includes:
- Closing and reopening a Claude Code terminal for any participant.
- `project_buzz` sent to any participant.
- Any manual intervention that would otherwise-dead processes.

If such intervention occurs mid-window, the affected role is excluded from that round's PRIMARY count and the intervention is logged in the observation report. The round still counts toward SECONDARY if all other conditions are met.

### V2. Pre-registered round log (addresses G2)

Before each observation round starts, tester:1 posts a `[tester:1] T1 observation round N/5 registered` broadcast with:
- Expected participants.
- Topic category (per V3).
- Expected binary version on each participant (per V5).

Rounds are counted per the pre-registered log. Retroactive exclusion is NOT permitted except for V1-violations (which are logged separately).

### V3. Topic diversity (addresses G3)

The 5 observation rounds must span at least 3 distinct topic categories:
- **Self-referential** (team discussing its own system; today's pipelines are this).
- **Novel-technical** (team discussing a code/architecture problem unrelated to team infrastructure).
- **Adversarial** (team facilitated into structured disagreement, e.g., a pipeline where stages are assigned opposing positions).

At least 1 round from each category must be observed across the 5. Pre-register per V2.

### V4. dev-challenger:0 participation (addresses G4, resolves Open question 3 from v0.1)

Any round missing dev-challenger:0 from the participant list is:
- EXCLUDED from the PRIMARY denominator.
- INCLUDED in SECONDARY aggregate (still counts toward rate calculation).
- INCLUDED in TRIPWIRE (still emits regression alert if other roles skip).
- Logged separately in the observation report.

If fewer than 3 rounds include dev-challenger:0, the window cannot resolve PRIMARY and the verdict is INVALID regardless of other data.

### V5. Binary-version gate

A round counts only if all participating agents' MCP sidecars were spawned after T1 merged. Current heuristic: ≥1 hour since merge AND no team message flagging an outdated binary. If a binary-version metadata field lands in heartbeat (pending platform-engineer:0 follow-up), use that instead.

---

## Measurement

### Window

- **5 pipeline rounds** post-T1-deploy, subject to V1-V5.
- Minimum sample: 5 rounds × 8 stages = **40 stage-attempts**.

### Data source

- `.vaak/sections/*/board.jsonl` in the active project.
- Primary query: `from=system:0` AND body matches `/did not respond within \d+s — pipeline auto-advanced \(skipped\)/`.
- Count skips per pipeline round; enumerate which role was skipped.
- Per-skip diagnostic (see V-guard / M4): when a skip is detected, also read sessions.json for that role's `last_heartbeat`. If heartbeat is > 10 minutes old, classify as TERMINAL-CLOSED, not DROP-OUT, and exclude from PRIMARY. Log separately.

### Measurement agent

- tester:1 (or any agent in the tester role).
- Measurement is read-only. No mutation of board.jsonl, discussion.json, or sessions.json.

### Reporting cadence

- `[tester:1] T1 observation round N/5 registered` broadcast before each round (per V2).
- `[tester:1] T1 observation N/5` broadcast after each round with: stage-attempt count, auto-skip count, per-role breakdown, dropout/terminal-closed classification, rolling rate, any V1/V4 exclusions.
- Final verdict broadcast after round 5: `[tester:1] T1 verdict — WORKED | FAILED | AMBIGUOUS | INVALID` with rationale.

---

## Success / failure thresholds

### PRIMARY success criterion

**dev-challenger:0 auto-skip count == 0 across all valid (V1-V5) PRIMARY-eligible rounds.**

Rationale: 100% of baseline skips concentrate on dev-challenger:0. If T1's hypothesis is correct, the role-specific failure should disappear. If dev-challenger:0 continues skipping (and the skip isn't a TERMINAL-CLOSED per M4), T1 did not fix the actual bug regardless of what happens to the aggregate rate.

### SECONDARY success criterion

**Aggregate auto-skip rate ≤ 5% across all 5 observed rounds** (≤2 skips out of ≥40 attempts).

Rationale: validates that the fix does not create new failure modes elsewhere while closing the primary one.

### Secondary metrics (addresses M1-M3)

In addition to PRIMARY/SECONDARY thresholds, these metrics are measured and reported alongside the verdict. They are NOT verdict-changing unless thresholds are breached (see TRIPWIRE).

- **M1 — Quality regression (response latency proxy):** measure median time from `pipeline_notification` / wake message to first response (ack or substance) across rounds. Compare to a pre-T1 baseline computed from today's pipelines. Flag if median increases > 50% post-T1.
- **M2 — Rate-limit / token exhaustion:** grep board and MCP logs for patterns `/rate.?limit/i`, `/429/`, `/token.*exhaust/i`, `/usage.*cap/i`. Any occurrence during observation window is logged and escalated.
- **M3 — Message ordering:** any observed out-of-order ack / stage transition is logged and escalated. Requires cross-reference with T7 ordering test when it lands. If T7 has not landed by end of window, note M3 as "not systematically measured."

### Verdicts

- **WORKED** — PRIMARY met AND SECONDARY met AND no M1/M2 threshold breach AND no TRIPWIRE.
- **FAILED** — PRIMARY not met (dev-challenger:0 DROP-OUT count ≥ 1, excluding TERMINAL-CLOSED per M4) AND aggregate rate ≥ 12%. No meaningful change from 12.5% baseline. T1 hypothesis refuted; next step is T2 (auto-rejoin with `VAAK_TERMINAL_ID` env injection).
- **AMBIGUOUS** — any mixed signal:
  - PRIMARY met but aggregate rate between 5% and 12%.
  - PRIMARY not met but aggregate rate < 5%.
  - M1 median-latency threshold breached regardless of skip counts.
  - M2 rate-limit pattern observed regardless of skip counts.
  - V4 dev-challenger:0 participated in fewer than 3 PRIMARY-eligible rounds (INVALID takes precedence).
  - Triggers per-role breakdown analysis. If dev-challenger:0 still dropping out while other roles clean: T1 insufficient, ship T2. If dev-challenger:0 clean but a new role is dropping: investigate.
- **INVALID** — fewer than 5 pipeline rounds observed per V1-V5, or V4 threshold (3 dev-challenger:0 rounds) not met. Do not declare verdict until validity conditions are satisfied.

### Tripwire (regression alert; addresses M1, M2)

**Halt observation and emit `[tester:1] T1 REGRESSION ALERT` if ANY of the following:**
- Aggregate rate ≥ 25% (significantly worse than baseline).
- Any previously-clean role (architect:0, developer:0, evil-architect:0, platform-engineer:0, tester:1, ux-engineer:0, manager:0, moderator:0) auto-skips ≥ 1 time classified as DROP-OUT.
- M1 median-latency increases > 100% post-T1 (severe slowdown).
- M2 rate-limit or token-exhaustion pattern observed more than twice across the window.

On REGRESSION ALERT: halt observation, trigger investigation, do not declare verdict until the regression is understood and either dismissed (not caused by T1) or documented.

---

## Post-verdict surveillance (addresses M5)

After the 5-round window closes and a verdict is emitted:

- tester:1 maintains casual monitoring for the next 10 pipeline rounds.
- If a WORKED verdict is followed by a resurgence of auto-skips (dev-challenger:0 drops once, or aggregate > 10% over any rolling 5-round window), emit `[tester:1] T1 verdict-reversal` broadcast and reopen investigation.
- If a FAILED or AMBIGUOUS verdict is followed by unexpected improvement over 10 rounds, emit `[tester:1] T1 late-improvement` broadcast for reconsideration.

Cost of ongoing surveillance is low: board.jsonl grep + per-round note. No active testing.

---

## Disagreements addressed

- **≤10% aggregate threshold (platform-engineer:0 msg 1435 / 1494):** too lenient given baseline concentrates in one role. Per-role-primary prevents averaging from masking the real bug. Adopted.

- **Self-certification risk (evil-architect:0 msg 1430 ATTACK 7):** addressed by full adversarial review of v0.1 → v0.2. Findings G1-G4, statistical-power disclaimer, M1-M5 all incorporated.

- **n=4 baseline is still small (evil-architect:0 msg 1391 ATTACK 3):** acknowledged in §Scope. This protocol pre-registers thresholds so post-T1 data is interpreted against them, not rationalized after.

- **Statistical power shortfall (evil-architect:0 msg 1542 axis 2):** acknowledged and reframed. Option B adopted: heuristic not statistical test. §Post-verdict surveillance provides longer-horizon check.

---

## Open questions (carried from v0.1, with updates)

1. **Is 5 rounds enough?** Acknowledged insufficient for statistical rigor. §Scope disclaims this. §Post-verdict surveillance provides 10 extra rounds of informal observation. If a future team wants rigorous power (~80%), extend to 15 rounds and re-invoke this protocol — the thresholds are unchanged, only the window grows.
2. **Binary-version gate reliability (V5):** current ≥1-hour heuristic is a guess. Platform-engineer:0 flagged session-identity-chain complexity in msg 1435; an explicit binary-version metadata field in heartbeat would make V5 programmatic. Flagged as follow-up, not blocking.
3. **Partial roster handling (V4):** resolved — any round missing dev-challenger:0 is excluded from PRIMARY but kept for SECONDARY/TRIPWIRE.

---

## Acceptance

Before T1 ships:
- [x] tester:1 drafts v0.1.
- [x] evil-architect:0 adversarial-reviews v0.1 (msg 1542).
- [x] tester:1 drafts v0.2 incorporating evil-architect review (this doc).
- [ ] evil-architect:0 signs off on v0.2.
- [ ] architect:0 signs off on population-SLO alignment (cross-ref with T3 contract section g).
- [ ] manager:0 confirms this blocks T1 ship per the ledger.

After T1 ships:
- [ ] tester:1 posts T1 observation round 1/5 registered.
- [ ] tester:1 posts T1 observation 1/5 result after first valid post-T1 pipeline round.
- [ ] tester:1 continues through 5/5 or triggers REGRESSION ALERT if warranted.
- [ ] Final verdict broadcast determines T2 sequencing.
- [ ] tester:1 maintains 10-round post-verdict surveillance per §Post-verdict surveillance.

---

## Version history

- **v0.2 (2026-04-20):** incorporates evil-architect:0 msg 1542 review. Added §Scope + statistical-power disclaimer, §Round validity rules (V1-V5 codifying G1-G4), TERMINAL-CLOSED vs DROP-OUT classification (M4), §Secondary metrics (M1-M3), expanded tripwire (M1/M2 thresholds), §Post-verdict surveillance (M5). Core PRIMARY/SECONDARY/FAIL thresholds unchanged.
- **v0.1 (2026-04-20):** initial draft (tester:1 msg 1539).
