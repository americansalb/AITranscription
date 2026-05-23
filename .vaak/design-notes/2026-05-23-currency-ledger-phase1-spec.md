# Spec: Currency Ledger — Phase 1

Section: 5-22. Owners: architect:0 (this spec), developer:1 (impl steps a+b), developer:0 (impl step c), dev-challenger:0 + ui-architect:0/1/2 (adversarial + data-shape review).

## Trigger

Human directive msg 1060 (2026-05-23): integrate an economic layer into Vaak so every agent action costs or earns copper. Phase 1 ships the ledger, balance math, escrow lifecycle, and deficit-cap gate. Disputes (Phase 2), Edit/Test detection (Phase 3), retroactive Pass penalties (Phase 4) are out of scope.

Pre-build adversarial review by dev-challenger:0 msg 1063, developer:0 msg 1069, developer:1 msg 1065, ui-architect:0 msg 1071, ui-architect:1 msg 1067, ui-architect:2 msg 1073 produced the rulings codified below. Architect rulings v2 broadcast at msg 1075.

## Constants

```rust
pub const COPPER_PER_SILVER: i64 = 100;
pub const COPPER_PER_GOLD: i64 = 10_000;
pub const STARTING_BALANCE_COPPER: i64 = 10_000;          // 1 gold on join
pub const DEFICIT_CAP_COPPER: i64 = -1_000;               // -10 silver, timeout threshold
pub const PASS_EARN_COPPER: i64 = 1;
pub const SPEAK_EARN_COPPER: i64 = 10;
pub const EDIT_EARN_COPPER_BASE: i64 = 25;                // Phase 3, reserved
pub const TEST_EARN_COPPER_BASE: i64 = 15;                // Phase 3, reserved
pub const PASS_ESCROW_TICKS: u64 = 3;
pub const SPEAK_ESCROW_TICKS: u64 = 5;
pub const EDIT_ESCROW_TICKS: u64 = 10;                    // Phase 3, reserved
pub const TEST_ESCROW_TICKS: u64 = 15;                    // Phase 3, reserved
pub const INTEREST_PER_10_COPPER_HELD: i64 = 1;
pub const INTEREST_MIN_HELD: i64 = 10;                    // below this no interest
pub const OBJECTION_COST_COPPER: i64 = 50;                // Phase 2, reserved
pub const PASS_NEGLIGENCE_PENALTY_COPPER: i64 = 10;       // Phase 4, reserved
```

## File layout

### `.vaak/currency.jsonl`

Append-only ledger. One JSON object per line. Source of truth — balances.json is rebuilt from this on startup.

Row schema (all fields required unless marked):

```json
{
  "id": 1,
  "type": "init" | "credit" | "escrow_hold" | "escrow_release" | "passive" | "interest" | "penalty" | "clawback",
  "seat": "architect:0",
  "amount": 10000,
  "reason": "init on project_join",
  "balance_after": 10000,
  "escrow_id": "esc_001",     // optional, present for escrow_hold/release/interest/clawback
  "ref_msg": 1234,            // optional, board.jsonl message id this transaction references
  "turn_at": 42,              // turn_counter snapshot at write time
  "at": "2026-05-23T05:55:04Z"
}
```

Constraints:

- `id` monotonic, gap-free per file. Replay validates `id == prev_id + 1`.
- `amount` signed; positive = credit to seat, negative = debit from seat. `balance_after` is the post-write balance.
- `reason` is human-readable prose, NOT an opcode (ui-architect:0 msg 1071 ruling 5). Examples: `"escrow release: speak @msg 1042"`, `"passive rotation tick"`, `"timeout at deficit cap"`. Future ledger UIs render rows without joining board.jsonl (which may be retention-pruned).
- `escrow_id` format `esc_<6 hex>`, generated via `format!("esc_{:06x}", n)` where `n` is a per-process counter persisted via balances.json `next_escrow_id`.

### `.vaak/balances.json`

Snapshot via `atomic_write`. Rebuilt from currency.jsonl on startup. Single project-wide file.

```json
{
  "schema_version": 1,
  "turn_counter": 42,
  "next_escrow_id": 17,
  "next_transaction_id": 89,
  "seats": {
    "architect:0": {
      "balance": 9985,
      "escrow_held": 15,
      "escrow_items": [
        {
          "id": "esc_00f",
          "amount": 10,
          "release_turn": 47,
          "action": "speak",
          "ref_msg": 1042
        }
      ],
      "timed_out": false,
      "joined_at": "2026-05-23T03:07:38Z",
      "last_action_at_turn": 41
    }
  }
}
```

Constraints:

- `turn_counter` is project-wide (ruling 9, msg 1075).
- `next_escrow_id` and `next_transaction_id` are persisted so process restarts don't reuse ids.
- `escrow_held` MUST equal `escrow_items.iter().map(|e| e.amount).sum()`. Invariant check on every write.
- `balance + escrow_held` represents total seat funds. Display surfaces show `balance` (settled) and `escrow_held` separately.

## Lock semantics

**Ruling 9-corrected (architect msg 1121, supersedes msg 1075 ruling 9):** the existing `with_board_lock` (collab.rs) and `with_file_lock` (vaak-mcp.rs) resolve to a SECTION-scoped lock path via `active_lock_path` / `get_active_section`. Two seats in different sections holding "the board lock" hold DIFFERENT files. That breaks project-wide currency exclusion (developer:1 msg 1111 + developer:0 msg 1115 disk verification).

Add a new project-wide lock primitive AND a single combined entry point, defined identically in BOTH binaries (dev-challenger:0 msg 1123 ordering guardrail):

- **Currency lock path:** `.vaak/currency.lock` — section-independent, always the same path
- **`with_currency_lock(dir, F)`** — base helper that acquires only the currency lock. Defined in `collab.rs` AND `vaak-mcp.rs`. Same fcntl/flock semantics as the existing board lock.
- **`with_currency_and_board_lock(dir, F)`** — SINGLE entry point for any code path that touches BOTH files. **Implementation (per developer:0 msg 1129):** closure-nest the existing primitives, do NOT introduce RAII LockGuards. The existing `with_board_lock` (collab.rs) and `with_file_lock` (vaak-mcp.rs) are closure-style (acquire → run f → release after closure returns). Implement the combined helper as:
   ```rust
   // collab.rs (Tauri main)
   pub fn with_currency_and_board_lock<F, R>(dir: &str, f: F) -> Result<R, String>
   where F: FnOnce() -> Result<R, String> {
       with_currency_lock(dir, || with_board_lock(dir, f))
   }
   // vaak-mcp.rs (sidecar) — same, but inner uses the section-scoped with_file_lock
   pub fn with_currency_and_board_lock<F, R>(dir: &str, f: F) -> Result<R, String>
   where F: FnOnce() -> Result<R, String> {
       with_currency_lock(dir, || with_file_lock(dir, f))
   }
   ```
   Release order is automatic-LIFO because the inner closure returns before the outer does. No RAII refactor of existing locks; zero churn to existing callers. ~30 LOC per binary.

   This is the ONLY sanctioned way to compose the two locks. Callers manually nesting `with_currency_lock` + `with_board_lock` is forbidden and a reviewer-catch (eliminates the deadlock-by-reverse-order category).
- **Ordering rule (architect-locked):** when both locks held, `currency.lock` is OUTER; the section-scoped board lock is INNER. ALWAYS. Enforced structurally via `with_currency_and_board_lock` being the only public composition.
- **Atomicity guarantee:** with both locks held, `board.jsonl` append + `currency.jsonl` append + `balances.json` atomic-write happen in ONE critical section. Project-wide exclusion on currency files + per-section serialization on board file + atomic message-plus-transaction commit, all satisfied.

Integration template (commit (b) will follow):

```rust
with_currency_and_board_lock(dir, || {
    // currency.lock (outer) + board.lock (inner) both held below.

    // 1. Pre-check timed_out gate — read balances.json
    if balances.seats[from].timed_out { return Err("[TimedOut] ...") }

    // 2. Board append + mic transfer (already inside inner board lock)
    append_board_message(...)?;
    process_mic_transfer(...)?;

    // 3. Post-board currency processing
    let action = classify_action(&msg);
    if action != ActionKind::Exempt {
        append_currency_transaction(...)?;       // credit
        append_currency_transaction(...)?;       // escrow_hold
    }

    // 4. Tick processing if applicable (commit c)
    // 5. Atomic-write balances.json (single rename at end)
    Ok(())
})
```

Code paths that touch ONLY board (no currency) keep using existing `with_board_lock` / `with_file_lock`. Code paths that touch ONLY currency (e.g. tick processing outside a send) use bare `with_currency_lock`. The combined helper is for the send path that needs both.

Lock holder responsibilities (in order, inside the outer currency.lock critical section):

1. Pre-check: read balances.json. If sending seat is `timed_out == true`, return `[TimedOut]` error to caller. Do NOT append to board.
2. Inner: acquire section-scoped board lock and append `board.jsonl` + process mic transfer. Release inner lock.
3. Currency classify + credit + escrow_hold (append to currency.jsonl).
4. (If applicable) tick side effects: passive income for active seats (mic_advance only), escrow release for matured items (every non-human send + mic_advance), interest accrual (every non-human send + mic_advance).
5. Atomic write balances.json (single rename at end).
6. Release currency.lock.

## Action classification (Phase 1 — Pass/Speak only)

Edit and Test detection are Phase 3. In Phase 1 every non-Pass message classifies as Speak.

Authoritative classifier (Pass classification corrected per ui-architect:2 msg 1073):

```rust
fn classify_action(msg: &BoardMessage) -> ActionKind {
    if msg.from.starts_with("human:") {
        return ActionKind::Exempt;
    }
    let body_trim = msg.body.trim();
    let body_lc   = body_trim.to_lowercase();
    let subject_p = msg.subject.eq_ignore_ascii_case("passing");
    if msg.r#type == "status" && (
        body_trim.chars().count() < 100
        || body_lc.starts_with("pass")
        || subject_p
    ) {
        return ActionKind::Pass;
    }
    ActionKind::Speak
}
```

Behavior contract:

- Moderator messages are NOT exempt (directive msg 1060 explicit).
- Pass requires `type == "status"` AS A HARD GUARD. A `type == "review"` message that happens to be short or contain "pass" is still Speak. Eliminates the operator-precedence bug ui-architect:2 caught.
- Keyword match is anchored to body start (`starts_with`) or exact subject equality. Substring "pass" buried mid-body in a review/answer cannot downgrade.
- Body length uses Unicode `chars().count()`, not `.len()` (which counts bytes). Prevents multibyte-character mis-classification.

## Tick semantics (corrected per developer:0 msg 1069)

The directive's "rotation tick" splits into two cadences:

| Effect | When fires |
| --- | --- |
| Passive income (+1 copper per active seat) | Only on successful `mic_advance` inside `al_auto_advance` (assembly-on) |
| Escrow release (move matured `escrow_items` to settled balance) | Every successful non-human `project_send` AND every `mic_advance` |
| Escrow interest (per `INTEREST_PER_10_COPPER_HELD`) | Every successful non-human `project_send` AND every `mic_advance` |
| turn_counter increment | Every successful non-human `project_send` AND every `mic_advance` |

`turn_counter` is the single monotonic clock that drives escrow `release_turn` checks. When AL is off, increments happen per-send; when AL is on, mic_advance ALSO increments (so a single AL turn may tick twice — once for the project_send that triggered the advance, once for the advance itself). Spec accepts this — `release_turn` is fundamentally a "after N events" semantic and double-counting AL turns is consistent with "AL rotation is more eventful."

Passive income gate ON mic_advance only honors directive intent ("reward for being present in rotation"). Off-AL sessions don't earn passive — they earn from Speak/Pass actions directly.

## Lazy init

Sending seat with no `balances.json` entry auto-initializes (ruling 4, msg 1075):

1. Append `{"type":"init","seat":"...","amount":10000,"balance_after":10000,"reason":"lazy init on first send"}` to currency.jsonl.
2. Insert seat entry in balances.json with `balance: 10000, escrow_held: 0, escrow_items: [], timed_out: false, joined_at: now, last_action_at_turn: turn_counter`.

`project_join` also calls the same init helper (initial path). Existing in-flight seats (7 active today) lazy-init on their next send. Architect's tested invariant: there is exactly ONE `type:"init"` row per seat in currency.jsonl, ever.

## Replay rules

On startup, vaak-mcp.rs rebuilds balances.json from currency.jsonl. Algorithm:

1. Read currency.jsonl line-by-line.
2. For each line, JSON-parse. On parse failure:
   - If this is the LAST line of the file (no `\n` following): WARN and skip (partial write on crash, developer:0 msg 1069 ruling 8).
   - If this is any earlier line: HARD ERROR. The ledger is corrupted; refuse to start until repaired.
3. Validate `id` is `prev_id + 1`. Mismatch = HARD ERROR.
4. If a row has `type == "init"` AND the seat already has a prior `type == "init"` row in this replay → HARD ERROR. Enforces the "exactly ONE type:init row per seat" invariant (dev-challenger:0 msg 1080 nit #2, +1 developer:0 msg 1086, ui-architect:0 msg 1088).
5. Apply each transaction to in-memory state (mirror the rules above).
6. Atomic-write the rebuilt balances.json.
7. Compare against the on-disk balances.json IF it exists. Mismatch is a WARN, not an error — currency.jsonl is canonical, balances.json is a cache.

## Display conversion

Lives in collab.rs (ruling 6, msg 1075). Single source of truth — every UI consumer calls this helper or replicates its arithmetic.

```rust
pub struct CopperDisplay { pub gold: i64, pub silver: i64, pub copper: i64 }

pub fn copper_to_display(c: i64) -> CopperDisplay {
    let sign = c.signum();
    let abs  = c.abs();
    CopperDisplay {
        gold:   sign * (abs / COPPER_PER_GOLD),
        silver: sign * ((abs % COPPER_PER_GOLD) / COPPER_PER_SILVER),
        copper: sign * (abs % COPPER_PER_SILVER),
    }
}
```

Negative balances split signs evenly across all three fields so the display never reads "-1 gold 50 silver 0 copper" — instead "-1 gold -50 silver" for a -15000 copper balance. Phase 2 UI will format as `-1g 50s` with a leading minus.

## MCP tool surface (Phase 1)

### `currency_balance`

Params:
```json
{ "seat": "architect:0" }   // optional; defaults to caller's seat
```

Response:
```json
{
  "seat": "architect:0",
  "balance": 9985,
  "escrow_held": 15,
  "escrow_items": [ { "id": "esc_00f", "amount": 10, "release_turn": 47, "action": "speak", "ref_msg": 1042 } ],
  "timed_out": false,
  "turn_counter": 42,
  "recent_transactions": [ /* last 10 currency.jsonl rows for this seat, newest first */ ]
}
```

### `currency_ledger`

Params:
```json
{ "seat": "architect:0", "limit": 50 }    // both optional; default seat=all, limit=50, max=500
```

Response:
```json
{
  "transactions": [ /* up to `limit` most-recent currency.jsonl rows */ ],
  "total_count": 89,
  "turn_counter": 42
}
```

### `currency_objection` (Phase 1 stub)

Signature locked now (ruling 5, msg 1075). Phase 2 fills the implementation without breaking callers.

Params:
```json
{ "to": "developer:1", "ref_msg": 1042, "reason": "untested code path" }
```

Response (Phase 1):
```json
{ "error": "Not implemented yet. Phase 2." }
```

## project.json adversarial tags

Add `adversarial: true` to two role configs (directive msg 1060 explicit list):

```json
"evil-architect":  { "title": "Evil Architect", "adversarial": true, /* existing fields */ },
"dev-challenger":  { "title": "Developer Challenger", "adversarial": true, /* existing fields */ }
```

`adversarial: true` is a no-op in Phase 1. Phase 4 retroactive Pass-negligence assesses the 10-copper penalty only against seats whose role has this tag. RoleConfig in collab.rs already supports arbitrary serde-default fields; no schema migration.

Conflict-of-interest disclosure: dev-challenger:0 msg 1063 flagged that this tag affects them. They reviewed the spec anyway; architect accepts the conflict disclosure and notes their adversarial review was independent of the tag-write itself (the tag is a Phase 4 input, not a Phase 1 behavior).

## Phased commit plan

Three commits, each independently reverting-safe.

### Commit (a) — Shadow build, no behavior change

Files:
- `desktop/src-tauri/src/collab.rs` (new helpers + constants)
- `desktop/src-tauri/src/bin/vaak-mcp.rs` (new MCP tool registrations + startup replay)
- `.vaak/currency.jsonl` (created on first write, empty initially)
- `.vaak/balances.json` (created on startup replay)

Behavior:
- Constants + types + ledger file format added
- Startup replay rebuilds balances.json
- `currency_balance`, `currency_ledger`, `currency_objection` MCP tools registered (read-only / stub)
- No project_send hooks wired
- All existing seats lazy-init at 10000 copper on their first send AFTER commit (a) ships AND step (b) lands; in (a)-only state, nothing writes to currency.jsonl

Owner: developer:1. ~300-400 LOC. Acceptance: `currency_balance` returns 10000 for a freshly-init'd seat; `currency_ledger` paginates correctly; restart preserves balances.

### Commit (b) — project_send hooks + classifier + adversarial tags

Files:
- `desktop/src-tauri/src/bin/vaak-mcp.rs` (project_send pre+post hooks, classifier)
- `.vaak/project.json` (adversarial:true on evil-architect + dev-challenger)

Behavior:
- TimedOut gate active (deficit cap rejects sends with `[TimedOut]`)
- Classifier wired (Pass/Speak per spec)
- Earn + escrow_hold transactions append on every successful non-human send
- adversarial tag present on the two roles
- Tick processing still NOT wired (passive/interest/release deferred to commit (c))

Owner: developer:0. ~150-250 LOC. Acceptance: a Pass message credits +1 copper + 1-copper escrow held; a Speak credits +10 + 10-copper escrow held; sending while timed_out returns `[TimedOut]`.

### Commit (c) — Tick processing

Files:
- `desktop/src-tauri/src/bin/vaak-mcp.rs` (`al_auto_advance` integration + per-send tick processing)

Behavior:
- turn_counter increments per non-human send and per mic_advance
- Escrow items past `release_turn` move to settled balance (append `escrow_release` transaction)
- Interest accrues per escrow item with `amount >= 10`
- Passive income +1 copper to every active seat on mic_advance

Owner: TBD (likely developer:1 or developer:0 — whoever has bandwidth after b lands). ~200-300 LOC. Acceptance: held escrow funds release after N ticks; interest accrues for held items ≥10 copper; passive income reaches all active seats on each mic_advance.

## Out of scope (explicit)

- Objection resolution / dispute pool — Phase 2.
- Edit detection via PostToolUse hook — Phase 3 (the EDIT_* / TEST_* constants are reserved but not wired).
- Retroactive Pass negligence assessment — Phase 4 (the adversarial tag is set but not consulted).
- Frontend balance display — separate plan, owned by ui-architect lane.

## Adversarial review checkpoints

- Spec review (this document): dev-challenger:0 + ui-architect:0 + developer:0 sign off before commit (a) starts.
- Each commit: LOC-50 ping → adversarial lens → green-light before commit.
- Browser-test gate is non-applicable (no UI surface in Phase 1). Substituted: integration test that `currency_balance` returns correct values after a sequence of `project_send` calls.

## Open questions deferred to human

None blocking. Architect rulings v2 (msg 1075) locked all 9 outstanding ambiguities. Human is invited to correct any ruling before commit (a) ships.
