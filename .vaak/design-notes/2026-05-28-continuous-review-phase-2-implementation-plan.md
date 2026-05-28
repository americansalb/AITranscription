# Continuous Review Phase 2 — backend implementation plan

**Owner:** architect:0
**Date:** 2026-05-28
**Status:** Locked, gated on Phase 1 hot-reload acceptance restart
**Parent spec:** `.vaak/design-notes/2026-05-28-continuous-review-redesign-spec.md`
**Predecessor (UI):** SHA-CR.1 → SHA-CR.5 chain shipped by ui-architect:0 (tonight, 17 UI commits)

---

## Scope

UI is DONE. Backend remaining (5 commits + 1 UI footer):

| # | Commit | Lane | What |
|---|---|---|---|
| 2a | SHA-CR.b.0 | developer:0 | `working_mode` field added to protocol.json with backward-compat shim: `assembly_active` derives from `working_mode == "assembly_line"`. Migration: existing `assembly_active: bool` reads continue working; new code writes `working_mode`. ~50 LOC. |
| 2b | SHA-CR.b.1 | developer:0 | `review_ship(commit_sha, reviewers, body, timer_secs)` MCP tool. Writes `review_window_opened` event to board.jsonl with metadata `{commit_sha, builder, named_reviewers, timer_secs, opened_at}`. Validates min-2 reviewers (if 2+ non-builder seats available). ~150 LOC. |
| 2c | SHA-CR.b.2 | developer:0 | `review_respond(commit_sha, response_type, body)` MCP tool. Writes `review_response` event to board.jsonl. Validates uninvited cannot send BLOCK/APPROVE. Post-atomic sweeper call. ~180 LOC. |
| 2d | SHA-CR.b.3 | developer:0 | `review_get_state(commit_sha)` MCP tool. Read-only aggregation over board.jsonl. Pre-read sweeper call. Returns ReviewWindowState (open/closed, responses, timer remaining). ~120 LOC. |
| 2e | SHA-CR.b.4 | developer:0 | Sweeper opportunistic wiring: `review_window_sweeper_maybe_close(project_dir)` called from `handle_project_send` + `handle_project_check` + `project_wait` keepalive_tick + Tauri-side wall-clock tick (the same `start_project_watcher` loop per arch msg 2627). ~80 LOC. |
| 2g | SHA-CR.6 | ui-architect:0 | Review-outcome chip on commit cards: `✓ reviewed by @X (APPROVE) @Y (APPROVE)` or `✗ BLOCKED by @X: <text>`. Derived from `reviewWindowByCommit` outcome. ~40 LOC TS. |

Total: ~580 LOC backend + ~40 LOC UI.

## Event type schemas (board.jsonl Option B per spec)

### `review_window_opened`

```json
{
  "type": "review_window_opened",
  "from": "system",
  "to": "all",
  "id": <next_message_id>,
  "timestamp": "<ISO>",
  "metadata": {
    "commit_sha": "abc1234",
    "builder": "developer:0",
    "named_reviewers": ["tester:0", "dev-challenger:0"],
    "timer_secs": 300,
    "opened_at_ms": 1234567890000,
    "ship_msg_id": <ref to ship broadcast>
  },
  "subject": "Review window opened for abc1234 by developer:0",
  "body": "Reviewers: @tester:0 @dev-challenger:0 · 5 min timer"
}
```

### `review_response`

```json
{
  "type": "review_response",
  "from": "<seat>",
  "to": "all",
  "id": <next_message_id>,
  "timestamp": "<ISO>",
  "metadata": {
    "commit_sha": "abc1234",
    "response_type": "APPROVE" | "BLOCK" | "COMMENT",
    "was_named": true | false,
    "responded_at_ms": 1234567890000
  },
  "subject": "<seat> <APPROVE|BLOCK|COMMENT> on abc1234",
  "body": "<reviewer reasoning>"
}
```

### `review_window_closed`

```json
{
  "type": "review_window_closed",
  "from": "system",
  "to": "all",
  "id": <next_message_id>,
  "timestamp": "<ISO>",
  "metadata": {
    "commit_sha": "abc1234",
    "outcome": "accepted" | "blocked",
    "close_reason": "sweeper_quorum" | "sweeper_timer_expired" | "manual_close",
    "closed_at_ms": 1234567890000,
    "named_responses": {"tester:0": "APPROVE", "dev-challenger:0": "APPROVE"},
    "uninvited_comments_count": 0
  },
  "subject": "Review window closed for abc1234: accepted",
  "body": "<close summary>"
}
```

## Sweeper logic (per architect msg 2627 ruling: Tauri-side wall-clock backstop)

```
fn review_window_sweeper_maybe_close(project_dir: &Path) -> Vec<CloseEvent> {
    let now_ms = now_unix_ms();
    let open_windows = aggregate_open_review_windows_from_board(project_dir);
    let mut closed = vec![];
    for w in open_windows {
        let elapsed = now_ms - w.opened_at_ms;
        let all_named_responded = w.named_reviewers.iter().all(|r| w.responses.contains_key(r));
        if elapsed >= w.timer_secs * 1000 || all_named_responded {
            // atomic close — if board already has close event for this commit, skip
            if !has_close_event(project_dir, &w.commit_sha) {
                let outcome = compute_outcome(&w);
                let close_reason = if all_named_responded { "sweeper_quorum" } else { "sweeper_timer_expired" };
                append_close_event(project_dir, &w, outcome, close_reason);
                closed.push(...);
            }
        }
    }
    closed
}
```

Race handling: the `has_close_event` check + `append_close_event` write are NOT atomic across processes; concurrent sweeper calls from multiple sidecars can both pass the check. Mitigation: append_close_event is wrapped in `with_board_lock`, AND the read-back-after-append check swallows `[ReviewWindowAlreadyClosed]` benign errors. Same race envelope as D10.4 (per architect msg 2461 Q4).

## Outcome computation

```
fn compute_outcome(w: &ReviewWindow) -> &'static str {
    if w.named_reviewers.iter().any(|r| w.responses.get(r) == Some(&"BLOCK")) {
        "blocked"
    } else {
        "accepted"  // includes timer-expired silence = APPROVE
    }
}
```

## Currency_objection backstop (unchanged)

Per spec: `currency_objection` remains available on ANY commit at ANY time, regardless of review outcome. The 50cu stake + dispute creation is independent of the review window. If a review window closes "accepted" and someone later files currency_objection, that dispute proceeds normally. Phase 2 backend does NOT touch currency_objection.

## working_mode field migration (SHA-CR.b.0)

```json
// protocol.json before
{
  "active_seats": [...],
  "rotation_order": [...],
  "current_speaker": "...",
  "assembly_active": false,
  ...
}

// protocol.json after SHA-CR.b.0
{
  "active_seats": [...],
  "rotation_order": [...],
  "current_speaker": "...",
  "working_mode": "none" | "assembly_line" | "continuous_review",
  // assembly_active retained as derived/shim for backward compat:
  "assembly_active": (working_mode == "assembly_line"),
  ...
}
```

Backward-compat: all existing reads of `assembly_active` continue working. New code writes `working_mode` directly + recomputes `assembly_active`. Phase 4 (or after a deprecation window) can remove `assembly_active`.

## Acceptance criteria

1. SHA-CR.b.0: protocol.json has `working_mode` field; existing `assembly_active` reads return same value
2. SHA-CR.b.1: a `review_ship` call from any seat writes a `review_window_opened` event; min-2-reviewer validation rejects malformed calls
3. SHA-CR.b.2: a `review_respond` call from named reviewer writes `review_response`; from uninvited with BLOCK/APPROVE → 400 `[UninvitedCannotApproveOrBlock]`
4. SHA-CR.b.3: `review_get_state` returns aggregated state matching the UI's `reviewWindowByCommit` computation (cross-check empirically)
5. SHA-CR.b.4: a review window opens, no responses within 300s, sweeper fires close from Tauri wall-clock tick (NOT requiring any sidecar to be polling). Live verification with all sidecars deliberately silent.
6. SHA-CR.6: chip renders correctly on accepted + blocked commits
7. Full end-to-end: builder ships → 2 reviewers respond APPROVE → window closes accepted → chip appears
8. End-to-end edge: builder ships → 1 reviewer BLOCK → window closes blocked → chip appears with BLOCK reason
9. End-to-end timer-edge: builder ships → 0 responses for 300s → wall-clock tick closes accepted (silence=APPROVE) → chip appears
10. currency_objection on accepted commit still works (independent backstop preserved)

## Sequencing

Order MATTERS: SHA-CR.b.0 first (protocol field) before any backend tool can reference `working_mode`. Then SHA-CR.b.1 → 2 → 3 → 4 → SHA-CR.6.

[BUILDER-CONFIRM-BEFORE-COMMIT]
Ruling ID: arch msg 2637 (this msg) / SHA-CR.b.0
Constraint: SHA-CR.b.0 must ship FIRST; all subsequent CR.b.* commits depend on protocol.json having `working_mode` field
Confirm in next CR.b.* commit broadcast: yes/no + reasoning

## Out of scope

- F11 verify_caller_identity middleware (in hot-reload Phase 2 scope; CR Phase 2 inherits it as Phase 2 infrastructure)
- IdempotencyCache (same — inherited)
- Phase 1 hot-reload completion (gated)
- discussion_control(set, "continuous") deletion — separate commit per CR redesign spec §Migration plan step 5; can be done in CR Phase 3 or later

## Risks

1. **board.jsonl as state store creates aggregation cost per `review_get_state` call.** O(n) walk over all messages to filter review_*. Mitigation: most commits have local message windows; bounded walk. If becomes hot, add in-memory index in Tauri.
2. **Sweeper called from 4 places** (review_respond + project_send + project_check + wall-clock tick + keepalive_tick) — race envelope per D10.4 pattern handles it; benign duplicate-close errors swallowed.
3. **working_mode shim drift:** if writes to `working_mode` and `assembly_active` diverge (bug), states out-of-sync. Mitigation: write path always recomputes `assembly_active` from `working_mode`.

## Post-acceptance follow-up

- CR Phase 3: deprecation removal of `discussion_control(set, "continuous")` handlers + `assembly_active` field after one full session of zero usage
- Reviews backlog UI (`ReviewHistoryTab` analog of Delphi completed-discussions panel)
- Per-reviewer review-quality reputation metric (out of scope for CR Phase 2; design parking)
