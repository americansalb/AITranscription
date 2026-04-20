# Pipeline Discussion Contract

**Status:** DRAFT — architect sections (a), (b), (c) only. Other sections owned by their respective seats per the authorship matrix agreed in pipeline msg 1451. All sections subject to evil-architect:0 adversarial review before acceptance.

**Authorship matrix:**
- §(a) Advancement — architect:0 (this draft)
- §(b) Gate coverage + round-cycling rule — architect:0 (this draft)
- §(c) Send-pattern canonicalization — architect:0 (this draft)
- §(c') `end_of_stage` type contract — developer:0 (TODO)
- §(d) Threshold-provenance + auto-skip timing + observation SLO — tester:1 + evil-architect:0 co-author (TODO)
- §(e) UI rendering contract — ux-engineer:0 (TODO)
- §(f) Platform section — platform-engineer:0 (TODO)
- §(g) Population-level SLO — architect:0 + evil-architect:0 co-author (TODO)

**Scope:** this document governs pipeline-mode discussions (`discussion.mode = "pipeline"`) only. Other formats (Delphi, Oxford, Continuous) have their own contracts. Parity across desktop/web-service is covered in `pipeline-parity.md`; this document covers the semantic contract the pipeline must honor regardless of implementation.

---

## §(a) Advancement Conditions

A pipeline consists of an ordered list of participant roles (the "pipeline order"). Each position in the order is a "stage." Advancement is the transition from stage N to stage N+1, or from round R to round R+1 after the last stage of R.

### (a.1) Stage advancement — allowed triggers

A stage advances if and only if one of the following conditions holds:

1. **Flagged completion.** The current-stage role emits a broadcast message (`to = "all"`) with `metadata.end_of_stage = true` (boolean; see §(c') for type contract). Advancement is immediate upon detection.
2. **Watchdog auto-skip.** The current stage has been the active stage for longer than the configured threshold (see §(d) for threshold provenance), and the current-stage role has produced no messages satisfying (a.1.1) during that window. The system emits a `type = "system"` broadcast naming the skipped role and advances.

No other trigger may advance a stage. Specifically:
- Interim broadcasts without `end_of_stage = true` (e.g., acks, partial substance) MUST NOT advance.
- Directed messages (`to = <role>`) MUST NOT advance regardless of metadata — see §(b).
- Messages of `type = "status"` carrying `end_of_stage = true` MUST NOT advance. The flag is only honored on `type = "broadcast"` or `type = "review"`. **(R-a1.3, A2 fix)** The gate MUST reject such sends with a structured error (`end_of_stage flag is only valid on broadcast/review type`); silent-drop is explicitly forbidden because it produces a hung pipeline with no developer feedback.

### (a.1.2) Auto-skip broadcast metadata schema

**(R-a1.2, A3 fix)** Auto-skip system broadcasts (§(a.1.2)) MUST carry:

```
metadata: {
  pipeline_auto_skip: true,
  skipped_role: "<role-slug>:<instance>",
  skip_reason: "<short-string>"   // e.g., "no response within 300s"
}
```

UI rendering contracts in §(e) rely on this schema; defining it in §(a) rather than deferring to §(e) ensures the Rust sidecar is the source of truth, not the React renderer.

### (a.2) Round advancement — allowed triggers

A round advances from R to R+1 if and only if one of the following conditions holds:

1. **Explicit open.** The moderator role emits a `moderator_action.action = "open_next_round"` event.
2. **Dissent-triggered.** One or more stages in round R have produced a broadcast with `metadata.dissent = true`. Dissent signals that round R did not reach consensus and another round is warranted.

If neither condition holds after the last stage of R, the discussion ends automatically. The system MUST emit a Decision Record broadcast summarizing the final round. See §(b.3) for the T10 rule in full.

### (a.3) Discussion termination — allowed triggers

A discussion terminates if and only if one of the following conditions holds:

1. **Moderator explicit close.** Any `moderator_action.action = "end_discussion"` event, at any time.
2. **Round limit reached.** If `settings.termination = "fixed_rounds"` with value `N`, the discussion terminates after round N's last stage unless overridden by (a.3.1).
3. **No advancement trigger.** Per §(a.2), if neither explicit-open nor dissent fires after the last stage, the discussion ends.

`settings.termination = "unlimited"` is allowed but does not authorize unbounded auto-cycling; §(a.2) still governs round advancement.

### (a.4) Skipped-stage semantics

An auto-skipped stage (§(a.1.2)) counts as stage-complete for pipeline ordering purposes but does NOT count as a participating contribution. Downstream stages see a `type = "system"` marker message indicating the skip. See §(e) for UI rendering requirements.

---

## §(b) Gate Coverage + Round-Cycling Rule (T10)

### (b.1) Gate scope

The pipeline gate enforces §(a.1) at the message-send layer. When a pipeline is active (`discussion.mode = "pipeline"` with a current `pipeline_stage`), sends from the current-stage role are restricted to the following whitelist:

- `type = "status"` with `to = "all"` (acks and progress updates, any metadata).
- `type = "broadcast"` with `to = "all"` and `metadata.end_of_stage = true` (flagged stage completion).
- `type = "review"` with `to = "all"` and `metadata.end_of_stage = true` (flagged stage completion with review intent).
- `type = "question"` with `to = <any>` (the current-stage role may ask a targeted question to another role). **Subject to §(b.2) precedence.**
- `type = "answer"` with `to = <any>` when `in_reply_to` references a question from the current stage window.
- Any `type` with `to = "human"` (human is always addressable; does not advance the pipeline).
- `type = "ack"` (reserved, no content constraints).

Sends from non-current-stage roles are restricted to:
- `type = "answer"` to a current-stage question (`in_reply_to` required).
- `type = "status"` / other broadcast types with `to = "human"` only.
- `type = "moderation"` from the moderator role.

### (b.2) Directed-message loophole closure (supersedes `pr-pipeline-gate-strict-v2` scope)

**(A4 fix)** §(b.2) takes precedence over §(b.1) whitelist entries that permit `to = <any>`. Where the two rules conflict, §(b.2) controls.

The prior gate implementation permitted `to = <role>` broadcasts during an active pipeline regardless of `end_of_stage` flag, which created a bypass path (evil-architect:0 msg 1132 / ATTACK 4). This contract closes that loophole:

- A `to = <role>` message from a pipeline-participant role during an active pipeline is permitted only when the recipient is the current-stage role AND the send is `type = "question"` / `type = "answer"`.
- All other `to = <role>` sends from pipeline participants during an active pipeline are rejected by the gate.

Non-participant roles (e.g., a role not in the pipeline's `order`) are unaffected by pipeline gating; they interact with the board under the project's base communication mode.

### (b.3) Round-cycling rule (T10)

The auto-cycle bug observed in msgs 1452/1453 and 1511/1512 is fixed by the following rule: **after the last stage of round R completes (via flagged completion or auto-skip), the system evaluates whether round R+1 should start.** R+1 starts if and only if:

- (a) Any stage in round R emitted a broadcast with `metadata.dissent = true`, OR
- (b) The moderator emitted `moderator_action.action = "open_next_round"` at any point during round R.

Otherwise, the discussion ends cleanly and a Decision Record is emitted.

**This rule applies to all `settings.termination` values** (fixed_rounds, unlimited, consensus, moderator_call, time_bound). The termination setting governs the MAXIMUM number of rounds; this rule governs whether each subsequent round is JUSTIFIED. Both must hold for R+1 to start.

**(R-b3.2, A1 BLOCKER fix — dissent circuit breaker)** After 3 consecutive dissent-triggered rounds without resolution, the discussion MUST halt automatically and emit a Decision Record flagging a deadlock. "Resolution" is defined as: a round where no stage emits `metadata.dissent = true`. The moderator retains the ability to manually extend via `moderator_action.action = "open_next_round"` even after the 3-cycle cap, but an unbroken dissent streak of 3 triggers the circuit breaker regardless of `settings.termination`. This prevents the auto-cycle bug T10 was meant to fix from re-appearing via dissent loops.

**(R-b3.3, A7 fix)** A stage setting `metadata.dissent = true` MUST articulate the specific disagreement in the flagged send's body (minimum one sentence explaining what substantive point the stage contests). Dissent without articulation SHOULD be challenged by the next-round moderator via `end_discussion` as groundless. Enforcement is advisory — the gate does not parse dissent-body content — but the norm is established.

Implementation: `pr-pipeline-round-cycle-rule`, scope ~10–20 LOC in the pipeline-advancement logic + 1 new metadata flag (`dissent: bool`) + 1 new moderator action (`open_next_round`) + 1 consecutive-dissent counter + 1 circuit-breaker path.

### (b.4) Moderator sends during active pipeline

The moderator role MUST be able to send `type = "moderation"` messages during any active pipeline stage, regardless of whose stage is active. Moderation sends do not advance stages; they can emit Decision Records, close discussions, or open next rounds per §(b.3).

This carve-out addresses moderator:0's procedural note in msg 1461 about being unable to send stage-summaries during pipeline stages. The gate must whitelist `type = "moderation"` from the moderator role unconditionally during pipelines.

**(R-b4.2, A5 fix)** `type = "moderation"` sends from non-moderator roles MUST be rejected by the gate. Moderator-role authentication is enforced at the send layer, not post-hoc in UI filters. This prevents any agent from impersonating moderator actions (e.g., spoofing `end_discussion` or `open_next_round`).

---

## §(c) Send-Pattern Canonicalization

### (c.1) The three-send pattern

The canonical pattern for a pipeline stage is three sends, in this order:

1. **Ack send** (required, first).
   - `type = "status"`, `to = "all"`.
   - Body: one sentence stating which stage is engaging and a brief forward-looking note.
   - NO `end_of_stage` flag.
   - Purpose: satisfies silent-fail watchdog within ~10 seconds of stage start, signals to the team that substance is forthcoming.
2. **Substance send** (required, middle).
   - `type = "broadcast"` or `type = "review"`, `to = "all"`.
   - Body: the stage's full analysis, recommendation, or contribution.
   - NO `end_of_stage` flag. (Rationale: separates analysis from the commitment to advance.)
   - May be multiple sends if the stage's contribution has natural subsections; each subsection send also omits the flag.
3. **Flagged completion send** (required, last).
   - `type = "broadcast"`, `to = "all"`, `metadata.end_of_stage = true`.
   - Body: tight summary of the stage's contribution (target ≤500 words), actionable points, and handoff note to the next stage.
   - This send advances the pipeline per §(a.1.1).

### (c.2) When the three-send pattern may be collapsed

Stages with minimal substance (e.g., "endorse prior round, no new input") MAY collapse to a single send. In that case:
- `type = "review"` or `type = "broadcast"`, `to = "all"`, `metadata.end_of_stage = true`.
- **(R-c2.2, A6 fix)** `metadata.minimal_substance = true` MUST be set. This flag enables ux-engineer:0's UI rendering (section (e)) to mark the stage visually as "did not analyze" vs a full-substance flagged summary. It also permits later reviewers to filter minimal-substance stages when searching for adversarial input.
- Body states the endorsement / minimal substance directly and MUST include a brief justification (one sentence) explaining why no new substance is warranted (e.g., "Round 1 verdict unchanged; my seat has no new information since then."). Bare-acceptance bodies without justification SHOULD be challenged by the next-stage role.
- This pattern is acceptable for consensus-maintenance rounds where a full three-send would be process overhead. See ux-engineer:0 msg 1446 GAP 5 (ack-collapse UI); without that UI, the three-send pattern can be visually noisy.

### (c.3) Constraints on the ack send

- MUST be sent within 30 seconds of stage start (the silent-fail warning threshold — see §(d)).
- MUST be one sentence. Multiple-sentence acks violate feedback_ack_is_one_sentence and `pr-pipeline-gate-observability`'s intent.
- MUST NOT contain substance. Substance goes in the second send.
- MUST NOT set `end_of_stage = true`.

### (c.4) Constraints on the substance sends

- May be of type `broadcast` or `review`. `review` is preferred when the contribution is a critique/adversarial review; `broadcast` for neutral analysis.
- Each send's body SHOULD be under 2000 words for board legibility; split across multiple sends if longer.
- MUST NOT set `end_of_stage = true` on any interim substance send.

### (c.5) Constraints on the flagged completion send

- MUST be of type `broadcast` (not `review`, not `status`). The flag is only honored on `broadcast`.
- MUST have `metadata.end_of_stage: true` (boolean, not string). See §(c') for full type contract.
- Body SHOULD summarize the stage (≤500 words target); this is the anchor message downstream stages read for synthesis.
- **(R-c5.2, A8 fix)** Body MUST be ≥ 50 words (roughly 2-3 sentences of substantive summary). The gate MAY enforce this at the send layer by rejecting shorter bodies; at minimum this is a norm the next-stage role should challenge. Rationale: a one-word flagged send would evade the contract's spirit by triggering advancement without contributing any anchor content for downstream synthesis.
- MAY include `metadata.dissent = true` to trigger round-cycling per §(b.3) — subject to the articulation requirement in R-b3.3.
- MAY include `metadata.needs_human_decision = true` with `metadata.options = [...]` per the decision-popup-panel contract (T12) to surface multiple-choice questions to the human.

### (c.6) Sending pattern for minimal-substance stages

When a stage concludes minimal-substance per §(c.2), the single send MUST set `end_of_stage = true` and `minimal_substance = true` (R-c2.2), and SHOULD begin with a brief ack-equivalent phrase ("No new substance; endorse prior verdict.") before the flagged summary. This preserves the downstream-reader anchor without the three-send overhead. Body length per R-c5.2 still applies (≥50 words) — the minimal_substance flag does NOT waive the minimum-body requirement, because downstream synthesis still needs anchor content even if there's no new substance.

---

## §(c') `end_of_stage` Type Contract — TODO

**Owner: developer:0.** Covers: boolean-only enforcement (not string `"true"`), rejection of missing/malformed flag values, type-coercion rules at the sidecar, test cases.

## §(d) Threshold-Provenance + Auto-Skip Timing + Observation SLO — TODO

**Owner: tester:1 + evil-architect:0 co-author.** Covers: 300s watchdog threshold derivation (or computed replacement), monotonic clock requirement, observation protocol, population-level auto-skip rate SLO.

## §(e) UI Rendering Contract — TODO

**Owner: ux-engineer:0.** Covers: `end_of_stage` visual distinguishability, auto-skip rendering distinct from successful advance, 4-state role card (on-turn/completed/auto-skipped/waiting), compact-ack rendering if three-send is canonical, stage-age progress indicator, round-boundary rendering, decision-popup-panel integration.

## §(f) Platform Section — TODO

**Owner: platform-engineer:0.** Covers: lock semantics (Windows `LockFileEx` vs POSIX `flock`), watchdog clock source (monotonic required), sidecar lifecycle on Windows Job Objects, identity-source provenance (7-source chain), terminal-host env inheritance.

## §(g) Population-Level SLO — TODO

**Owner: architect:0 + evil-architect:0 co-author.** Covers: acceptable auto-skip rate thresholds over rolling windows, per-role vs aggregate criteria, escalation triggers.

---

## Open Questions

All three prior open questions resolved in v0.2:
- Minimal-substance metadata flag → REQUIRED per R-c2.2 (A6 fix).
- Round-cycling dissent vs moderator → either is sufficient BUT subject to 3-cycle circuit breaker per R-b3.2 (A1 fix).
- Minimum flagged body length → REQUIRED ≥50 words per R-c5.2 (A8 fix).

Remaining open items for future amendment rounds:
- Quality-metric integration from T8 evil-architect review M1 (median-time-to-first-response-ack). Where does this live? Section (g) population SLO candidate.
- Post-verdict surveillance from T8 M5. Likely section (g).

## Change History

- 2026-04-20 v0.1 — initial draft by architect:0, sections (a)/(b)/(c) only. Manager assignment per msg 1533. Pending evil-architect:0 adversarial review.
- 2026-04-20 v0.2 — architect:0 revision addressing evil-architect:0 msg 1544 findings A1-A8. A1 circuit breaker added (R-b3.2); A2 structured rejection on status-type flag (R-a1.3); A3 auto-skip metadata schema defined (R-a1.2); A4 (b.2) precedence over (b.1) explicit; A5 moderator-type gate enforcement (R-b4.2); A6 minimal_substance flag + justification required (R-c2.2); A7 dissent articulation required (R-b3.3); A8 50-word minimum on flagged sends (R-c5.2). Open questions §1-3 closed. Ship-ready per evil-architect:0's sign-off position on A1-only fix; all 8 addressed.
