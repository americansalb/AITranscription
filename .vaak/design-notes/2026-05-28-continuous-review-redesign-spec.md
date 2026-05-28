# Continuous Review Redesign Spec

**Author:** architect:0 (2026-05-28, per human msg 2549 directive)
**Status:** Spec only. Implementation gated on Phase 1 hot-reload chain completing (SHA-HR.1.4 → 1.5 → 1.6). Design work proceeds in parallel.

## What Continuous Review IS (per directive verbatim)

Continuous Review is the peer review system for shipped work. When a developer commits code, a short review window opens, named reviewers give feedback, and the commit either ships clean or gets challenged.

It is NOT a discussion mode. It is NOT a debate. It is the quality gate between "code committed" and "code accepted."

## Architectural shift from the existing "Continuous" discussion mode

The current implementation treats Continuous as a `discussion_control mode = "continuous"` that auto-triggers micro-rounds from status messages. **This is DEPRECATED entirely** under the new design. The new Continuous Review is a WORKING MODE alongside Assembly Line, not a discussion mode.

Discussion modes today (open / directed) remain orthogonal — they govern message routing, not review semantics. Working modes (Assembly Line / Continuous Review) govern build orchestration + review gating.

## Two operating configurations

### Configuration 1: Continuous Review standalone

- No rotation order. Agents work freely — anyone can build at any time.
- When a builder ships a commit, they name at least 2 reviewers (if 2+ non-builder seats are available; 1 if only 1 available; 0 only if nobody else is online).
- Builder CHOOSES who based on what changed and whose eyes they need.
- A review window opens for the named reviewers; timer starts (default 60s, configurable).
- Named reviewers respond: APPROVE / BLOCK / COMMENT.
- Uninvited agents can voluntarily respond with COMMENT only (advisory, non-blocking, cost-free).
- Window closes when all named reviewers have responded OR timer expires.
- Silence from named reviewer within timer = APPROVE.
- Zero BLOCKs → commit accepted. Any BLOCK → builder addresses and re-submits for review.

### Configuration 2: Assembly Line (includes Continuous Review)

- Assembly Line manages rotation order for building. Mic passes through roster in order.
- When the mic holder ships a commit, Continuous Review kicks in AUTOMATICALLY. Same rules: builder names ≥2 reviewers, window opens, timer runs.
- The difference vs standalone: Assembly Line provides ROTATION CONTEXT — the builder knows who's next in rotation and can use that to inform reviewer picks. But the picks are still the builder's choice.
- After the review window closes, the mic passes to the next person in rotation order.
- Building and reviewing are PHASES within Assembly, not separate modes.

**Assembly Line ALWAYS has Continuous Review built in. You never launch both separately.**

## Launch row updates

After this redesign, the launch row has:
- **Assembly Line** (ordered rotation + built-in review)
- **Continuous Review** (free-form work + review on commit)
- **Oxford Debate** (discrete event, separate)
- **Delphi Discussion** (discrete event, separate)

Oxford and Delphi remain separate discrete events launched on top of either working mode.

The current `Start Continuous Review` button (per SHA-LR.3 `7ee55d3` and SHA-LR.2 launcher) needs RESEMANTICIZATION — it currently starts a Continuous Discussion (auto-triggered micro-rounds from status messages). Post-redesign, it starts a Continuous Review working mode (no automatic micro-rounds; just builders shipping commits with named-reviewer windows).

## Review flow (both configurations)

1. Builder commits code (git commit)
2. Builder posts a ship message naming reviewers:
   ```
   SHIPPED [commit SHA]. @dev-challenger:0 @tester:0 review.
   ```
   Or structurally via `metadata.reviewers: ["dev-challenger:0", "tester:0"]` (preferred — cleaner than parsing body).
3. System opens a review window (timer starts).
4. Named reviewers respond: APPROVE / BLOCK / COMMENT.
5. Uninvited agents may optionally respond: COMMENT only.
6. Window closes when:
   - All named reviewers responded, OR
   - Timer expires (silence from named reviewers = APPROVE)
7. Result:
   - Zero BLOCKs → commit accepted, move on
   - Any BLOCK → builder addresses, re-submits for review

## Minimum reviewer rules

- Builder MUST name at least 2 reviewers if 2+ non-builder seats are available
- 1 reviewer if only 1 other seat is available
- 0 reviewers if nobody else is online; commit ships unreviewed
- Builder CHOOSES based on what changed (backend Rust → maybe dev-challenger + tester; CSS tweak → ui-architect + ux-engineer; risky architecture → architect + evil-architect + tester)
- Builder cannot game by always picking soft reviewers — backed by the economic backstop

## Economic backstop

Anyone can file `currency_objection` against ANY shipped commit at ANY time, regardless of whether they were invited to review. This costs 50 copper.

- If objection succeeds (concede or judge ruling) → builder loses escrow + retro pass penalties
- If objection fails → challenger loses 50 copper

Implications:
- Adversarial roles (dev-challenger, evil-architect) can ALWAYS challenge a commit they weren't invited to review
- They don't need invitation or permission; they need willingness to stake 50 copper
- Builder who consistently skips adversarial review on risky commits → gets objected to, loses copper, learns
- Frivolous challenger → loses 50 copper repeatedly, learns to pick battles

The review system is the FAST PATH (invited, free, bounded by timer). The objection system is the BACKSTOP (uninvited, costs copper, unbounded by timer).

## System tracks per review window

Each review window produces a record:
- `commit_sha`: string
- `builder`: seat label (e.g., "developer:0")
- `named_reviewers`: array of seat labels
- `responses`: array of `{seat, type: APPROVE|BLOCK|COMMENT, text, at, was_named: bool}`
- `timer_duration_secs`: number (default 60, configurable)
- `timer_expired`: bool
- `outcome`: "accepted" | "blocked"
- `opened_at`: ISO timestamp
- `closed_at`: ISO timestamp

This feeds the Flow Feed (Chitragupta) and the per-message economic footer.

Examples on commit messages:
- `✓ reviewed by @dev-challenger:0 (APPROVE) @tester:0 (APPROVE)`
- `✗ BLOCKED by @dev-challenger:0: missing error handling`

## Role routing — NOT hardcoded

The review routing is NOT hardcoded to specific role names. Roles will grow over time. The system does not need to know what a "dev-challenger" or "security-auditor" does. It only needs to know:
- Who the builder named as reviewers
- Whether each named reviewer responded within the timer
- Whether any response was a BLOCK

Role-specific review authority (who SHOULD review what) is the builder's judgment call, not the system's enforcement. The minimum-2 rule and the economic backstop are the only structural enforcement.

## Constraints summary

- Minimum 2 named reviewers (if available)
- Builder chooses who
- Named reviewers: APPROVE / BLOCK / COMMENT
- Uninvited reviewers: COMMENT only (advisory, non-blocking)
- Timer default 60s (configurable in Assembly setup, presumably Continuous Review standalone setup too)
- Silence from named reviewer within timer = APPROVE
- BLOCK requires resolution before commit is accepted
- `currency_objection` remains available on any commit at any time regardless of review outcome — the economic backstop
- Review records persist in Flow Feed and on message cards
- Assembly Line includes this automatically; Continuous Review standalone uses the same rules without rotation order
- Oxford and Delphi are separate discrete events, not review modes

## State storage architectural question

**Option A — `.vaak/reviews.jsonl` append-only file.** New canonical store. Each review window writes one open event + N response events + one close event. Clean separation from board events.

**Option B — extend `board.jsonl` with `type: "review_*"` events.** No new file; review events live alongside other team messages. Easier query for "everything that happened around this commit."

**Architect lean: Option B.** Review windows ARE team events; they correlate tightly with the ship broadcast that triggers them. Querying by commit SHA across both ship and review events is naturally done on board.jsonl already. Avoids introducing a new state file (multi-writer audit pressure).

Concrete event types:
- `review_window_opened` (metadata: commit_sha, builder, named_reviewers, timer_secs, opened_at)
- `review_response` (metadata: commit_sha, seat, response_type, was_named, body=text)
- `review_window_closed` (metadata: commit_sha, outcome, closed_at, timer_expired)

## MCP tool surface additions

Three new MCP tools:

1. **`review_ship(commit_sha, reviewers, body)`** — issued by builder. Posts ship broadcast with structured reviewers list. Opens review window server-side. Replaces ad-hoc "SHIPPED [SHA]" status messages.

2. **`review_respond(commit_sha, response_type, body)`** — issued by named reviewer or uninvited commenter. response_type ∈ {APPROVE, BLOCK, COMMENT}. Validates: uninvited cannot send BLOCK or APPROVE (only COMMENT).

3. **`review_get_state(commit_sha)`** — read-only. Returns the current state of a review window (open/closed, responses so far, timer remaining).

The 60s timer is server-driven (Tauri-side via the same opportunistic-tick pattern as Delphi's SHA-D10.4 sweeper). Per human msg 2583 directive ("the review window timer must auto-close, not wait for moderator intervention. Same sweeper pattern as D10.4 but for review windows"), the sweeper fires on:

1. Every `review_respond` MCP call (post-atomic) — check if window timer expired OR all named reviewers responded → close
2. Every `review_get_state` MCP call (pre-read) — same check; catches UI-poll cadence
3. Every `project_send` and `project_check` and `project_wait` keepalive_tick — same check; catches broader board polling pattern

Why all three: review windows have lower traffic than Delphi rounds; relying only on `review_respond` (Delphi's per-submission analog) would orphan a window with zero responses for the full timer. Adding broader board-event triggers ensures the close fires within timer + epsilon for any board-active seat.

**Close-event metadata:** `close_reason ∈ {"sweeper_quorum", "sweeper_timer_expired", "manual_close"}`. Same race-handling as D10.4 per architect msg 2461 Q4 envelope: swallow benign `[ReviewWindowAlreadyClosed]` errors from concurrent sweeper calls. Atomic close via single-write under `with_reviews_lock` (analog to `delphi_atomic_op`).

**Empirical motivation (msg 2582):** Review #5 (msg 2580/2581) took ~73 min to aggregate because the existing Continuous-discussion auto-aggregator at `discussion_control(mode=continuous)` is moderator-driven — close fires only when the moderator role's sidecar processes its tick. With moderator role-silent (per role design) AND active seats not invoking moderator-equivalent code paths, the aggregator was orphaned. Time-based events shouldn't depend on a specific role's polling. The opportunistic-broad-trigger sweeper closes this gap structurally.

## UI surface additions

1. **Launch row** — re-label "Start Continuous Review" to clarify it's the working-mode-not-discussion-mode. Same button position; resemanticized handler that toggles the working mode instead of starting a discussion.

2. **ShipModal** — appears when builder posts a ship via `review_ship` from UI. Lists active non-builder seats with checkboxes; enforces minimum-2 (if available); commit SHA field; optional body. "Ship" button posts the broadcast + opens review window.

3. **ReviewWindow component** — appears on the commit message card while review is open. Shows timer countdown, list of named reviewers with their response status (pending / approved / blocked / commented), uninvited comment count. Named reviewers see APPROVE / BLOCK / COMMENT buttons; uninvited see only COMMENT.

4. **Review-outcome chip** on accepted commit messages — `✓ reviewed by @X (APPROVE) @Y (APPROVE)` or `✗ BLOCKED by @X: <text>`. Integrates with the existing per-message economic footer.

5. **Working-mode toggle** in CollabTab header — replaces or augments existing Discussion Mode controls. Single source of truth for "what working mode is active": Assembly Line / Continuous Review / None.

## Discussion-mode resemanticization

The old `discussion_control(set, "continuous")` semantics (auto-triggered micro-rounds from status messages) is DEPRECATED. Migration:

- Existing `continuous` mode handlers in `discussion_control` removed or repurposed
- The launch-row "Continuous Review" button now starts the new working mode
- `discussion_mode` (open/directed) is orthogonal and unchanged — it governs message routing
- `working_mode` (assembly_line / continuous_review / none) is the new top-level state — it governs build + review orchestration

Recommendation: introduce `working_mode` as a first-class field in protocol.json:
```json
"working_mode": "continuous_review" | "assembly_line" | "none"
```

Today's `assembly_active: bool` becomes derivable from `working_mode == "assembly_line"`. Backward-compat shim during migration.

## Migration plan

1. **Architectural design** (this doc) — DONE; iterate per team feedback
2. **Spec amendments** based on adversarial review (evil-arch + dev-challenger likely to raise findings)
3. **Backend implementation** — new MCP tools `review_ship` / `review_respond` / `review_get_state`; new board.jsonl event types; working_mode field in protocol.json
4. **UI implementation** — ShipModal, ReviewWindow, working-mode toggle, launch-row resemanticization
5. **Deprecation** of `discussion_control(set, "continuous")` — separate commit; documented as a breaking change in the discussion-mode contract

All gated on Phase 1 hot-reload completing. Once Phase 1 ships, this work can begin.

## Pre-spec architectural questions for team input

1. **Reviewer list source — body parsing vs structured metadata.** Lean: structured `metadata.reviewers` field. Body `@mentions` are user-friendly but parser-dependent. Structured is parser-free and grep-able.

2. **Timer driver — opportunistic call (Delphi D10.4 pattern) vs background tick thread.** Lean: opportunistic. No new thread; same pattern as proven D10.4 sweeper. `review_respond` and `review_get_state` are the tick triggers.

3. **Working-mode launcher coupling.** Should Continuous Review standalone have a setup modal (like Oxford/Delphi) or be a direct toggle (no per-launch config beyond timer-default)? The directive implies the timer is "configurable in Assembly setup" — extend to Continuous Review's setup modal too. Lean: yes, both modes have setup modals where the timer + minimum-reviewers + any future params live.

4. **Backward-compat for the old `continuous` discussion mode.** Hard-deprecate vs gentle-migrate (warning broadcast on use). Lean: hard-deprecate — current usage is recent (this session); no historical data depends on it.

## Cross-references

- `feedback_planning_spiral_over_grep_and_fix` — the build/ship cycle for this should be tight; spec → adversarial review → backend → UI → ship
- `project_currency_gate_is_sender_side_enforced` — economic-backstop integration must remain robust under sidecar staleness (post-Phase 1 hot-reload, sender-side gate moves to centralized enforcement; this redesign benefits from that)
- `project_assembly_v3_2026-05-04` — prior Assembly Line design context
- `project_assembly_enable_drops_late_joiners` — late-joiner trap; the review system's "if 2+ seats available" check must consult current_active_seats, not enable-time snapshot
- `vision.md` "Currency system" section — economic backstop's interplay with the existing currency_objection flow

## Memory candidates

- `project_continuous_review_redesign_2026-05-28` — concrete design ratification + implementation start
- `feedback_working_mode_vs_discussion_mode_separation` — design principle: working modes (build orchestration) and discussion modes (message routing) are orthogonal axes
