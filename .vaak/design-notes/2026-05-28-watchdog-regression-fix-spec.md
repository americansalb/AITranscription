# Watchdog `floor_stall` / `max_floor_exceeded` regression fix spec

**Author:** architect:0 (2026-05-28 ~15:10Z, after diagnosing the regression introduced by SHA-MW6.fix-2)

**Status:** spec only; no implementation. Queued for developer-lane next sweep.

## Problem

SHA-MW6.fix-2 (commit `e851569`) correctly closed the backend root cause of the reconnecting bug (human msg 2382): project_wait's messages-arrived early-return path bypassed the 30s heartbeat tick at vaak-mcp.rs:15043, leaving busy-seat per-seat heartbeat files stale while sidecars were actually alive. The fix: 1-line wire-up to call `update_session_heartbeat_in_file()` in the messages-arrived early return.

**Side effect:** the watchdog at `desktop/src-tauri/src/main.rs:7404` (max_floor_exceeded) and `7412` (floor_stall) both gate release on `!heartbeat_fresh`. The watchdog was implicitly designed around the pre-fix bug — it assumed busy seats that aren't doing tool work would go stale on heartbeat. Post-fix, `project_wait` always updates heartbeat, so the AND-condition (`idle_secs > stall_threshold_secs && !heartbeat_fresh`) never triggers for any seat doing project_wait.

**Net effect:** `floor_stall` and `max_floor_exceeded` are both permanently disabled for any agent polling project_wait. A speaker who stops broadcasting but keeps polling holds the mic indefinitely.

## Live evidence (2026-05-28 15:04-15:10Z)

evil-architect:0 held the mic for >20 minutes:
- `last_working_at` = 2026-05-28T14:49:27Z (stale ~21 min)
- `last_heartbeat` = 2026-05-28T15:08:45Z (fresh, ongoing)
- `activity` = "standby"
- `idle_secs` ≈ 1260s ≫ `stall_threshold_secs` = 180s
- `heartbeat_fresh` = TRUE
- AND-condition: `idle_secs > stall AND !heartbeat_fresh` = `TRUE AND FALSE` = **FALSE** → watchdog does not fire

The mic-hold persists until the speaker explicitly yields.

## Acceptance criteria

After the fix:
- A speaker who has been idle (no project_send or substantive tool call) for >180s should auto-release the mic via `floor_stall`, EVEN IF they are calling project_wait
- A speaker who has been on the mic >300s should auto-release via `max_floor_exceeded`, EVEN IF they are calling project_wait
- Existing protection: a speaker actively doing tool work (Read/Edit/Bash via PreToolUse/PostToolUse hooks) should NOT be auto-released (false positive risk)

## Three architectural options

### Option (a) — Drop the heartbeat gate from floor_stall

Change `main.rs:7412` to:
```rust
} else if idle_secs > stall_threshold_secs {
```

**Pros:** simplest 1-line fix. Restores `floor_stall` behavior for polling agents.

**Cons:** kicks tool-call workers as false positives. An agent doing a long Read or Edit between project_sends would idle the working-activity timer but their actual work is legitimate. Pre-MW6.fix-2, the PreToolUse/PostToolUse hook + Signal A RPC heartbeat path kept `last_alive_at_ms` fresh; that was the discrimination signal. Post-fix, project_wait also keeps `last_alive_at_ms` fresh, so the signal no longer discriminates.

### Option (b) — SEPARATE Signal-A RPC heartbeat for the gate

Add a new field `last_tool_call_at_ms` that is ONLY updated by the PreToolUse/PostToolUse hook + Signal A RPC, NOT by project_wait. Change `main.rs:7412` to read against `tool_call_fresh` instead of `heartbeat_fresh`:
```rust
} else if idle_secs > stall_threshold_secs && !tool_call_fresh {
```

**Pros:** proper architectural fix. Restores discrimination between "doing tool work" and "polling messages" with correct semantics. No false positives for tool-call workers.

**Cons:** requires new infrastructure (new field, new write paths, new read path in watchdog). ~30-50 LOC across hooks + sidecar + watchdog. More substantial.

### Option (c) — `max_floor_FORCE` ignoring heartbeat at longer threshold

Add a third watchdog branch BEFORE the existing two:
```rust
if rev_age_secs > ASSEMBLY_MAX_FLOOR_FORCE_SECS {
    // Force-release after absolute ceiling regardless of heartbeat or activity.
    // Polling agents (post-MW6.fix-2) will be caught by this branch.
    (
        "max_floor_force_exceeded",
        format!("held mic {}s past force-release ceiling of {}s — auto-release regardless of heartbeat", rev_age_secs, ASSEMBLY_MAX_FLOOR_FORCE_SECS),
    )
}
```

With `ASSEMBLY_MAX_FLOOR_FORCE_SECS = 1800` (30 min).

**Pros:** simplest backstop. Caps mic-hold regardless of any other condition.

**Cons:** doesn't fix `floor_stall` regression. 30-min ceiling is much longer than the intended 180s stall release. Still allows speakers to hold the mic 30x longer than the design intent in the post-fix world.

## Recommendation

Ship (b) as the proper fix. Ship (c) as immediate stopgap if (b) is multi-hour. Avoid (a) — false-positive on tool-call workers is a real UX regression.

If shipping (b): the new field `last_tool_call_at_ms` should be updated:
- In the PreToolUse hook (or equivalent) — bumps on every Read/Edit/Bash/etc. call
- In Signal A RPC handler if it exists
- NOT in project_wait

Both watchdog branches (`floor_stall` and `max_floor_exceeded`) gate release on `!tool_call_fresh` instead of `!heartbeat_fresh`. The `last_alive_at_ms` field continues to reflect sidecar liveness (correctly, per SHA-MW6.fix-2) but is no longer the watchdog's signal.

## Tests required

- Watchdog releases speaker after `stall_threshold_secs` of `last_working_at` staleness even with fresh project_wait calls
- Watchdog does NOT release speaker who is making PreToolUse-triggering tool calls (Read/Edit/Bash) within `stall_threshold_secs`
- max_floor_exceeded force-releases after `ASSEMBLY_MAX_FLOOR_SECS` (or `ASSEMBLY_MAX_FLOOR_FORCE_SECS` for option c)
- Regression: live evidence at 2026-05-28 15:10Z (evil-arch held mic 21 min) should be reproducible in a cargo test by mocking the sessions.json state.

## Cross-references

- vision.md "Multi-writer audit — running tally (as of 2026-05-28)" — NEW MW INSTANCE block describes this regression
- `.vaak/_human-inbox/architect-0-arbitration-and-data-2026-05-28.md` — empirical data + 3-option analysis
- SHA-MW6.fix-2 commit `e851569` — the heartbeat semantics fix that exposed the watchdog regression
- `.vaak/docs/multi-writer-contract.md` v6 — multi-writer audit doc; this regression is a new class instance

## Memory candidates

- `feedback_watchdog_assumed_busy_seat_heartbeat_bug_as_signal` — class-of-bug: subsystems with different semantic expectations of a shared field; fixing one breaks the other
- `project_watchdog_regression_post_mw6_fix2_2026-05-28` — concrete instance documentation
