# Spec — Decision-gate presence collision + bounce observability

**STATUS: BUILT (developer msg 67) + ARCHITECT-VERIFIED (msg pending).** 3 edits in `vaak-mcp.rs`:
options gate unchanged + `[OptionsRequired]` redirect text (L14043); `[DecisionPending]` path calls
`emit_blocked_decision_stub` (L14122); helper at L13345–13373 (deduped per `(caller, blocking_id)`
under `with_currency_and_board_lock`, honest wording, best-effort `let _ =`). All four ship-gates
(H1/H2/H3/lock) independently confirmed in code by architect. **NOT YET ACTIVE** — sender-side gate,
each Claude Code window must be closed+reopened (stale sidecars won't enforce). Fix 3 (durable FIFO)
remains DEFERRED: bounces are now VISIBLE, not DURABLE.

**Date:** 2026-06-08
**Author:** architect:0
**Authorization:** human msg 28 ("Architect agreed fix it"), in reply to architect msg 26.
**Owner split:** architect = this spec; developer = Rust impl in `vaak-mcp.rs`; evil-architect = adversarial review (raised the observability framing, board msg 29).
**Branch:** feature/strict-turn-discipline.

---

## 1. The incident (ground truth)

Human broadcast "report to me if you're here." Multiple seats (moderator, dev-challenger) tried to
report presence directly to the human and were silently bounced. The human read the silence as
**dead seats** and burned minutes (3 false "X can't speak" reports in ~90s).

## 2. Root cause (verified in code)

Two gates in `handle_project_send`, `desktop/src-tauri/src/bin/vaak-mcp.rs`:

- **Options-or-blocked gate** (~L13974–13993): when `target_base == "human"`, the send MUST carry
  `metadata.choices` (2–4) + `allow_other == true`, else `[OptionsRequired]`. This forces a pure
  "I'm here" presence ping to become a decision card.
- **One-active-decision gate** (~L14013–14064): fires only when `choices` is non-empty
  (`this_is_decision`). A 2nd open decision-to-human → `[DecisionPending]`.

**Cascade:** options gate conscripts presence pings into the decision channel → one-active gate then
rejects all but the first → losers bounce. The `[DecisionPending]`/`[OptionsRequired]` error returns
to the **sender only**. From the human's seat a correctly-gated agent and a crashed agent are
**indistinguishable** — both are silent. That is the real defect (evil-architect, board msg 29):
a liveness-**observability** failure, not just a presence-ping nuisance.

## 3. Design

> **REVISION (2026-06-08, board msgs 32/35/36/49/52):** The original Fix 1 below (exempt
> `type:"status"` from the options gate) was **RETRACTED**. dev-challenger + evil-architect correctly
> flagged it as a loophole: `type` is agent-controlled, so an agent could relabel a prose-wall
> decision as `status` and bypass the prose-wall protection of directive 966. The design of record is
> now **Fix 1' (presence convention + error-text)** + **Fix 2 (visibility stub)**, with the
> options-or-blocked gate left **byte-for-byte unchanged**. Architect concurred and retracted Fix 1
> (msg 52).

### Fix 1' — Presence convention + sharpened error text (NO code exemption)

The options-or-blocked gate stays **unchanged**: every `to:human` send still requires
`metadata.choices` (2–4) + `allow_other == true`. The discriminator is NOT relaxed — no `type`-based
carve-out, so directive 966's prose-wall protection is fully intact.

Instead:
- **Convention (zero code):** presence/liveness reports go to `to:"all"` (already ungated), never to
  the human as a card. `project_wait` is itself the liveness signal the UI reads; a dead seat is
  observable from the roster, not from a ping.
- **One string change:** sharpen the `[OptionsRequired]` error text (vaak-mcp.rs ~L13988) to add
  guidance: *"For a presence/status ping, send to:all instead of the human."* Turns the bounce into
  self-service guidance. This is a string edit, not a code exemption — nothing becomes gameable.

**Why not the type-exemption:** keying "is this a decision?" off agent-controlled `type=="status"` is
trivially gameable by relabeling → reopens the false-negative (unactionable prose walls) that
directive 966 was built to stop. Rejected. (dev-challenger msgs 32/36, evil-arch msg 35.)

### Fix 2 — Make bounces visible (observability net; evil-arch #1, board msg 29)

Fix 1 removes presence-ping collisions but NOT the residual case: multiple agents that each genuinely
need to surface a real decision still serialize behind one slot, and the losers still bounce invisibly.

When a send is rejected by the one-active-decision gate (`[DecisionPending]`), the sidecar
**auto-emits a lightweight `to:"all"` stub** before returning the error. The human now sees the blocked
state instead of silence — a gated agent no longer looks dead.

**SHIP GATES (independently converged by evil-arch msgs 53/61 + dev-challenger msgs 55/59 — treat H1+H2
as blocking, not nitpicks):**

- **H1 — DEDUP (must-fix).** The `[DecisionPending]` error tells the bounced agent to *retain and
  re-send*. Every re-send re-bounces and would emit another stub → an agent retrying in its loop
  sprays the board, rebuilding the exact decision-spam channel the one-card gate was built to kill
  (human msg 983). **Emit at most once per `(caller, blocking_decision_id)` pair** — check the board
  tail for an existing unresolved stub for that pair and skip re-emit. Without dedup, do not ship.
- **H2 — HONEST WORDING (must-fix).** Nothing is actually queued; the decision is **rejected** and
  depends on the agent re-sending (the unenforced contract). Telling the human "queued behind #N" over
  a non-durable retry is a false durability promise — if the agent then drops to `project_wait`, the
  human waits for a card that never comes (worse than silence). **Stub text must read:**
  `{caller} tried to send a decision — blocked behind card #{id}. Resolve #{id} to let it through.`
  Do NOT use the word "queued."
- **H3 — CORRECT ID (reuse, don't recompute).** Reference `blocking_decision_id` = the gate's existing
  `latest_decision_id` (computed at vaak-mcp.rs ~L14026–14038 = max id over `to==human` AND
  choices-non-empty; the gate is deadlock-safe-by-design so this single value IS the active blocker).
  **Reuse that value** — the same id the `[DecisionPending]` error already cites. Do NOT introduce a
  fresh "latest board message id" lookup; that fresh lookup IS the H3 bug (latest board msg ≠ open card).
- **LOCK — risk lower than first feared, but still verify (dev-challenger msg 66, code-read).**
  `parking_lot::Mutex` is **not** re-entrant, so a held-then-re-acquired board lock would be a **hard
  hang**, not a panic. BUT: at ~L14024 `let board = read_board(...)` returns an **owned `Vec`** and no
  guard is held across L14024–14063 — so at the append point (~L14056) `handle_project_send` holds no
  board lock, and the deadlock risk is low. Still required: confirm (a) `read_board` releases its lock
  before returning, and (b) `append_dispute_system_message`'s own lock order. Keep the append
  **best-effort + FAIL-OPEN**, matching the gate's documented fail-open design (~L14009–14011): a
  stub-append failure must NEVER convert into blocking the `[DecisionPending]` return or the team's
  path to the human. Reuse the `append_dispute_system_message` pattern (~L13295).

### Fix 3 — Reject→queue (DEFER; needs human/coordinator ruling)

The deeper change from the open design call (BACKLOG msg 1013): replace strict-reject with a
server-side FIFO that auto-surfaces the next decision when the active one resolves. Evil-arch's
"standby-loss" mode (rejected sender drops to `project_wait` and never re-sends → decision silently
lost) is the real argument for it. **Not in this spec** — it adds persistent queue state (the
multi-writer class we keep paying for) and was already awaiting a ruling. Fix 2 mitigates the worst
symptom (invisibility) cheaply; revisit Fix 3 as a follow-up.

## 4. Scope / acceptance

- Code: `handle_project_send`, `vaak-mcp.rs` only. Fix 1' = edit the `[OptionsRequired]` error STRING
  only (no predicate change). Fix 2 = add the stub-emit on the `[DecisionPending]` return path.
- **Activation (standing rule):** sender-side gate → `npm run build-sidecar` + copy binary + **each
  Claude Code window close+reopen**. Stale sidecars won't enforce. (`cargo build` alone does NOT
  rebuild the sidecar.)
- **Impl risk to verify first (developer msg 49):** the stub-append fires inside the one-active gate,
  which already does a raw `read_board` at ~L14024. Confirm which locks `handle_project_send` holds at
  that point so the `to:"all"` stub-append does not deadlock or double-lock. Reuse the
  `append_dispute_system_message` pattern (~L13295).
- Acceptance:
  1. Agent sends `type:"status"` "I'm here" to human with no choices → still `[OptionsRequired]`, but
     the error text now points to `to:all`. (Gate UNCHANGED — this is the corrected expectation.)
  2. Two agents each send a real decision card to human → 1st opens, 2nd bounces `[DecisionPending]`
     AND a stub `"<caller> tried to send a decision — blocked behind card #<id>. Resolve #<id> to let
     it through."` appears on the `all` board (no "queued" wording — H2).
  3. Stub is deduped: a re-send of the same bounced decision while #<id> is still open does NOT post a
     second stub for that `(caller, id)` pair (H1).
  4. Agent sends `type:"decision"` with no choices to human → still `[OptionsRequired]` (decisions
     must stay pickable — prose-wall protection intact).

## 5. Interim behavioral rule (until code lands)

All agents: report presence via `to:"all"` broadcast (ungated), NOT a human decision card. Reserve
decision cards for genuine choices the human must make. (Already announced, board msg 21.)
