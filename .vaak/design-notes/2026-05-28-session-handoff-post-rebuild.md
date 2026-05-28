# 2026-05-28 session handoff — post-rebuild

**Owner:** architect:0
**Time:** 2026-05-28 23:00Z
**Status:** Tonight's `npm run tauri build` SUCCEEDED (exit 0). Fresh `vaak-desktop.exe` at `desktop\src-tauri\target\release\vaak-desktop.exe`. Human about to close all CC windows + relaunch Vaak via desktop .bat. This doc captures session state for the next architect:0 to read on rejoin.

---

## What just landed (full chain bundled in fresh exe)

**Phase 1 hot-reload pilot (9 commits):**
- SHA-HR.1.1 `9795df9` developer:0 — protocol_active_seats_set → mcp_handlers/assembly_line.rs
- SHA-HR.1.2 `79c6703` developer:0 — seed_rotation_order_if_empty + force + protocol_normalize_in_place moved
- SHA-HR.1.3 `48709c0` developer:0 — do_protocol_mutate_inner set_preset arm via Option (a) serde_json::Value round-trip
- SHA-HR.1.2b `a905f31` developer:0 — apply_set_preset moved
- SHA-HR.1.4 `3588f70` developer:0 — Tauri POST /mcp/assembly_line endpoint in start_speak_server
- SHA-HR.1.4.token `ec84b58` developer:0 — F9 token-file ACL fail-closed
- SHA-HR.1.5 `8ebcd17` developer:0 — handle_assembly_line shrunk to ureq HTTP forwarder + retry+backoff+jitter
- SHA-HR.1.6 `572c1d9` developer:0 — `_hot_reload_phase: 1` sentinel canary

**Continuous Review backend fixes (3 commits):**
- SHA-CR.sweeper `49cafbf` developer:0 — auto_close_timed_out_round wired into project_wait 30s tick
- SHA-CR.timeout `53f067c` developer:0 — default review-window timeout 60s → 300s (5min)

**UI redesign + Continuous Review Phase 4 UI (21 commits + 83 frontend tests):**
- SHA-LR.1 / LR.2 / LR.3 — unified launch row (Oxford / Delphi / Assembly / Continuous)
- SHA-CR.1 / CR.2 / CR.2.1 / CR.3 / CR.4 / CR.5 / CR.5.1 / CR.6 / CR.6.1 — ShipModal + ReviewWindow + ReviewOutcomeChip + extractor track
- SHA-CR.econ-stale `6f06310` — "no ledger" placeholder for chronic gold-display gap
- SHA-FF.1 / FF.2 / FF.3 / FF.4 — flow-feed batcher + currencyDisplay + workflowTypes extracted with tests

**Spec deliverables (9 architect-lane commits):**
- vision.md late-session entry (`914b33c`)
- Phase 2 currency migration plan (`f1de9b7`)
- [BUILDER-CONFIRM-BEFORE-COMMIT] header pattern (`471394f`)
- F3 audit placeholder (`adbaa34`) + complete (`75df883`) + NR6-NR9 amend (`ecd4102`)
- Phase 2 plan amend (`4fe5569`)
- Phase 3 stub (`0396aeb`)
- CR Phase 2 implementation plan (`707c1ae`)

**Memory file:** `feedback_opportunistic_substrate_reliance_degrades_in_silence.md`

## Phase 1 acceptance canary

After human restarts Vaak via the new exe:
1. Reopen Claude Code windows (per `project_sidecar_relaunch_requires_claude_code_restart` — sidecar caches per-PPID)
2. Call `assembly_line(action="enable")` from any CC session
3. Observe `_hot_reload_phase: 1` in result → canary FIRED → Phase 1 acceptance gate CLOSED
4. SHA-HR.2.0 (middleware stack: F9 + F11 + F6) can begin

## What's queued post-canary

**Hot-reload Phase 2 currency migration (7 commits, RISK-CLASS grouping per arch msg 2654):**

| # | Commit | What |
|---|---|---|
| SHA-HR.2.0 | dev:0 | Middleware stack: IdempotencyCache (key on `(request_id, source_session_id)` per NR6; 400 on missing X-Vaak-Request-Id per NR4) + F11 verify_caller_identity (5-step derivation per arch msg 2662 — NOT bindings.status per NR9) + F9 token check (shipped). Cache write LAST INSIDE lock per NR7. |
| SHA-HR.2.1 | dev:0 | currency_balance + currency_ledger (lowest risk; preserve NR1 replay-on-read + NR3 supply invariant 31,971 cu baseline) |
| SHA-HR.2.2 | dev:0 | Unrestricted mutators (system_dispute + call_judge + objection; NR2 multi-party check for objection) |
| SHA-HR.2.3 | dev:0 | Identity-bound mutators (claim/abandon/submit/approve/reject_bounty + concede + dispute_message) |
| SHA-HR.2.4 | dev:0 | ROLE-GATED HIGHEST-RISK (human_adjust + judge_ruling + post_bounty) with NR8 kick-effect tests |
| SHA-HR.2.5 | dev:0 | Sidecar cleanup: delete 15 handle_currency_* fns + 2 dispatcher arms; ~2000 LOC shrinkage |
| SHA-HR.2.6 | tester:0 | Post-migration sum-walk: total === 31,971 cu ± transient escrow (per NR3 baseline from msg 2628) |

Per NR5: full chain ships to production atomically (no partial release).

**Continuous Review Phase 2 backend (6 commits):**
- SHA-CR.b.0 working_mode field in protocol.json + backward-compat shim
- SHA-CR.b.1 review_ship MCP tool + review_window_opened board event
- SHA-CR.b.2 review_respond MCP tool + review_response event
- SHA-CR.b.3 review_get_state MCP tool + pre-read sweeper
- SHA-CR.b.4 sweeper wiring (project_send + project_check + keepalive_tick + Tauri wall-clock tick per arch msg 2641 — insertion point main.rs:7584)
- SHA-CR.6 ui-arch:0 review-outcome chip (ALREADY SHIPPED tonight as 0259a68 — strike from queue)

**SHA-CR.bug.gold-display (independent, pre-Phase-2):**
- Fix main.rs:3960-3963 to derive active-ness from `last_alive_at_ms` heartbeat freshness (60s window) instead of literal `status="active"` filter
- ~5 LOC; scaffold in arch msg 2657
- Waiting on tester:0 empirical sessions.json:bindings:status report (was pending at session end)

## F3 audit summary (all 10 NR findings)

| # | Risk | Status |
|---|---|---|
| NR1 | currency_balance replay-on-read is a write | Preserve OR cleanly split paths |
| NR2 | Multi-party identity binding pattern | `dispute.participants.contains(&caller)` shape |
| NR3 | replay_balances_from_ledger preserves 31,971 cu supply invariant | SHA-HR.2.6 final test |
| NR4 | Missing X-Vaak-Request-Id → 400 (not fail-open) | Locked |
| NR4b | sessions.json:bindings:status multi-writer dependency | Independent fix SHA-CR.bug.gold-display |
| NR5 | Cross-handler atomic chain risk under partial migration | Atomic production release |
| NR6 | IdempotencyCache replay-within-TTL attack | Cache key `(request_id, source_session_id)` |
| NR7 | Cache write-after-lock race | Cache write INSIDE lock at end |
| NR8 | Kicked-seat binding revocation mid-session | In-flight complete; subsequent fail 403 |
| NR9 | F11 may inherit bindings.status multi-writer fragility | F11 reads session_id binding + last_alive_at_ms ONLY |

## Spec docs to read on rejoin

- `.vaak/vision.md` — full session chain documented (sections 2026-05-28 SHA-D10.4 + 2026-05-28 late session)
- `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md` — 5-phase migration plan + F1-F11 amendments + [BUILDER-CONFIRM-BEFORE-COMMIT] pattern
- `.vaak/design-notes/2026-05-28-hot-reload-phase-2-currency-migration-plan.md` — Phase 2 sequence + F11 middleware stack
- `.vaak/design-notes/2026-05-28-hot-reload-phase-2-f3-state-residency-audit.md` — F3 audit COMPLETE with NR1-NR9
- `.vaak/design-notes/2026-05-28-hot-reload-phase-3-migration-stub.md` — Phase 3 scope
- `.vaak/design-notes/2026-05-28-continuous-review-redesign-spec.md` — CR redesign architecture
- `.vaak/design-notes/2026-05-28-continuous-review-phase-2-implementation-plan.md` — CR backend 6-commit chain
- `.vaak/design-notes/2026-05-28-unified-launch-row-ui-spec.md` — UI launch-row spec

## Open items at handoff

1. **Phase 1 canary** — fires on operator first call to `assembly_line` after relaunch
2. **SHA-CR.bug.gold-display** — waiting on tester:0 sessions.json data, then ~5 LOC patch
3. **Architect-lane backlog DRY pre-restart** — post-canary, next architect work is reviewing dev:0 Phase 2 commits + tester:0 supply-invariant checks

## Calling out to next-session architect

- Read this doc + vision.md late-session FIRST
- Phase 1 canary should fire within minutes of restart — confirm via dev:0 broadcast OR call assembly_line yourself if needed
- If canary FIRES → ratify with team + green-light SHA-HR.2.0
- If canary DOESN'T fire → investigation lane; check sidecar logs + verify Tauri-side endpoint reachable
- Maintain Rule 1 (never sit in project_wait when work exists) but respect Rule 4 (ONE answers status questions)
- Build-cycle blind-window pattern: rebuild takes 10-15 min; broadcast pre-commit so team doesn't debate already-shipped work
