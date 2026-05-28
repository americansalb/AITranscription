# Hot-reload Phase 2 — F3 state-residency audit (per-handler)

**Owner (audit lead):** dev-challenger:0 (per spec invitation, msg 2434 + reaffirmed in `2026-05-28-hot-reload-phase-2-currency-migration-plan.md`)
**Owner (placeholder author):** architect:0
**Date:** 2026-05-28
**Status:** Open — rows below are skeletons. dev-challenger:0 fills in BEFORE Phase 2 implementation begins.
**Parent spec:** `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md` §F3
**Parent plan:** `.vaak/design-notes/2026-05-28-hot-reload-phase-2-currency-migration-plan.md`

---

## Audit purpose

Each of the 15 `currency_*` handlers being migrated to Tauri must have its state interactions enumerated BEFORE the migration commit. Goal: discover lock ordering, hook-chain dependencies, file-locking concerns, and sender-identity assumptions that could break post-migration. The audit is a structural defect-finder — fill in the rows below and architect-lane will review for risks BEFORE the implementing commit chain starts.

## Six questions per handler (from Phase 2 plan §F3)

1. **What state does this handler read?** (files, locks held during read)
2. **What state does this handler write?** (files, atomic-vs-multi-step)
3. **What locks does it acquire?** (and in what order — deadlock-avoidance critical)
4. **What downstream effects?** (opportunistic sweeper calls, hooks triggered, internal project_send calls)
5. **Sender-identity dependency?** Currency gating is sender-side. The Tauri handler MUST receive `(role, instance, session_id)` from the sidecar proxy POST — NOT derive from process context. Verify the migration preserves this.
6. **File-op-hook interaction?** Per `project_currency_edit_test_earns_dead` memory: file-op-claim.py hook lookup chain. If the handler is in edit/test earn flows, verify the hook still reaches the right handler post-migration.

## Per-handler audit rows

### 1. `currency_balance` (READ)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 2. `currency_ledger` (READ)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 3. `currency_human_adjust` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in — known: only human:0 can call; verify Tauri-side enforcement]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in — unbounded grant power; spoofing risk highest]_

### 4. `currency_post_bounty` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 5. `currency_claim_bounty` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 6. `currency_abandon_bounty` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 7. `currency_submit_bounty` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 8. `currency_approve_bounty` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 9. `currency_reject_bounty` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 10. `currency_objection` (MUTATE)

- **Reads:** _[fill in — likely balances.json, claims.json, currency.jsonl tail]_
- **Writes:** _[fill in — likely currency.jsonl append + balances.json update + dispute creation]_
- **Locks:** _[fill in — likely currency.lock]_
- **Downstream effects:** _[fill in — creates dispute object; triggers Continuous Review backstop semantics]_
- **Sender-identity dependency:** _[fill in — known: anyone can call; verify (role, instance) passed correctly]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in — most cross-cutting; touches escrow logic]_

### 11. `currency_call_judge` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 12. `currency_judge_ruling` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in — only judge role can call; verify Tauri-side enforcement]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 13. `currency_system_dispute` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 14. `currency_concede` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

### 15. `currency_dispute_message` (MUTATE)

- **Reads:** _[fill in]_
- **Writes:** _[fill in]_
- **Locks:** _[fill in]_
- **Downstream effects:** _[fill in]_
- **Sender-identity dependency:** _[fill in]_
- **File-op-hook interaction:** _[fill in]_
- **Migration risks:** _[fill in]_

## Cross-handler concerns (architect-lane prefilled — dev-challenger to validate/extend)

### Lock ordering invariants

- `currency.lock` always acquired BEFORE `board.lock` if both needed (per existing convention — verify with grep)
- `currency.lock` never held during network I/O (per existing convention)
- Reading currency.jsonl tail does NOT require currency.lock (append-only file; tail-read is safe lock-free); writing currency.jsonl append DOES require currency.lock
- balances.json read+write IS protected by currency.lock (snapshot, not append-only)

### File-op-hook (file-op-claim.py) interaction

- Hook runs in sidecar's lifecycle on Edit/Test tool use
- Hook constructs a marker file (per `project_currency_edit_test_earns_dead`)
- Handler reaching via project_send proxy honors the marker → mints copper
- **Migration concern:** if currency_balance or currency_ledger move to Tauri, does the marker-file consumer (likely in a different handler chain) still work? Validate with empirical test in commit SHA-HR.2.1 / SHA-HR.2.2.

### Sender-identity passing (F11 trust-model)

- POST body includes `(role, instance, session_id)`
- Tauri verifies (role, instance) against PPID-bound session_id from `.vaak/sessions/*.json` (per Phase 2 plan §F11 mitigation)
- 403 if mismatch
- **All 13 mutating handlers** are subject to this check; 2 read handlers (balance, ledger) are too because read access to OTHER seats' balances may need authorization

### Idempotency cache touch points

- IdempotencyCache (per F6) interposes BEFORE handler invocation
- Cache hit → return cached response without re-executing
- Cache miss → execute handler → store response in cache before returning
- **Verification per migration commit:** scripted retry with same X-Vaak-Request-Id returns cached response, distinct request_id re-executes

## Acceptance gate for this audit doc

The audit is COMPLETE when:
1. All 15 handler rows have all 6 questions answered with concrete file/line citations
2. At least one row identifies a NEW risk not enumerated in cross-handler-concerns
3. dev-challenger signs off + ships a commit `SHA-HR.spec.f3-audit-complete` updating this doc with the audit-COMPLETE status
4. Architect-lane reviews the completed audit and either ratifies the Phase 2 commit sequence OR amends the sequence per audit findings

Until then, Phase 2 commit SHA-HR.2.0 (IdempotencyCache) does NOT ship. **F3 audit gates Phase 2 start.**

## Open invitation

- **dev-challenger:0** — primary lead per spec
- **evil-architect:0** — adversarial review of completed audit (find missing risks)
- **tester:0** — empirical verification of Phase 2 plan acceptance criterion 4 (total-copper supply invariant)
- **developer:0** — implementation lane (cannot ship Phase 2 until this audit closes)
