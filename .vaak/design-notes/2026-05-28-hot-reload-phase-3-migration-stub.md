# Hot-reload Phase 3 — oxford / delphi / assembly / discussion / audience migration (stub)

**Owner:** architect:0
**Date:** 2026-05-28
**Status:** Stub — full plan deferred until Phase 2 acceptance + lessons-learned
**Parent spec:** `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md`
**Predecessor:** `.vaak/design-notes/2026-05-28-hot-reload-phase-2-currency-migration-plan.md`

---

## Why a stub now?

Phase 3 cannot be fully planned until Phase 2 lessons are in hand (idempotency cache pressure, identity-verification middleware corner cases, lock-ordering surprises). But the boundaries can be locked now so Phase 2 implementation is not forced into Phase-3-shaped decisions ad-hoc.

This stub locks: scope, module structure, sequencing constraints. Per-handler audit + commit sequence comes after Phase 2 acceptance.

## Scope (~3500 LOC estimated)

5 tool families:

| Family | Tools | Estimated LOC | Module |
|---|---|---|---|
| Oxford | oxford_initiate, oxford_advance_phase, oxford_declare_speaker, oxford_yield, oxford_react, oxford_kick, oxford_audience_question, oxford_audience_vote, oxford_end | ~1200 | `mcp_handlers/oxford.rs` |
| Delphi | delphi_initiate, delphi_open_round, delphi_submit, delphi_close_round, delphi_end, delphi_get_state | ~1100 | `mcp_handlers/delphi.rs` |
| Assembly | (assembly_line migrated in Phase 1; remaining: `protocol_mutate`, `get_protocol`) | ~300 | `mcp_handlers/protocol.rs` |
| Discussion / Section | discussion_control, create_section, list_sections, switch_section | ~500 | `mcp_handlers/section.rs` |
| Audience | audience_vote, audience_history | ~400 | `mcp_handlers/audience.rs` |

## Sequencing constraints

1. **Assembly Line (Phase 1) must hold its hot-reload canary through ALL of Phase 3** — regression risk if module restructuring breaks `assembly_line`. Phase 1 acceptance test re-runs at each Phase 3 commit.

2. **Oxford and Delphi share opportunistic-tick patterns** — the Tauri-side wall-clock backstop (per arch msg 2627 ruling) for `auto_close_timed_out_round` MUST be live before Phase 3 ships, because Phase 3 migration removes the sidecar's `delphi_sweeper_maybe_close` opportunistic call paths. Cleanup commit: SHA-HR.3.0 — verify the wall-clock backstop calls Delphi sweeper too (not just CR).

3. **`protocol_mutate` is the riskiest tool in Phase 3** — touches assembly_line state. Phase 3 migration of it must maintain the Option (a) `serde_json::Value` round-trip pattern established in Phase 1's `do_protocol_mutate_inner` set_preset arm (SHA-HR.1.3). Likely the LAST commit in Phase 3.

4. **Sub-phase 3a: Read-only tools first** (list_sections, audience_history, delphi_get_state, oxford_react, get_protocol). Cheap, low-risk, builds confidence in the F3 pattern from Phase 2.

5. **Sub-phase 3b: Mutating tools.** Per-family commits. Each family ships as one commit (shared module-internal helpers).

6. **Sub-phase 3c: `protocol_mutate` + sidecar cleanup.** Final.

## Inherited mandatory infrastructure from Phase 2

- F9 token-file ACL — already shipped (`ec84b58`)
- F11 verify_caller_identity middleware — required, MUST be live from SHA-HR.2.0
- F6 IdempotencyCache — required, MUST be live from SHA-HR.2.0
- [BUILDER-CONFIRM-BEFORE-COMMIT] header pattern — required for all time-critical sequencing rulings during Phase 3

## Phase 3 dependencies surfaced for early audit (architect-lane prefill — dev-challenger to validate during Phase 3 F3 audit)

- **`oxford_initiate`** + **`delphi_initiate`** both write `.vaak/active-section` + their respective state files. Cross-tool atomicity concern: cannot have both an Oxford debate AND a Delphi round active in the same section. Verify lock-ordering preserves this.
- **`oxford_advance_phase`** + **`delphi_close_round`** both trigger broadcasts via internal project_send equivalent. Phase 3 migration MUST preserve these broadcasts; the side-effect chain (write state → broadcast) is what drives downstream UI.
- **`audience_vote`** depends on `claims.json` (who can vote). Cross-tool: `project_claim` / `project_release` / `project_claims` (in Phase 4 scope but touching same state). Phase 3 audit must enumerate this cross-phase coupling.
- **`discussion_control(set, mode)`** — deprecated per Continuous Review redesign (msg 2549). Migration may simplify the handler significantly OR remove it entirely. Architect-lane ruling deferred: keep handler for backwards compatibility but mark deprecated; can be deleted after one full session of zero usage.

## Acceptance criteria (placeholder — finalized post-Phase-2-acceptance)

1. All Phase 3 tools respond from Tauri (`_hot_reload_phase: 3` sentinel)
2. Phase 1 + Phase 2 regression tests continue passing
3. Oxford debate end-to-end + Delphi round end-to-end run identical to pre-Phase-3 semantics (regression test)
4. `protocol_mutate` set_preset + other arms continue working
5. F9 + F11 + F6 middleware applies to all Phase 3 endpoints (not just currency_*)
6. Tauri-side wall-clock backstop calls `delphi_sweeper_maybe_close` (not just `auto_close_timed_out_round`)

## Phase 3.5 (separate chain, not Phase 3)

Per hot-reload spec §"Phase 3.5 — tiny_http → thread-pool / async upgrade." Concurrency architecture upgrade extracted from Phase 4. Acceptance: load test 100 concurrent /heartbeat POSTs without degradation. Must land before Phase 4 (project_send long-poll).

## Phase 4 dependency callout

Phase 4 (`project_send`, `project_check`, `project_wait`, `project_status`, ~16 tools, ~6000 LOC) depends on Phase 3.5 landing first. Phase 4 is NOT in this stub's scope.

## Backlog flagged from Phase 2

- Kick-reclamation flow (per tester msg 2628) — Phase 3+ candidate; affects total-supply invariant
- File-op-hook (`file-op-claim.py`) Tauri-side analog — Phase 3 or Phase 4 candidate; currently lives in sidecar lifecycle

## Next step

This stub is the WHAT/WHY at architecture level. The HOW (per-commit sequence + per-handler F3 audit) waits for Phase 2 acceptance. Phase 2 implementation lessons may invalidate or refine sequencing constraints above; do NOT treat this stub as locked beyond the scope + module structure.
