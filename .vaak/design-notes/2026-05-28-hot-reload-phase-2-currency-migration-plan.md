# Hot-reload Phase 2 — `currency_*` migration plan

**Owner:** architect:0
**Date:** 2026-05-28
**Status:** Draft, pre-restart (Phase 1 acceptance still pending operator restart canary)
**Parent spec:** `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md`
**Prerequisite:** Phase 1 hot-reload `_hot_reload_phase: 1` canary observed in `assembly_line` response post-restart

---

## Goal

Migrate all 15 `currency_*` MCP handlers from the `vaak-mcp.rs` sidecar to the Tauri main process (`desktop/src-tauri/src/mcp_handlers/currency_*.rs`), preserving exact semantics + currency-economic invariants while eliminating sidecar-cached handler logic. After Phase 2, currency rules can be hot-patched with a Tauri-only restart; sidecar tool surface remains stable.

## Scope: 15 handlers

| # | Tool | Kind | sidecar fn (vaak-mcp.rs) | Migrates to |
|---|---|---|---|---|
| 1 | `currency_balance` | READ | inline at line 18554 | `mcp_handlers/currency_balance.rs` |
| 2 | `currency_ledger` | READ | inline at line 18617 | `mcp_handlers/currency_ledger.rs` |
| 3 | `currency_human_adjust` | MUTATE | `handle_currency_human_adjust` 12474 | `mcp_handlers/currency_human_adjust.rs` |
| 4 | `currency_post_bounty` | MUTATE | `handle_currency_post_bounty` 10286 | `mcp_handlers/currency_bounty.rs` (shared module — post/claim/abandon/submit/approve/reject) |
| 5 | `currency_claim_bounty` | MUTATE | `handle_currency_claim_bounty` 12499 | (shared `currency_bounty.rs`) |
| 6 | `currency_abandon_bounty` | MUTATE | `handle_currency_abandon_bounty` 12543 | (shared `currency_bounty.rs`) |
| 7 | `currency_submit_bounty` | MUTATE | `handle_currency_submit_bounty` 12596 | (shared `currency_bounty.rs`) |
| 8 | `currency_approve_bounty` | MUTATE | `handle_currency_approve_bounty` 12633 | (shared `currency_bounty.rs`) |
| 9 | `currency_reject_bounty` | MUTATE | `handle_currency_reject_bounty` 12696 | (shared `currency_bounty.rs`) |
| 10 | `currency_objection` | MUTATE | `handle_currency_objection` 12750 | `mcp_handlers/currency_objection.rs` |
| 11 | `currency_call_judge` | MUTATE | `handle_currency_call_judge` 12967 | `mcp_handlers/currency_dispute.rs` (shared module — call_judge/judge_ruling/system_dispute/concede/dispute_message) |
| 12 | `currency_judge_ruling` | MUTATE | `handle_currency_judge_ruling` 13022 | (shared `currency_dispute.rs`) |
| 13 | `currency_system_dispute` | MUTATE | `handle_currency_system_dispute` 13160 | (shared `currency_dispute.rs`) |
| 14 | `currency_concede` | MUTATE | `handle_currency_concede` 13217 | (shared `currency_dispute.rs`) |
| 15 | `currency_dispute_message` | MUTATE | `handle_currency_dispute_message` 13346 | (shared `currency_dispute.rs`) |

Total: 2 read + 13 mutate.

Module grouping (4 files): `currency_balance.rs` + `currency_ledger.rs` + `currency_bounty.rs` + `currency_objection.rs` + `currency_dispute.rs` — five files total, three shared by tool family. Per-file LOC estimate ranges from ~60 (balance) to ~600 (dispute module). Total ~2000 LOC moved out of sidecar.

## F3 state-residency audit prerequisite (per evil-arch msg 2434 + dev-challenger lead invite)

Per handler enumerate the following BEFORE migration:

1. **What state does this handler read?** (currency.jsonl tail, balances.json, claims.json, board.jsonl, project.json, .vaak/sessions/*.json, etc.)
2. **What state does this handler write?** (currency.jsonl append, balances.json update, board.jsonl append, etc.)
3. **What locks does it acquire?** (currency.lock, board.lock, etc.) — order matters for deadlock-avoidance
4. **What downstream effects?** (does it call `auto_close_timed_out_round` opportunistically? Does it trigger a hook? Does it call `project_send` internally?)
5. **Sender-identity dependency?** Currency gating is sender-side; the Tauri-side handler MUST receive the canonical (`role`, `instance`) tuple from the sidecar proxy POST — not derive it from process context (sidecar IS the sender; Tauri is not). **F11 trust-model shift documentation** covers this.
6. **File-op-hook interaction?** Per `project_currency_edit_test_earns_dead` memory: file-op-claim.py hook lookup chain. If the handler is involved in edit/test earn flows, verify the hook still reaches the right handler post-migration. The hook runs in the sidecar's lifecycle; the handler it reaches via proxy needs to honor the marker file.

**Invitation reaffirmed:** dev-challenger:0 to lead this audit pre-Phase-2-implementation. Output deliverable is one per-handler row in this doc's §"State-residency audit results" section (currently empty placeholder).

## F6 idempotency contract — Mutating-tool double-execute prevention

Per the hot-reload spec §F6 ruling: mutating tools can double-execute on retry. Sidecar generates `X-Vaak-Request-Id: <UUID v4>` per outbound POST; Tauri caches request_id → response for at least 60s after first execution; second POST with same request_id returns cached response without re-executing. Cache keyed JUST on request_id (not request_id + tool_name) so a sidecar bug that reuses a request_id across tools is detected as a 409 Conflict.

**Phase 1 SHA-HR.1.5 retry helper already generates request_id** (per architect spec §F6 implementation note). Phase 2's first commit MUST add the Tauri-side `IdempotencyCache` struct + middleware before any handler migration to avoid double-execute window during the migration itself.

**Cache invariants:**
- Insert: key = `request_id`, value = `(tool_name, response_body, inserted_at_ms)`
- Lookup: hit → return `response_body` if `tool_name` matches, else `409 IdempotencyKeyReused`
- TTL: 60s after `inserted_at_ms` — chosen because retry helper's exponential backoff caps at ~30s + safety margin
- Eviction: simple lazy on-lookup + on-insert sweep; bounded to 10K entries to prevent unbounded growth under attack
- Persistence: in-memory only; restart wipes the cache. This is acceptable because post-restart the sidecar regenerates fresh request_ids; the only window of risk is during Tauri restart while sidecar is mid-retry, which Phase 5 auto-detect-restart handles by re-handshake.

## F11 trust-model shift documentation

**Pre-Phase-2 trust model:** sidecar enforces currency rules at the handler level (e.g., `currency_objection` checks payer balance against `currency.lock`-protected balances.json, deducts stake atomically, posts to currency.jsonl). The sidecar IS the trusted enforcement boundary.

**Post-Phase-2 trust model:** Tauri main process becomes the trusted enforcement boundary; sidecar becomes a relay. The `X-Vaak-Token` header (per msg 2426 Q1 + F9 SHA-HR.1.4.token) authenticates "this POST came from the Vaak-installed sidecar," not "this POST is authorized to act on behalf of role X." The (role, instance) tuple in the POST body is asserted by the sidecar but enforced by Tauri.

**Risks the shift introduces:**
1. **Sidecar spoofing of (role, instance):** A compromised or modified sidecar could POST `role=human, instance=0` for arbitrary currency_human_adjust grants. Mitigation: Tauri verifies the (role, instance) against the sidecar's PPID-bound session_id from `sessions.json` — same liveness file the watchdog uses. A POST claiming an identity not bound to the calling sidecar's session_id returns 403.
2. **Replay across sidecar restarts:** A captured POST replayed by a stale sidecar attempts to act as the original sender. Mitigation: request_id idempotency cache catches duplicates within TTL; X-Vaak-Token rotation on Tauri restart catches cross-restart replays.
3. **Token-file leak:** Token-file ACL (F9 SHA-HR.1.4.token) reduces but does not eliminate. Defense-in-depth: token rotation on Tauri startup (already implemented per F9).

**Risks the shift REMOVES:**
1. **Stale-sidecar currency-gate bypass** (per `project_currency_gate_is_sender_side_enforced`) — IS the primary motivation. A stale sidecar no longer holds enforcement; it just proxies. Currency rule updates take effect on Tauri restart.
2. **Per-sidecar rule drift** — multiple seats running different sidecar versions had divergent enforcement. Post-migration, all enforcement is centralized in one Tauri process.

## Per-handler migration commit sequence (proposed)

| # | Commit | Lane | What |
|---|---|---|---|
| 1 | SHA-HR.2.0 | developer:0 | Add `IdempotencyCache` struct + middleware in main.rs `start_speak_server`. POST handlers receive `X-Vaak-Request-Id` header; cache lookup before invocation. Tests: replay same request_id → cache hit; replay with different tool_name → 409. |
| 2 | SHA-HR.2.1 | developer:0 | Migrate `currency_balance` (READ, simplest). New `mcp_handlers/currency_balance.rs`. Sidecar inline code at vaak-mcp.rs:18554 becomes ureq POST to `/mcp/currency_balance`. **Canary:** sidecar response gets `_hot_reload_phase: 2` sentinel. |
| 3 | SHA-HR.2.2 | developer:0 | Migrate `currency_ledger` (READ, also simple). Same pattern. |
| 4 | SHA-HR.2.3 | developer:0 | Migrate `currency_human_adjust` (MUTATE, simplest mutating — single balance update). Idempotency cache live in this commit. |
| 5 | SHA-HR.2.4 | developer:0 | Migrate bounty module (6 tools: post/claim/abandon/submit/approve/reject) as ONE shared `mcp_handlers/currency_bounty.rs` module. Single commit to preserve module-internal helpers (e.g., `find_bounty_by_id`). |
| 6 | SHA-HR.2.5 | developer:0 | Migrate `currency_objection` (MUTATE, most cross-cutting — touches dispute creation + escrow). |
| 7 | SHA-HR.2.6 | developer:0 | Migrate dispute module (5 tools: call_judge/judge_ruling/system_dispute/concede/dispute_message) as `mcp_handlers/currency_dispute.rs`. |
| 8 | SHA-HR.2.7 | developer:0 | Sidecar cleanup: delete the 15 sidecar-side `handle_currency_*` functions; verify sidecar binary shrinks ~2000 LOC; verify `cargo build` clean. |

Per-commit verification:
1. `cargo build --release` clean (both sidecar + Tauri)
2. `cargo build --release --bin vaak-mcp` clean (sidecar standalone)
3. Test the migrated handler via fresh CC session: read-only → expected balance; mutate → ledger row appears + balance updates
4. **Hot-reload verification (every Phase 2 commit):** Tauri restart only (Vaak.exe relaunch); sidecar process untouched; next handler call hits new logic. Validates the hot-reload promise.

**Sequencing rule (per build-cycle-blind-window class-of-bug):** each commit's broadcast must cite the previous commit's SHA explicitly + state `Phase 2 step N/8`. The `[BUILDER-CONFIRM-BEFORE-COMMIT]` header pattern (proposed in tonight's vision.md update) applies.

## Phase 2 acceptance gate

Phase 2 acceptance = ALL of:
1. All 15 handlers respond from Tauri (not sidecar). Sentinel `_hot_reload_phase: 2` in every currency_* response.
2. Idempotency cache verified empirically: scripted retry of same request_id returns cached response in <50ms (vs ~5-50ms re-execution).
3. F11 risks 1-3 verified with manual tests: spoof attempt 403'd, replay across restart blocked, token leak survives ACL.
4. Currency invariants preserved across migration: total copper supply stable (sum of balances + escrow + reserve = constant) — testable via `currency_balance` for all seats + ledger sum-walk.
5. Live multi-seat economic round produces same outcomes as pre-Phase-2 (regression test). At least one full Continuous Review window with currency_objection backstop, end-to-end.

## Open architect-lane questions (NOT blocking; for evil-arch or dev-challenger pre-Phase-2)

1. **F3 audit ownership:** dev-challenger:0 invited per spec §F3. Reaffirm willingness.
2. **F11 spoofing mitigation enforcement scope:** Q is whether Tauri verifies (role, instance) against PPID-bound session_id on EVERY currency_* POST, or only on currency_human_adjust (which has unbounded grant power). Architect-lane lean: every POST, because cheapness (one hashmap lookup).
3. **Hot-reload during Phase 2:** can a partially-migrated state (e.g., 8 handlers Tauri-side, 7 still sidecar-side) ship to production? **Architect-lane ruling:** YES, but only between Phase 2 commits 4-7 if the migration is paused mid-way. The mixed state preserves correctness because each handler is end-to-end-consistent in its current location. **Do not pause mid-bounty-module** (commit 5) because module-internal helpers are shared.

## Backlog / deferred (not Phase 2)

- Phase 3.5 `tiny_http` → thread-pool / async upgrade — currently single-threaded server. Phase 2 throughput likely fine; Phase 4 `project_send` migration probably forces the upgrade.
- File-op-hook (`file-op-claim.py`) Tauri-side analog — currently runs in sidecar lifecycle; long-term should migrate too.
- `currency.lock` re-architecture — currently file-lock; if Tauri becomes the only writer, an in-process mutex is faster and equally correct.

## Next architect-lane work after this plan publishes

1. Draft `[BUILDER-CONFIRM-BEFORE-COMMIT]` header pattern as a spec amendment to the hot-reload spec
2. Open the F3 state-residency audit doc placeholder so dev-challenger can fill in the per-handler rows
3. Phase 3 oxford/delphi/assembly migration plan (post-Phase-2 acceptance)
