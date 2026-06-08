# Spec — Phantom decision cards: unify the decision predicate on type=="question"

**Date:** 2026-06-08
**Author:** architect:0
**Authorization:** human msg 158 — "architect — spec THAT complete version. Greenlit."
**Reviewers (converged + code-verified):** dev-challenger:1 (msg 150 — caught the dropped-decision hole), developer:0 (msg 154 — verified UI render predicate, settled the directive caveat).
**Owner split:** architect = this spec; developer = Rust impl in `vaak-mcp.rs`.
**Branch:** feature/strict-turn-discipline. **Evolves:** 2026-06-08-decision-gate-presence-collision-spec.md (today's bounce-visibility build, msg 67) — same gate block.

---

## 1. The defect (verified in code + on the live board)

The human reported "I dont have a decision card" while agents believed multiple cards were open and
blocking the channel. Root cause is a **three-layer disagreement on what a decision card is**:

- **Backend one-active-decision gate** (vaak-mcp.rs ~L14070-14093): `this_is_decision = to:human AND
  metadata.choices non-empty` — **NO type check**.
- **Backend options-or-blocked gate** (~L14029-14048): FORCES `choices` on **every** `to:human`
  send regardless of type. So agents staple options onto `type:"status"` reports to get them through.
- **Frontend DecisionPanel** (DecisionPanel.tsx:179-186): renders a card **only** when
  `type=="question"` (plus `type=="directive"` for the "Other"→directive resolution path).
  `type=="status"` never renders.

**Consequence:** a status report to the human is forced to staple options → the backend counts it as
a blocking decision → but the UI never renders it → it becomes an **invisible phantom** that wedges
the single decision slot. Board evidence (section 6-8): 8 `to:human`+choices messages; only **#12**
was `type:"question"` (rendered, human answered it). The other 7 (#2,#4,#15,#25,#42,#68,#87) were
`type:"status"` — invisible, yet the gate blocked new decisions behind the latest (#87). That is why
the moderator's debate-winner card "couldn't reach the human."

## 2. The fix — one predicate, all three layers agree: `type=="question"`

**Why not the naive "count only type==question" (dev-challenger msg 150, must-heed):** several queued
cards are *genuine* decisions an agent mistyped as `status` (#42 is a real safe-vs-minimal ask). If the
backend merely *counts* only `question`, those stop blocking AND (per the UI finding) never render →
they **silently vanish** — converting clogged-channel into dropped-decision, the exact invisible-loss
class fixed earlier today. The fix must **REQUIRE** the type, not just count it.

### Change A — options-or-blocked gate (vaak-mcp.rs ~L14029-14048)

When `to:human` AND `metadata.choices` present → **require `type=="question"`**. Reject any
non-question typed send that carries choices to the human:

> `[DecisionMustBeQuestion] A decision card to the human must be type:"question" so it renders and is
> resolvable. You sent type:"{type}" with choices. If this is a real decision, set type:"question".
> If it's a status/presence report, drop the choices and send it to:"all" (status to the human is not
> a card). Per human msg 158.`

This kills the stapling at the source — an agent can no longer manufacture an invisible phantom.

### Change B — one-active-decision gate (vaak-mcp.rs ~L14070-14093)

`this_is_decision = (to:human AND type=="question" AND choices non-empty)`. Add the `type=="question"`
conjunct to BOTH `this_is_decision` (L14070-14074) and the `latest_decision_id` filter (L14081-14090).

- **`directive` does NOT block** (caveat settled, architect msg 156 + developer msg 154): the
  one-active gate caps *pending asks* competing for the human. Only `question` is a pending ask;
  `directive` is a resolution/instruction artifact — it RENDERS (display/audit) but must never open a
  block, else an "Other" resolution would wedge the slot.

### Change C — visibility stub (emit_blocked_decision_stub, ~L13345)

**No separate change.** The stub fires off the same decision-detection path; once B adds the
`type=="question"` conjunct, the stub automatically follows the corrected predicate.

### UI — unchanged

DecisionPanel already renders `question` (+`directive`). After this fix, backend-count,
gate-enforcement, and UI-render all key on the same `type=="question"` definition.

## 3. Properties

- **966 protection becomes UI-enforced:** to make the human DECIDE you MUST use `type:"question"` or it
  won't render as an actionable card → zero incentive to mislabel a prose decision as status. The hole
  directive 966 closed stays closed, now structurally.
- **Self-cleaning, no migration:** the 7 legacy phantoms are all `type=="status"` → the instant B adds
  the `type=="question"` conjunct they stop counting. Channel unclogs on first run of the new sidecar.
- **Status-to-human:** plain status to the human (no choices) still passes (it's not a card). Team
  convention steers presence/liveness to `to:"all"` (board msg 21); the redirect text reinforces it.

## 3b. Anti-half-fix call-site checklist (evil-arch msg 166 — buzz-door bug class)

This codebase's recurring failure is the **half-applied fix**: updating one gate but not its twin, so
the inconsistency reappears under a new name. ALL backend decision-detection sites must move to the
SAME `type=="question"` test in ONE commit. Checklist the developer must tick:

- [ ] options-or-blocked gate predicate (~L14029-14048) — requires `type=="question"` when to:human+choices.
- [ ] one-active gate `this_is_decision` (~L14070-14074) — `type=="question"` conjunct added.
- [ ] one-active gate `latest_decision_id` filter (~L14081-14090) — `type=="question"` conjunct added.
- [ ] grep the file for any OTHER `to=="human" && choices`-style test and confirm none is left on the
      old "choices present" predicate.

**One nuance — block predicate ≠ render predicate, intentionally:** the three BACKEND sites above key
on `type=="question"` ONLY. The UI render predicate is `question` **OR** `directive`
(DecisionPanel.tsx:179-186) — `directive` renders because it's the "Other"→resolution display path,
but it must NOT block (settled caveat). So this is NOT the split bug — it's two different questions:
"does this OPEN a pending decision?" (`question` only) vs "does this RENDER for display?"
(`question`+`directive`). Keep them distinct on purpose; do not "unify" the UI to drop directive.

## 4. Acceptance

1. Agent sends `type:"status"` + choices to human → `[DecisionMustBeQuestion]`, redirect to type:question or to:all.
2. Agent sends `type:"question"` + choices+allow_other to human → delivered, renders as a card, occupies the slot.
3. Second `type:"question"` to human while one open → `[DecisionPending]` + visibility stub (unchanged behavior, now only triggered by real questions).
4. `type:"directive"` to human (the "Other" resolution path) → renders, does NOT open a new block.
5. Post-build live re-check: the 7 legacy `status` phantoms no longer count — a fresh `type:"question"` to human is NOT blocked behind #87.

## 5. Activation

Sender-side gate → `npm run build-sidecar` + copy binary + **each Claude Code window close+reopen**.
Stale sidecars won't enforce. Same gate block as today's bounce-visibility build — one combined edit,
one rebuild.
