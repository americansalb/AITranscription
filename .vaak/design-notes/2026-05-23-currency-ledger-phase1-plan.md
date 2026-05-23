# Plan: Currency Ledger — Phase 1

Section: 5-22. Owners: architect:0 (this plan + spec), developer:1 (commit a), developer:0 (commit b), TBD (commit c), dev-challenger:0 (adversarial review), ui-architect:0/1/2 (data-shape review only — no UI surface in Phase 1).

<!-- scope: desktop/src-tauri/src/bin/vaak-mcp.rs desktop/src-tauri/src/collab.rs desktop/src-tauri/Cargo.toml .vaak/project.json .vaak/design-notes/2026-05-23-currency-ledger-phase1-spec.md .vaak/design-notes/2026-05-23-currency-ledger-phase1-plan.md -->

## Trigger

Human directive msg 1060 (2026-05-23): integrate an economic layer. Phase 1 = ledger + balance math + escrow lifecycle + deficit-cap gate. Disputes (Phase 2), Edit/Test wiring (Phase 3), retroactive Pass penalties (Phase 4) are out of scope and explicitly forbidden in this plan.

Pre-spec adversarial review: dev-challenger:0 msg 1063, developer:0 msg 1069, developer:1 msg 1065, ui-architect:0 msg 1071, ui-architect:1 msg 1067, ui-architect:2 msg 1073. Rulings consolidated in architect:0 msg 1075. Full design at `.vaak/design-notes/2026-05-23-currency-ledger-phase1-spec.md`.

## Why a new plan (and not the AL-roster plan)

developer:0 msg 1069 flagged the audit-trail concern: the active `2026-05-22-al-roster-merge-and-rotation-fix-v1-plan.md` scope includes vaak-mcp.rs + collab.rs, so the `planning_blocks_commit` hook would technically PASS a currency commit. But currency has nothing to do with the AL-roster fix's intent. Committing under the AL-roster plan corrupts the audit trail. This plan separates the scope cleanly.

## Commits (three, sequential, each independently revert-safe)

### Commit (a) — Shadow build

Owner: developer:1. ETA ~60-90 min. ~300-400 LOC.

Files: `desktop/src-tauri/src/collab.rs`, `desktop/src-tauri/src/bin/vaak-mcp.rs`.

Surface:
- Constants per spec §"Constants"
- Types: `TransactionType`, `LedgerRow`, `SeatBalance`, `EscrowItem`, `BalancesSnapshot`, `ActionKind`, `CopperDisplay`
- Helpers in collab.rs: `append_currency_transaction`, `read_balances_snapshot`, `write_balances_snapshot` (via atomic_write), `copper_to_display`, `next_escrow_id`, `replay_balances_from_ledger`
- Startup replay in vaak-mcp.rs `main` — rebuilds balances.json from currency.jsonl with skip-last-line-on-parse-fail per spec §"Replay rules"
- MCP tool registrations: `currency_balance`, `currency_ledger`, `currency_objection` (stub returning `Not implemented yet. Phase 2.`)
- NO project_send hooks wired
- NO tick processing wired

Acceptance:
- `currency_balance` returns 10000 for a fresh test seat after a single lazy-init transaction
- `currency_ledger { limit: 5 }` returns at most 5 transactions, newest first
- `currency_ledger { seat: "X" }` filters correctly
- `currency_objection` returns the locked Phase 2 error
- Restart preserves balances (snapshot persists; replay reconstructs same state)
- Truncated last line in currency.jsonl produces a startup WARN, not a panic
- Truncated NON-last line is a HARD ERROR

Pre-commit gates (NOT optional per developer:1 msg 1008 commitment):
- LOC ~150 ping to dev-challenger:0 + developer:0 for review
- `cargo build --release` from `desktop/src-tauri/` passes clean
- Manual smoke test via stdin JSON-RPC to vaak-mcp.exe (the existing test pattern)

### Commit (b) — project_send hooks + classifier + adversarial tags

Owner: developer:0. ETA ~45-60 min. ~150-250 LOC.

Files: `desktop/src-tauri/src/bin/vaak-mcp.rs`, `.vaak/project.json`.

Surface:
- TimedOut pre-hook in `handle_project_send` — checks `balances.seats[from].timed_out` BEFORE board append, returns `[TimedOut]` error if true
- Post-board-append hook (inside the same `with_board_lock` scope):
  - Classify via `classify_action` (per spec)
  - Skip if `ActionKind::Exempt` (human)
  - Append earn transaction (`type: "credit"`, amount per spec)
  - Append escrow_hold transaction (`type: "escrow_hold"`, negative amount = funds held)
  - Update balances.json via atomic_write
- project.json: add `adversarial: true` to `evil-architect` and `dev-challenger` role configs
- Tick processing still deferred to commit (c)

Acceptance:
- Pass message → +1 copper credit + 1 copper held in escrow (balance +0 net, escrow_held +1)
- Speak message → +10 copper credit + 10 copper held in escrow (balance +0 net, escrow_held +10)
- Human message → no currency transaction emitted (exempt)
- Moderator message → currency transaction emitted (NOT exempt per directive)
- Send while `timed_out: true` → returns `[TimedOut]` BEFORE board append
- adversarial:true present on evil-architect and dev-challenger in project.json

Pre-commit gates:
- LOC ~75 ping to dev-challenger:0 + architect:0
- `cargo build --release` passes
- Integration test: send a sequence of messages, verify ledger via `currency_ledger`

### Commit (c) — Tick processing

Owner: TBD (assigned after b lands; likely developer:0 or developer:1 depending on availability). ETA ~45-60 min. ~200-300 LOC.

Files: `desktop/src-tauri/src/bin/vaak-mcp.rs`.

Surface:
- turn_counter increments inside `with_board_lock` on every successful non-human send and on every `al_auto_advance` mic_advance
- Escrow release: iterate `escrow_items`, move matured (`release_turn <= turn_counter`) items to settled balance, append `escrow_release` transaction
- Interest: for each escrow item with `amount >= INTEREST_MIN_HELD`, credit `amount / 10` copper to settled balance, append `interest` transaction
- Passive income: on mic_advance only, +1 copper to every active seat, append `passive` transaction
- All processing inside the same `with_board_lock` scope

Acceptance:
- Held Speak escrow releases after 5 ticks (PASS_ESCROW_TICKS / SPEAK_ESCROW_TICKS per spec)
- 50-copper escrow accrues 5 copper interest per tick (50/10 = 5)
- 5-copper escrow accrues NO interest (below INTEREST_MIN_HELD)
- Passive income reaches all 7+ active seats per mic_advance
- Off-AL session: escrow releases + interest accrue per project_send; passive income does NOT fire
- On-AL session: both per project_send AND per mic_advance

Pre-commit gates:
- LOC ~100 ping to dev-challenger:0 + architect:0
- `cargo build --release` passes
- End-to-end test: simulate 20-tick run, verify ledger matches expected balances

## Activation gate (per project_rebake_requires_claude_code_window_relaunch)

After commits (a) + (b) + (c) all land:

1. `cargo build --release` from `desktop/src-tauri/`
2. `npm run build-sidecar` (rebakes vaak-mcp.exe into the sidecar bundle per project_rebuild_command_includes_sidecar)
3. Close ALL running Claude Code windows (sidecar caches session_id per-PPID)
4. Restart Vaak
5. Reopen Claude Code windows
6. Each seat lazy-inits on first send

The human will see no UI change in Phase 1 — verification is via `currency_balance` MCP tool from any Claude Code window.

## Anti-scope (explicit forbiddens this plan)

- Touching ANY frontend file (directive msg 1060 explicit)
- Implementing Objection logic beyond the stub return
- Implementing Edit/Test detection or PostToolUse hooks
- Implementing retroactive Pass-negligence assessment
- Adding any new currency-related MCP tools beyond the 3 specified
- Schema changes to board.jsonl, sessions/, protocol.json, claims.json

## Multi-role lock + collision avoidance

Single-builder lanes per commit (architect:0 msg 1075 rule). Builders MUST broadcast at LOC-N ping AND explicitly claim the build before editing. Watchdog burned 6+ mic cycles on collision today — don't repeat.

If a builder stalls (watchdog max_floor_exceeded mid-build), the next assigned builder verifies disk state via `git status` + `git log --oneline -1` (NOT just `git diff`) before assuming clean slate. The P5-v2 false-clean-tree mistake (developer:0 msg 962 + ui-architect:1 msg 960) is the cautionary tale.

## Adversarial review acceptance

Spec ratification: dev-challenger:0 + ui-architect:0 + developer:0 must each post explicit `APPROVED` or `BLOCK: <reason>` on this plan + spec before commit (a) begins. ui-architect:1 + ui-architect:2 may concur or no-op per their stated standing-down from backend work.

Human approval gate: architect rulings v2 (msg 1075) are locked unless human responds with explicit corrections. Plan + spec posted for review; absence of human objection within 30 min of architect broadcasting plan-ready = implicit acceptance and commit (a) may proceed under the standard reviewer gates.
