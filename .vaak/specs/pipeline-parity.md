# Pipeline / Session Mode — Desktop ↔ Web-Service Parity Contract

**Version:** 0.1 (draft)
**Author:** Architect
**Date:** 2026-04-16
**Scope:** PR M + PR A + PR R + PR H surface only. Not a full session-mode taxonomy.
**Reviewers:** Platform-engineer (OS semantics), Tech-leader (arbitration).

---

## Purpose

The desktop app (`desktop/src-tauri/`) and web-service (`web-service/`) implement the same collaboration primitives against different substrates: file-based JSONL vs. Postgres. Without a contract, the two implementations drift — as Continuous mode did in early 2026.

This document defines the **minimum guarantees** that must hold identically on both substrates so that:
1. A session started on desktop and migrated to web-service (or vice-versa) preserves its state.
2. Behavior visible to participants (turn order, timeouts, vote-counts, moderator actions) is observationally identical.
3. Tests written against one substrate validate the semantics, not the transport.

Anything not in this document is a substrate-specific implementation choice.

---

## 1. Session identity

| Field | Desktop | Web-service | Parity requirement |
|---|---|---|---|
| `session_id` | UUID-v7 string in `session.json` | `sessions.id PRIMARY KEY UUID` | **Same value space** (UUID-v7 everywhere). Desktop generates via `uuid::Uuid::now_v7()`; web-service via `uuid-ossp` extension or app-level generation with `uuid` Python library. No auto-incrementing ints — cross-substrate correlation requires a shared ID space. |
| `started_at` | ISO-8601 UTC string | `timestamptz` | Both wire-format as ISO-8601 UTC in API responses. Store native type on each substrate. |
| `format` | `pipeline \| delphi \| oxford \| continuous` enum | Same enum via `VARCHAR CHECK IN (...)` or enum type | **Closed set.** Adding a format requires simultaneous migration on both substrates. |
| `mode` (legacy key) | Dual-key read during transition: `mode` OR `discussion_mode` | Dual-column read `format` OR `discussion_mode` for one release | Transition window: 1 release. Writers emit new key only. Readers accept both. Drop legacy after transition. |

**Why UUID-v7 specifically:** embeds 48-bit ms timestamp + 74 bits randomness. Intra-process collisions handled by the crate; cross-substrate ordering acceptable within NTP skew (see § 6).

---

## 2. Participant identity

| Field | Desktop | Web-service | Parity requirement |
|---|---|---|---|
| `role_instance` | `"developer:0"` string | `VARCHAR(64)` same format | **Regex-validated** `^[a-z][a-z0-9-]*:[0-9]+$`. Rejected at write on both substrates. |
| Session key | `{ppid, started_at_ms}` tuple | JWT subject + connection ID | Substrate-specific liveness mechanism; neither exposed cross-substrate. **What must be consistent**: `is_session_alive(role_instance)` returns the same answer on both substrates for the same participant. |
| PID reuse guard | Started-at timestamp in key | JWT `jti` + `exp` | Desktop's Windows PID-reuse concern (per vision § 11.4) does not apply to web-service's JWT path. Both prevent zombie sessions. |

---

## 3. Moderator and manager roles

Per vision § 11.3, these are **two distinct privileged roles** with different capability sets. Parity requirement:

| Capability | Owner role | Desktop check | Web-service check |
|---|---|---|---|
| `ReorderPipeline` | moderator | `vaak-mcp.rs:2170` `moderator_only_actions` list | `sessions.moderator_role_instance` match + RBAC |
| `JumpToStage` | moderator | same | same |
| `PauseSession` / `ResumeSession` | moderator | same | same |
| `EndSession` | moderator | same | same |
| `SpeakOutOfTurn` | both moderator and manager | `vaak-mcp.rs:3473, 3550` bypass | connection-level role check |
| `DirectMessageHuman` | manager only | `vaak-mcp.rs:3533` | manager-role-id check + dedicated route |

**Fallback invariant (vision § 11.4):** both substrates consult `active_moderator_session()` with `human:0` fallback when vacant. Fallback semantics are identical.

**Manager invariant (vision § 11.4b):** manager capabilities consult `active_manager_session()` without fallback. A vacant manager seat does NOT promote human:0 to manager privileges on either substrate.

---

## 4. Tiered reason-required actions (PR M scope)

| Action | Reason required | Min length after trim | Error variant |
|---|---|---|---|
| `reorder_pipeline` | yes | 3 chars | `ModeratorError::ReasonRequired` |
| `jump_to_stage` | yes | 3 chars | `ModeratorError::ReasonRequired` |
| `skip_participant` | yes | 3 chars | `ModeratorError::ReasonRequired` |
| `end_discussion` / `end_session` | yes | 3 chars | `ModeratorError::ReasonRequired` |
| `pause` / `resume` | no | — | — |
| `speak_out_of_turn` | no | — | — |

**Parity requirement:** both substrates reject the same inputs with the same error variant. `"   "` (whitespace only), `""` (empty), `"ok"` (too short) all fail identically. Error variant must be an enum, not a string — UI consumes for tooltip rendering (vision § 11.5, dev-challenger attack 2).

---

## 5. Audit metadata

Every privileged action emits a message carrying:

```json
{
  "moderator_action": {
    "action": "end_session",
    "reason": "Consensus reached, closing per synthesis vote.",
    "timestamp": "2026-04-16T18:00:00Z",
    "actor": "moderator:0",
    "affected_role": "developer:0"
  }
}
```

**Parity requirement:**
- Field names identical on both substrates (snake_case).
- `timestamp` is ISO-8601 UTC server time, not client-supplied.
- `actor` is the full `role_instance` string.
- `affected_role` optional; null/absent when action affects the whole session.

Queryability: both substrates must support filtering messages by `moderator_action.action` and by `actor`. Desktop uses a JSONL scan; web-service uses JSONB GIN index on the `metadata` column. Same query returns same results.

---

## 6. Clock and timing

| Concern | Desktop | Web-service | Parity requirement |
|---|---|---|---|
| `deadline_at_server` in turn notifications | Computed from `std::time::Instant` + `SystemTime` | Computed from `time.monotonic()` + `datetime.utcnow()` | Both emit wall-clock ISO-8601 UTC in the message; internally use monotonic for deadline math. Clients may drift up to NTP sync interval; display includes staleness marker (vision / UX spec). |
| Pipeline advance monotonicity | `Instant::now()` never regresses | `time.monotonic()` never regresses | Both substrates reject a session_id's `completed_stages` going backwards. |
| Suspend handling | Laptop-sleep pauses `Instant` on Windows/macOS | Server doesn't suspend; web-service deadline fires on schedule | **Divergence accepted.** Desktop must document pause-on-suspend semantics; web-service doesn't need them. Test matrices differ here. |

---

## 7. Auto-termination (PR A)

Consensus check fires when **every participant in the most recently completed round** has posted a message with `metadata.vote: accept` on the same `on: <msg_id>`. Snapshot semantics (vision § 11.8).

**Parity requirement:**
- Round boundary detection: same on both substrates.
- Vote classification: `accept` / `reject` / `defer` values are a closed enum; anything else is rejected at write.
- Most-recent-vote-wins: both substrates scan in reverse message order per `role_instance`.
- Self-vote counts (no synthesizer exclusion): vision § 11.8.

On fire:
1. Emit `session_terminated_by_consensus` system message.
2. Transition `sessions.state` from `active` to `terminated_by_consensus`.
3. Do not emit on paused sessions (vision § 11.7).

---

## 8. Moderator-exit auto-pause (PR M scope)

When the moderator session becomes non-live (see § 2), the session auto-pauses:

- Desktop: file-mtime of moderator's heartbeat record exceeds threshold OR explicit `project_leave` from moderator.
- Web-service: WebSocket disconnect + grace period OR explicit leave.

**Parity requirement:** both emit `session_paused_moderator_offline` system message. Both set `sessions.paused_at`. Both defer PR A's auto-termination while paused. Both allow resume only on `manager` claim — not on any other role.

---

## 9. Rename (PR R) — file and schema migrations

### Desktop

- `.vaak/discussion.json` → `.vaak/session.json` on first read with old file present.
- Migration is idempotent, one-way, runs inside the existing board-file lock.
- Windows path uses `MoveFileExW + MOVEFILE_REPLACE_EXISTING`, retries 3× with 250ms backoff, aborts cleanly on final failure (releases lock).
- POSIX uses `std::fs::rename`.

### Web-service

**No rename required.** Web-service never used the "discussion" term in its schema. Tables already use `sessions`, `messages`, `agent_state`. Parity contract applies forward: new desktop reads/writes use `session` terminology; web-service is unchanged.

**If this assumption fails at code review:** file a follow-up PR. Do not block PR R on web-service work.

---

## 10. Human-channel filter (PR H) — client-side only

Tab filter is pure UI. No backend parity.

- Desktop: `CollabTab.tsx` filters `project.messages[]` by `from` / `to` (NOT body text — vision § 11 adjacency to `@human` formatting vs. capability distinction).
- Web-service: same filter logic applied client-side in `web-client/`.

**Parity requirement:** filter predicate identical. No server-side filtering; the `messages` API returns the full set and the client chooses what to show.

---

## 11. Format-gating

Per vision § 11.5, capabilities declare `allowed_formats`. A call to a capability not valid for the active session's format returns `ModeratorError::CapabilityNotSupportedForFormat { capability, format }` on both substrates.

UI behavior: disabled-with-tooltip on both. The UI layer is shared conceptually (same labels, same tooltips) even where the transport differs.

---

## 12. Divergence allowed (explicit)

Not everything needs to match. The following are substrate-specific and deliberately not in parity:

- Storage transport (JSONL vs. Postgres)
- Locking mechanism (file lock vs. row lock)
- Notification transport (file mtime vs. WebSocket)
- Session liveness detection (PID+timestamp vs. JWT+connection)
- Suspend handling (desktop-only)
- Backup / export format (each substrate picks its own)

---

## 13. Test strategy

Two classes of tests:

1. **Substrate-specific tests**: run on one side only. Lock semantics, clock source, file migration.
2. **Parity tests**: run identical scenario on both substrates, hash-compare the resulting state. Required for every item in §§ 1–8 and § 11.

Parity test harness owned by tester (follow-up work, not in this spec).

---

## 14. Open questions

Flagged for resolution before PR M merges:

1. **Web-service rename assumption (§ 9)**: developer or tester confirms `web-service/` has no `discussion` field. If present, file follow-up PR.
2. **Parity test harness**: tester scopes after PR M lands. Not blocking this contract.
3. **Lamport counter for causal ordering**: per platform-engineer msg 175, not needed today. Revisit if cross-substrate message ordering becomes load-bearing.

---

## 15. Change log

| Date | Change | Author |
|---|---|---|
| 2026-04-16 | Initial draft (0.1) | Architect |

---

*This spec follows the narrative-comment standard (vision § 11.11) — every requirement carries a "why" (either inline or via vision reference). Future edits must preserve that standard.*
