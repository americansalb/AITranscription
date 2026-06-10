# UI2 Phase 0 — IA Decision Table (Feed-Filter Rules)

**Owner:** ui-architect:0 (absorbed from removed ux-engineer seat, per human msg 225) · **Date:** 2026-06-09
**Governing doc:** "One Window" decree (msg 210) §2, §4.1–4.5 · **Companion:** 2026-06-09-ui2-phase0-token-sheet-and-wireframe.md
**Status:** DELIVERABLE — second half of the Phase 0 gate

---

## 0. The invariant before any rule

**The Engine Room receives 100% of board traffic, always, regardless of every rule below.** These rules govern *attention* (what the Signal Feed surfaces), never *audit* (what exists). Reconciliation test: `signal_rows ∪ digest_contents ∪ engine_only` must equal the full board, no message unaccounted. This is a §7-covered store test, not a promise.

## 1. The decision table — priority-ordered, first match wins

Deterministic field-level match rules. This table compiles directly into one pure function in the store (`classify(message) → treatment`), unit-tested to ≥80% (§7). No component makes filtering decisions.

**Normalization (applies to every rule, per evil-architect msg 245 MED-1):** `role(x)` strips the `:instance` suffix — `role("human:0") = "human"`, `role("code-interpreter:0") = "code-interpreter"`. ALL `from`/`to` comparisons below use `role()`, never raw equality or prefix tests. Live board traffic addresses both `human` and `human:0`; without this, human-addressed DMs silently slip past R6's flag.

| # | Class | Match rule (message fields) | Signal Feed treatment | Engine Room |
|---|---|---|---|---|
| R1 | Human's own posts | `role(from) = human` | **Expanded**, full body, visually distinct (operator's own voice) | logged |
| R2 | Decision cards | (`metadata.choices` present OR `type = "decision"`) **AND `role(from) ∈ {code-interpreter, human, system}`** — the relay is the only agent writer of cards (decree §8.2). A choices-bearing message from any other seat does NOT match R2; it falls through to R6/R7 and is flagged as a protocol violation | **Card**: docked in Decision Dock + inline feed row with `--accent` rule. Body clamps at 6 lines with expand — a card is options, not an essay | logged |
| R3 | Relay posts | `role(from) = code-interpreter` AND `role(to) ∈ {all, human}` | **Expanded**, full body | logged |
| R4 | Discussion verdicts | end-of-discussion event carrying an outcome, by field predicate only: `metadata.oxford_event = "ended"` OR (`metadata.discussion_action = "end"`) — both carry outcome/final_round payloads (cf. board msgs 207, 220). No prose-matched clauses | **Verdict digest** (one row): format · topic · verdict line · participation. Tally line carries a **derived** diversity note: if per-seat model metadata exists in the record, render it ("N seats · M models"); if it does not (true today — RoleConfig has no model field), render "N seats · model diversity unverified". Never assert "1 model" the records can't prove (§3.5: derived, never asserted). Expand → full transcript in Engine Room | logged |
| R5 | Discussion lifecycle | any of `metadata.debate_id`, `metadata.discussion_action`, `metadata.oxford_event`, `metadata.round` present (and not R4), AND the key resolves to a live discussion | **Folded into that discussion's ONE living digest row** — phase/round/count update *in place*; no new feed row per event, ever (§4.5: ceremony gets no UI) | logged |
| R6 | DM-style messages to human from non-relay | `role(to) = human` AND `role(from) ∉ {code-interpreter, system}` | **Engine Room only** + increments a `protocol` counter visible in the ⚙ digest (the counter is itself signal for the next seat-reduction round). The relay and the dock are the only doors (§6) — this rule is the architectural enforcement of complaint #1 | logged + flagged |
| R7 | Everything else | broadcasts, status, reviews, handoffs, SYSTEM events, agent↔agent directed traffic, and any R5-keyed message whose discussion doesn't resolve | **Folded into a time-burst ⚙ digest row**: "⚙ N engine events · expand". A silence gap > 10 min closes the burst; next event opens a new row. One row per burst, count updates in place | logged |

Tie-breaks: a message matching R5 keys for two discussions resolves by `debate_id` first, then `discussion_action` context. R2 beats R3 (a relay-authored card is a card, not a relay post). R2's author gate beats R2's field match — author check runs first. **R4 beats R3** (recorded post-implementation per review msg 282 LOW-1: a relay/system post carrying an end-event renders as the verdict digest, not as relay prose — the verdict IS the signal).

**Per-discussion identity (amended per reviews msg 282 MED-3 / msg 284 MED-2):** non-Oxford discussions are keyed by their start-message id (`disc-<id>`), not a shared sentinel; an end event retires the key, so the next start opens a NEW row. Sequential Delphi/Continuous discussions never merge and verdicts never overwrite each other. Oxford keys (`oxford-<debate_id>`) likewise retire on `ended`.

## 2. Mute overlay (§4.3) — modifies the table while active

While muted: **only R1 and R2 render new content.** R3–R7 accrue silently (Engine Room gets everything; digest counts do NOT tick — no movement on screen at all). On unmute: one catch-up row — "caught up: N events while muted · expand" — then normal rules resume. The human's silence does not depend on agent compliance: a non-compliant post simply lands under this overlay.

## 3. Acceptance map — the five recorded complaints (§9: "my five complaints are the acceptance test")

| Complaint (verbatim from record) | Rule that resolves it | Demonstrable check (Phase 2 gate) |
|---|---|---|
| "stop DMs" | R6: non-relay DMs never reach attention; relay + dock are the only doors | Send a test DM from any agent → feed shows nothing; Engine Room shows it flagged |
| "not reading all that" | Only R1–R4 render expanded; R4 is a one-line verdict, not a transcript | Today's board (~230 msgs) renders as a compact feed — target ≤ ~10 rows, **measured at the Phase 2 gate against the real board, not promised here** (evil-architect LOW-2: R7's 10-min burst gap can yield 12–15 rows on a long day; if measurement exceeds target, the pre-approved lever is collapsing bursts older than a few hours into one day-row — a parameter change, not a redesign) |
| "decisions must be selectable options" | R2: cards are first-class docked objects, options-with-consequences, 6-line body clamp | Card #125 renders natively with selectable options + "other" |
| "one relay voice" | R3 is the *only* rule granting full-prose rendering to an agent, and it matches exactly one role | Grep the rule table: no other rule renders agent prose expanded |
| "the system talks more than a human can absorb" | R5 in-place folding + R7 burst digests + §2 mute | Delphi #12's ~40 lifecycle events = ONE row; mute test: zero screen movement while active |

## 4. Edge cases decided now (so Phase 2 doesn't improvise)

1. **Launch state:** every digest row collapsed, dock shows active card if any, Engine Room closed. Expansion state is in-memory only — nothing persisted (no localStorage truth, §3.4).
2. **Queued decision cards:** dock state machine, not feed filter: one active card; queued cards greyed with "blocked by #N" label (the msg-104/122 silent-block failure made visible, §4.2).
3. **R5 keys that resolve to no live discussion** (orphaned `debate_id`, bare `metadata.round`, events predating a start record): fold into R7 burst, not a phantom discussion row. Resolution = the key maps to a discussion the store has a start record for; anything else is R7 by definition (extended per evil-architect msg 245 LOW-1).
4. **Malformed/unclassifiable message** (missing fields): R7 by definition (it is the catch-all) — `classify()` is total, never throws, tested with fuzzed inputs.
5. **The relay posting agent-directed traffic** (`to = developer` etc.): R7, not R3 — relay's privilege is scoped to what it tells the human/room, not its peer coordination.
6. **system:keepalive ticks:** never appear anywhere, including Engine Room counts header (they are transport, not record; they don't append to board.jsonl).

## 5. What this table deliberately does NOT do

- No per-seat mute/filter customization (scope freeze §12 — goes to ui2/LATER.md if ever).
- No notification badges/sounds — the dock IS the notification surface.
- No "smart" summarization of digest contents — counts and verbatim expansion only. The UI does not paraphrase the record; paraphrase is the relay's job, and it is fact-checked (§9).
