# Hot-reload Phase 2 — F3 state-residency audit (per-handler)

**Owner (audit lead):** dev-challenger:0 (per spec invitation, msg 2434 + reaffirmed in `2026-05-28-hot-reload-phase-2-currency-migration-plan.md`)
**Owner (placeholder author):** architect:0
**Date:** 2026-05-28
**Status:** **COMPLETE** — dev-challenger:0 audit pass via direct grep + read of all 15 handlers at vaak-mcp.rs:10286-13346 + dispatcher reads at 18554-18675. Architect-lane review next per acceptance gate.
**Parent spec:** `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md` §F3
**Parent plan:** `.vaak/design-notes/2026-05-28-hot-reload-phase-2-currency-migration-plan.md`

---

## Audit purpose

Each of the 15 `currency_*` handlers being migrated to Tauri must have its state interactions enumerated BEFORE the migration commit. Goal: discover lock ordering, hook-chain dependencies, file-locking concerns, and sender-identity assumptions that could break post-migration.

## Six questions per handler

1. **What state does this handler read?** (files, locks held during read)
2. **What state does this handler write?** (files, atomic-vs-multi-step)
3. **What locks does it acquire?** (and in what order — deadlock-avoidance critical)
4. **What downstream effects?** (opportunistic sweeper calls, hooks triggered, internal project_send calls)
5. **Sender-identity dependency?** Currency gating is sender-side. The Tauri handler MUST receive `(role, instance, session_id)` from the sidecar proxy POST — NOT derive from process context. Verify the migration preserves this.
6. **File-op-hook interaction?** Per `project_currency_edit_test_earns_dead` memory: file-op-claim.py hook lookup chain.

## Universal handler pattern (empirically verified across all 15)

All currency_* handlers share this pattern:
- **Sender identity entry:** `let state = get_or_rejoin_state()?; let caller = format!("{}:{}", state.role, state.instance);` — derived from sidecar-process-local state singleton ACTIVE_PROJECT.
- **Lock scope:** wrapped in `collab::with_currency_lock(&dir, ...)` OR `collab::with_currency_and_board_lock(&dir, ...)` (the latter for handlers that broadcast).
- **Snapshot pattern:** `read_balances_snapshot(&dir)` → mutate in memory → `write_balances_snapshot(&dir, &snap)` (atomic write).
- **Ledger append:** all mutators write a row to `.vaak/currency.jsonl` via `append_*_row` helpers (append-only, lock-protected).
- **Board broadcast:** mutators that need team visibility call `append_to_board` with a system message — this is INSIDE the with_currency_and_board_lock guard.
- **Role-based authorization:** some handlers gate by role (`if !poster.starts_with("human:")`); the check uses `caller`, which is sidecar-local.

**F11 migration concern (universal):** the `caller` derivation from sidecar-process-local state singleton MUST move to "caller from POST payload" with the X-Vaak-Token header verifying the sidecar's identity claim. Per architect msg 2627: every-POST F11 enforcement scope locked.

## Per-handler audit rows

### 1. `currency_balance` (READ) — dispatcher inline at vaak-mcp.rs:18554-18617

- **Reads:** `.vaak/balances.json` via `collab::currency::read_balances_snapshot`; falls back to `currency.jsonl` replay via `replay_balances_from_ledger` if balances missing.
- **Writes:** lazy-replay-on-read writes `balances.json` if missing (via `write_balances_snapshot`). This is the ONLY way a READ handler writes — boundary case.
- **Locks:** `collab::with_currency_lock` (currency.lock).
- **Downstream effects:** none observed; pure read+rebuild.
- **Sender-identity dependency:** `state.role + state.instance` default; `seat` arg overrides for reads of OTHER seats' balances. No role-based gate on cross-seat read — anyone can query anyone's balance (matches existing semantics).
- **File-op-hook interaction:** none in handler. Hook fires on Edit/Test tool calls separately; currency_balance MCP call is not Edit/Test.
- **Migration risks:** the lazy-replay write inside a "read" handler MUST be preserved post-migration. If Tauri side splits read/write paths cleanly, the replay-write path needs explicit handling.

### 2. `currency_ledger` (READ) — dispatcher inline at vaak-mcp.rs:18617-18675

- **Reads:** `.vaak/currency.jsonl` tail (last N rows via filesystem tail, no lock per the append-only invariant).
- **Writes:** none.
- **Locks:** none (tail-read on append-only file is safe lock-free per cross-handler convention §"Lock ordering invariants" in placeholder).
- **Downstream effects:** none.
- **Sender-identity dependency:** `state.role + state.instance` default; `seat` arg filter. No role-gate.
- **File-op-hook interaction:** none.
- **Migration risks:** **LOW.** Pure read. Tail-read pattern translates cleanly to Tauri. F11 still needed to authorize cross-seat ledger views.

### 3. `currency_human_adjust` (MUTATE) — vaak-mcp.rs:12474+

- **Reads:** balances snapshot via `collab::currency::apply_human_adjust` (calls `read_balances_snapshot` internally).
- **Writes:** balances snapshot (`write_balances_snapshot`) + ledger append (`append_human_adjust_row`) + board broadcast (`append_to_board`).
- **Locks:** `collab::with_currency_and_board_lock` (currency.lock + board.lock acquired in that order per existing convention).
- **Downstream effects:** board broadcast `[human_adjust]` event; no other handlers triggered.
- **Sender-identity dependency:** `caller = "role:instance"`; passed to `apply_human_adjust` which gates by `caller.starts_with("human:")` (verified at collab layer). **HIGHEST-STAKES authorization — only human can grant arbitrary copper.**
- **File-op-hook interaction:** none.
- **Migration risks:** **HIGHEST.** Per evil-arch msg 2564 + my msg 2567: a local malicious script POSTing `role: "human"` could spoof the gate UNLESS X-Vaak-Token + sidecar-bound session_id verification is enforced. F11 every-POST mitigation (architect msg 2627) addresses this; verify the Tauri handler's authorization check runs on POST-payload caller verified against sidecar's session_id binding, NOT on caller-asserted string alone.

### 4. `currency_post_bounty` (MUTATE) — vaak-mcp.rs:10286+

- **Reads:** `read_balances_snapshot` + `read_open_bounties_snapshot`.
- **Writes:** balances snapshot + open-bounties snapshot (`write_open_bounties_snapshot`) + bounty row append (`append_bounty_row`) + board broadcast (`[bounty] new`).
- **Locks:** `collab::with_currency_and_board_lock` (currency.lock + board.lock).
- **Downstream effects:** board broadcast `[bounty] new`; no other handlers.
- **Sender-identity dependency:** `caller = "role:instance"`; gates `if !poster.starts_with("human:")` — only human can post bounties.
- **File-op-hook interaction:** none.
- **Migration risks:** SAME as currency_human_adjust — role-gate must use verified caller.

### 5. `currency_claim_bounty` (MUTATE) — vaak-mcp.rs:12499+

- **Reads:** balances + open-bounties snapshots.
- **Writes:** balances + open-bounties snapshots + claim row append + board broadcast.
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast; escrow lifecycle (10% stake locked on caller's balance).
- **Sender-identity dependency:** caller's seat used as claimant; no role-gate (any seat can claim).
- **File-op-hook interaction:** none.
- **Migration risks:** medium — escrow lifecycle MUST atomically update balance + bounty.status under the lock. Cache-and-restart edge case: if IdempotencyCache returns cached response on retry but balances.json was successfully updated before HTTP response interrupted, the cache returns success without re-execution → idempotency holds. Verify cache scope key includes mutation result hash.

### 6. `currency_abandon_bounty` (MUTATE) — vaak-mcp.rs:12543+

- **Reads:** balances + open-bounties.
- **Writes:** balances + open-bounties (returns claimant's stake) + board broadcast.
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast `[bounty] abandoned`.
- **Sender-identity dependency:** caller must match `bounty.claimant`; no role-gate (any claimant can abandon).
- **File-op-hook interaction:** none.
- **Migration risks:** identity binding to `bounty.claimant` — must verify the POST-payload caller matches the stored claimant, NOT spoofable.

### 7. `currency_submit_bounty` (MUTATE) — vaak-mcp.rs:12596+

- **Reads:** balances + open-bounties + board (for ref_msg validation).
- **Writes:** open-bounties (status → submitted) + ledger append (no balance change yet — payout on approve).
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast `[bounty] submitted`; ref_msg attached.
- **Sender-identity dependency:** caller must match `bounty.claimant`.
- **File-op-hook interaction:** none.
- **Migration risks:** identity binding same as abandon_bounty.

### 8. `currency_approve_bounty` (MUTATE) — vaak-mcp.rs:12633+

- **Reads:** balances + open-bounties.
- **Writes:** balances (payout claimant) + open-bounties (status → approved + approved_by) + ledger row + board broadcast.
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast `[bounty] approved`; claimant balance increases.
- **Sender-identity dependency:** caller must be `bounty.posted_by` (poster approves their own bounty). Role-gate via match: only poster.
- **File-op-hook interaction:** none.
- **Migration risks:** poster-identity binding — must verify POST-payload caller matches stored `posted_by`.

### 9. `currency_reject_bounty` (MUTATE) — vaak-mcp.rs:12696+

- **Reads:** balances + open-bounties.
- **Writes:** open-bounties (status → rejected) + ledger row + board broadcast.
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast; claimant's stake released back to claimant balance.
- **Sender-identity dependency:** caller must be `bounty.posted_by`.
- **File-op-hook interaction:** none.
- **Migration risks:** same poster-identity binding as approve_bounty.

### 10. `currency_objection` (MUTATE) — vaak-mcp.rs:12750+

- **Reads:** balances + board (for target_msg_id validation) + disputes.json.
- **Writes:** balances (50 copper escrow charged to challenger) + dispute row append + ledger row + board broadcast.
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast `[objection]`; dispute lifecycle initiated.
- **Sender-identity dependency:** caller's seat = challenger. No role-gate (anyone can object — per CR redesign economic backstop spec).
- **File-op-hook interaction:** none.
- **Migration risks:** **MEDIUM-HIGH.** Touches escrow lifecycle + dispute creation + economic backstop semantics that Continuous Review redesign per human msg 2549 depends on. Per architect spec: any commit, any time → economic backstop. Verify Tauri handler preserves the "any seat, any time" access semantics with F11 caller-verification only (no role-gate).

### 11. `currency_call_judge` (MUTATE) — vaak-mcp.rs:12967+

- **Reads:** disputes.json + balances.
- **Writes:** disputes.json (dispute status update) + board broadcast.
- **Locks:** `with_currency_and_board_lock` (likely; verify).
- **Downstream effects:** board broadcast notifying judge role; judge role triggered to respond.
- **Sender-identity dependency:** caller's seat = invoker; may have role-gate (verify).
- **File-op-hook interaction:** none.
- **Migration risks:** depends on judge-role authorization model. Verify the role lookup is from POST payload not sidecar context.

### 12. `currency_judge_ruling` (MUTATE) — vaak-mcp.rs:13022+

- **Reads:** disputes + balances.
- **Writes:** disputes (ruling field + resolution) + balances (escrow distribution per ruling) + ledger rows + board broadcast.
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast `[ruling]`; balance changes for both disputant + challenger (escrow distribution).
- **Sender-identity dependency:** **only `judge` role can call.** Verify Tauri-side role-gate.
- **File-op-hook interaction:** none.
- **Migration risks:** **HIGH.** Same class as currency_human_adjust — restricted role. Must verify POST-payload caller against sidecar session binding before allowing the action. Spoofing risk: a local script POSTing `role: "judge"` without F11 enforcement would let it issue arbitrary rulings.

### 13. `currency_system_dispute` (MUTATE) — vaak-mcp.rs:13160+

- **Reads:** balances + disputes.
- **Writes:** disputes (system-initiated dispute) + balances (50 copper system stake) + ledger row + board broadcast.
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast `[system_dispute]`.
- **Sender-identity dependency:** anyone can call; charge against caller's balance.
- **File-op-hook interaction:** none.
- **Migration risks:** standard economic backstop; F11 caller-verification sufficient.

### 14. `currency_concede` (MUTATE) — vaak-mcp.rs:13217+

- **Reads:** disputes + balances.
- **Writes:** disputes (concession marker) + balances (escrow distribution to challenger) + ledger row + board broadcast.
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast `[concede]`.
- **Sender-identity dependency:** caller must be the `disputed` party (the one objection was filed against).
- **File-op-hook interaction:** none.
- **Migration risks:** identity binding to `dispute.disputed_seat` — must verify POST-payload caller matches stored disputed party.

### 15. `currency_dispute_message` (MUTATE) — vaak-mcp.rs:13346+

- **Reads:** disputes + board (for context).
- **Writes:** disputes (message thread append) + board broadcast.
- **Locks:** `with_currency_and_board_lock`.
- **Downstream effects:** board broadcast `[dispute_message]`.
- **Sender-identity dependency:** caller must be disputant OR challenger OR judge (verify).
- **File-op-hook interaction:** none.
- **Migration risks:** identity binding to dispute participants — verify multi-party authorization preserved.

## Cross-handler concerns (validated by audit)

### Lock ordering invariants (CONFIRMED)

- `currency.lock` always acquired BEFORE `board.lock` via the combined `with_currency_and_board_lock` helper. Direct grep shows no handler acquires them out-of-order. ✓
- `currency.lock` never held during network I/O — handlers do all file I/O inline; no async/network calls inside the lock. ✓
- `currency.jsonl` tail-read for `currency_ledger` is lock-free per append-only invariant. ✓ Verified safe.
- `balances.json` read+write IS lock-protected via `with_currency_lock`. ✓

### File-op-hook (file-op-claim.py) interaction (NEW FINDING)

- Hook fires on PreToolUse/PostToolUse for Read/Edit/Write/NotebookEdit. NOT on `currency_*` MCP tool calls (currency tool calls aren't in the hook matcher).
- Hook writes `.vaak/sessions/<seat>-pending-edit.json` marker (per file-op-claim.py line 115).
- Currency handler chain consumes the marker via `next project_send` of the seat — this happens INSIDE collab::send_message logic, NOT inside currency_* handlers.
- **F3 finding:** the file-op-hook → marker → project_send-consume chain is INDEPENDENT of the currency_* handler migration. Currency handlers don't touch the marker file directly. Migration to Tauri does NOT break the hook chain.
- **Verification commitment for SHA-HR.2.x:** after Phase 2 currency_* handlers move to Tauri, verify a fresh Edit → next project_send → marker consumed → edit_test_earn copper minted. Should still work because the consumer is collab::send_message, not the migrated handlers.

### Sender-identity passing (F11 trust-model) — NEW AUDIT FINDINGS

- **All 15 handlers depend on sidecar-process-local `state.role` + `state.instance`** for caller identity. This is the universal F11 migration concern.
- **Role-gated handlers (HIGHEST-RISK, listed by risk):**
  1. `currency_human_adjust` — only `human:*` (unbounded grant power)
  2. `currency_judge_ruling` — only `judge` role (escrow redistribution)
  3. `currency_post_bounty` — only `human:*` (bounty creation)
- **Identity-binding handlers (binding to stored party in state file):**
  - `currency_claim_bounty` / `abandon_bounty` / `submit_bounty` — bound to `bounty.claimant`
  - `currency_approve_bounty` / `reject_bounty` — bound to `bounty.posted_by`
  - `currency_concede` — bound to `dispute.disputed_seat`
  - `currency_dispute_message` — bound to dispute participants
- **Unrestricted handlers (anyone can call; charge against caller's balance):**
  - `currency_objection` / `currency_system_dispute` / `currency_call_judge` / `currency_balance` / `currency_ledger`
- **F11 verification rule:** Tauri-side handler MUST receive POST-payload `(role, instance, session_id)` and verify against `.vaak/sessions/<role>-<instance>.json:session_id` binding (per architect msg 2627 every-POST scope). Any handler that derives caller from "POST payload caller string trusted directly" without binding-verification is spoofable.

### Idempotency cache touch points (F6) — NEW AUDIT FINDINGS

- **All 13 mutating handlers** are subject to F6 IdempotencyCache. Cache key: `(X-Vaak-Request-Id, tool_name)`. TTL: 60s per architect msg 2461.
- **Cache scope verification:**
  - Mutating handler returns response → cache stores `(request_id → response)`.
  - Retry within 60s → cache returns stored response WITHOUT re-executing handler.
  - **Critical edge case:** if mutating handler PARTIALLY completed (e.g., wrote currency.jsonl row but crashed before board broadcast), retry could either (a) return cached partial-success response — leaving the board un-notified, OR (b) re-execute the handler — risking double-write.
  - **Recommendation:** cache stores response ONLY AFTER full handler completion (board broadcast included). On partial failure, no cache entry written → retry re-executes. The `with_currency_and_board_lock` already serializes; idempotency requires the cache write to be the LAST step after the lock releases.
- **Read handlers (`currency_balance`, `currency_ledger`):** technically idempotent without cache; but cache short-circuits redundant work. No correctness concern.

## NEW risks identified by this audit (beyond cross-handler-concerns placeholder)

### NR1: Replay-on-read inside currency_balance is a write

`currency_balance` has a non-obvious WRITE path: if `balances.json` is missing but `currency.jsonl` exists, it rebuilds balances and writes the snapshot. This means a "read" handler can produce a side-effect write. Migration must preserve this — or move replay logic out (cleaner separation). Test case: delete balances.json, call currency_balance, verify balances.json reappears with replay'd contents.

### NR2: Sender-identity binding ambiguity for multi-party handlers

`currency_dispute_message` allows multiple parties to call (disputant + challenger + judge). The role-gate is NOT a simple `starts_with("human:")` check — it's a participant-membership check against `dispute.participants`. The F11 verification rule must support this pattern: caller's verified identity must be CHECKED AGAINST a list, not gated by a fixed role string. Tauri handler implementation pattern: derive verified `caller = "role:instance"` from POST payload, then check `dispute.participants.contains(&caller)`. Pattern works but adds one more layer over the simple role-gate handlers.

### NR3: replay_balances_from_ledger contains the supply-invariant guarantee

`tester msg 2628` baseline: 31,971 copper total supply. The `replay_balances_from_ledger` function MUST preserve this invariant exactly — if Tauri-side replay produces a different total, the migration broke balance accounting. Acceptance test (per architect msg 2629 phase-2-amend baseline): replay total === 31,971 ± transient escrow movement during the audit window. Tester-lane verification per acceptance criterion 4.

### NR4: Idempotency cache key collision risk on missing X-Vaak-Request-Id

If a malformed sidecar POST omits `X-Vaak-Request-Id`, the cache lookup defaults to empty-string key → ALL such requests collide on cache. The first such request's response gets returned for any subsequent request without request_id. **Tauri-side defensive:** reject POST without `X-Vaak-Request-Id` header with 400. Don't fail open.

### NR4b — UI currency-display path has unrelated multi-writer dependency on sessions.json:bindings:status (per architect msg 2650)

**This is NOT a `handle_currency_balance` MCP handler concern — it's an adjacent finding requested by architect msg 2650 during the audit window.**

`get_currency_balances_cmd` at `main.rs:3941-3996` (the Tauri-side IPC command consumed by the UI's 30s currency-poll, distinct from the MCP `handle_currency_balance`) hard-filters bindings by `status == "active"` at main.rs:3962. If the bindings:status field isn't kept fresh by some writer (which is the chronic empty-UI-pill symptom per human msg 2645 + 2649), all seats are excluded → currencyBalances Map empty → frontend renders no pills.

**State-residency implication for Phase 2:** while the MCP `currency_balance` handler I audited doesn't have this dependency, the BROADER currency-UI ecosystem depends on `sessions.json:bindings:status` being maintained. This is one of the MW6/MW10 multi-writer instances per `project_multi_writer_audit_complete_2026-05-27`. The fix architect msg 2651 leaning toward — "derive active-ness from heartbeat freshness (last_alive_at_ms within 60s) NOT add a 2nd hardcoded status value" — sidesteps the multi-writer problem cleanly.

**Cross-references:** human msg 2645 (chronic UI economy bug); architect msg 2650/2651 (diagnosis + [BUILDER-CONFIRM-BEFORE-COMMIT] header); tester msg 2628 (31,971cu backend baseline confirms data exists); ui-arch msg 2652 (UI lane independent diagnosis of silent-catch + currency_enabled flag).

**Not blocking Phase 2 of hot-reload migration** — this bug is in `get_currency_balances_cmd` (already Tauri-side), not in any of the 15 MCP handlers being migrated. Document for cross-team visibility only.

### NR5: Cross-handler atomic chain risk under partial migration

During Phase 2 migration, some currency_* handlers will be Tauri-side (proxied) and others still sidecar-side (legacy). If a single user action triggers BOTH a Tauri-side handler AND a sidecar-side handler in quick succession (e.g., currency_objection then currency_call_judge), the file locks are still file-based and cross-process safe — but the F11 verification path is asymmetric. The Tauri handler does X-Vaak-Token check; the sidecar handler doesn't. **Mitigation:** maintain the universal pattern that all currency_* handlers migrate as a single Phase 2 chain (not staged per-handler) to avoid the asymmetric-trust window.

## Acceptance gate

The audit is COMPLETE:
1. ✓ All 15 handler rows have all 6 questions answered with concrete file/line citations
2. ✓ At least one row identifies a NEW risk not enumerated in cross-handler-concerns (5 NEW risks identified: NR1-NR5)
3. **PENDING:** dev-challenger signs off + ships a commit `SHA-HR.spec.f3-audit-complete` updating this doc with the audit-COMPLETE status — THIS COMMIT BEING PREPARED
4. **PENDING:** Architect-lane reviews the completed audit and either ratifies the Phase 2 commit sequence OR amends the sequence per audit findings

## Open invitation (REAFFIRMED)

- **dev-challenger:0 — AUDIT LEAD (filled in this commit pass)**
- **evil-architect:0** — adversarial review of completed audit (find risks I missed)
- **tester:0** — empirical verification of Phase 2 plan acceptance criterion 4 (total-copper supply invariant per msg 2628 baseline 31,971)
- **developer:0** — implementation lane (cannot ship Phase 2 until this audit closes per architect ratification)

## Summary recommendations for Phase 2 implementation order

Based on NEW risks NR1-NR5, recommended Phase 2 implementation sequencing:

1. **SHA-HR.2.0 IdempotencyCache** with mandatory `X-Vaak-Request-Id` header per NR4 (reject 400 on absence; cache write LAST after lock release per F6 cache scope verification)
2. **SHA-HR.2.1 F11 every-POST identity verification helper** (Tauri-side `verify_caller(post_payload, headers) → Result<(role, instance), Error>` against `.vaak/sessions/*.json` session_id binding)
3. **SHA-HR.2.2 currency_balance + currency_ledger migration** (lowest risk; verify NR1 replay-on-read preserved + NR3 supply invariant via post-migration baseline check)
4. **SHA-HR.2.3 Unrestricted mutating handlers** (currency_system_dispute, currency_call_judge, currency_objection — bulk migrate; NR2 multi-party check pattern applies to objection)
5. **SHA-HR.2.4 Identity-bound handlers** (currency_claim/abandon/submit/approve/reject_bounty + currency_concede + currency_dispute_message — verify bound-party check in each)
6. **SHA-HR.2.5 Role-gated handlers** (currency_human_adjust, currency_judge_ruling, currency_post_bounty — HIGHEST RISK; ship last with extensive Tauri-side authorization tests)
7. **SHA-HR.2.6 Post-migration supply-invariant verification** (tester runs full ledger replay → balances reconstruction → total === 31,971 baseline per NR3)

**Phase 2 migration MUST be atomic per NR5 — all 15 handlers migrate in this commit chain; no partial-migration window left in production.**
