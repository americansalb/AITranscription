# Hot-reload architecture spec — sidecar-as-thin-proxy

**Author:** architect:0 (2026-05-28 ~15:30Z, per human msg 2415 directive)
**Status:** Phase 1 spec; Phases 2-5 sketched. Spec only — no implementation. developer:0 + developer:1 own build; tester:0 owns per-phase verification.

## Problem statement (per human msg 2415)

> The restart-to-propagate-changes problem has cost more time than any bug in the project's history. Target: I never restart Claude Code windows for code changes. I restart Vaak (the Tauri app) at most. Agent sessions and context survive all updates.

The current pain: every change to `vaak-mcp.rs` requires:
1. `cargo build --release` of the sidecar binary
2. `npm run build-sidecar` to bundle
3. Closing ALL Claude Code windows (kills agent context — chronic complaint per `feedback_stop_telling_human_to_restart_cc_windows`)
4. Restarting Vaak
5. Reopening each Claude Code window (loses session and context)

Steps 3 + 5 cost the most. Steps 1 + 2 are unavoidable on any binary change.

## Target architecture (locked from human directive)

- **Sidecar (`vaak-mcp.rs`)** becomes thin MCP proxy:
  - Tool registration with Claude Code (stdio JSON-RPC)
  - Arg serialization
  - HTTP POST to `localhost:7865/mcp/<tool_name>`
  - Response deserialization + return to Claude Code
  - Estimated ~500 LOC after migration completes
  - Changes ONLY when MCP tool surface itself changes (add/remove/rename a tool)

- **Tauri app (`main.rs` + new modules)** holds ALL business logic:
  - Existing `Server::http("127.0.0.1:7865")` at main.rs:1161 (tiny_http crate) gets new `/mcp/<tool_name>` endpoints
  - Each endpoint receives the sidecar's POST, deserializes the payload (project_dir + session_id + role + instance + tool args), executes the orchestration logic, returns JSON response
  - Restart of Vaak (the Tauri binary) atomically swaps the running endpoint code; running sidecars next POST hits the new code; no sidecar restart required; CC windows untouched

## Existing infrastructure that's already there

Confirmed by `git grep` at 2026-05-28 15:30Z:

- **`Server::http("127.0.0.1:7865")`** at `main.rs:1161` — tiny_http, sync recv loop with shutdown flag check, runs in a dedicated thread spawned by `start_speak_server()`. Already handles `/heartbeat`, `/speak`, `/collab/notify` POSTs.

- **Sidecar already POSTs to localhost:7865** at multiple sites:
  - `vaak-mcp.rs:949` → `/collab/notify`
  - `vaak-mcp.rs:16912` → `/heartbeat`
  - `vaak-mcp.rs:17066` → `/speak`

- **`Server::http` shutdown flag** (`HTTP_SERVER_SHUTDOWN`) at main.rs:1174 — already plumbed for clean shutdown on Tauri exit.

**The sidecar → Tauri HTTP channel pattern is ESTABLISHED.** Phase 1 doesn't invent it; it generalizes it.

## Sidecar handler inventory (current state)

`vaak-mcp.rs` is 20,503 LOC with ~50 `handle_*` functions:

- 18 currency_* handlers (10270, 10670 currency_objection range, etc.)
- 9 oxford_* handlers (10320-11432)
- 6 delphi_* handlers (11525-12357)
- 4 project_send/check/wait/status (15094-15487)
- 5 project_join/leave/kick/buzz/claim handlers
- 3 audience_vote/history + discussion_control
- 1 assembly_line (~700 LOC: 3162-3839)
- 2 protocol_mutate / get_protocol
- 3 list/create/switch_section
- 1 update_briefing
- handle_request dispatcher at vaak-mcp.rs:17078

Each handler embeds business logic: file reads (sessions.json, currency.jsonl, board.jsonl, protocol.json), file locks (board.lock, currency.lock, etc.), JSON serialization, event broadcasts.

## Phase 1 — single-tool proof of concept

**Tool choice:** `assembly_line` (per directive's "one high-churn tool" example). Rationale:
- Single handler with clear inputs/outputs (action: enable|disable|get_state)
- Touches protocol.json (locked) but no board writes — simplest lock surface
- Recent intensive work (SHA-13.4 force re-seed) shows it's actively maintained
- ~700 LOC — meaningful proof but not the largest

**Tauri-side delta (`main.rs`):**

```rust
// In start_speak_server or its successor, add:
} else if method == "POST" && url == "/mcp/assembly_line" {
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() { /* 400 */ }
    let payload: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => { /* 400 */ }
    };
    // Extract sidecar-local state from payload
    let project_dir = payload["project_dir"].as_str().unwrap_or("");
    let session_id = payload["session_id"].as_str().unwrap_or("");
    let action = payload["args"]["action"].as_str().unwrap_or("");

    // Execute the moved-from-sidecar handler:
    let result = mcp_handlers::assembly_line(project_dir, session_id, action);

    // Respond
    let response_body = serde_json::to_string(&result).unwrap_or_default();
    let _ = request.respond(tiny_http::Response::from_string(response_body)
        .with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap()));
    continue;
}
```

The `mcp_handlers` module is NEW — `desktop/src-tauri/src/mcp_handlers/mod.rs` + `assembly_line.rs`. The function body is the EXISTING `handle_assembly_line` from vaak-mcp.rs:3162-3839 — moved, not rewritten.

**Sidecar-side delta (`vaak-mcp.rs`):**

```rust
fn handle_assembly_line(action: &str) -> Result<serde_json::Value, String> {
    let state = MCP_STATE.lock();  // or whatever the sidecar-local state singleton is
    let payload = serde_json::json!({
        "project_dir": state.project_dir,
        "session_id": state.session_id,
        "role": state.role,
        "instance": state.instance,
        "args": { "action": action },
    });
    let client = reqwest::blocking::Client::new();
    let resp = client.post("http://127.0.0.1:7865/mcp/assembly_line")
        .json(&payload)
        .send()
        .map_err(|e| format!("[ProxyError] {}", e))?;
    let body: serde_json::Value = resp.json()
        .map_err(|e| format!("[ProxyDecodeError] {}", e))?;
    Ok(body)
}
```

The sidecar handler shrinks from ~700 LOC to ~15 LOC.

**Locking surface considerations:**

The moved-to-Tauri handler runs INSIDE the tiny_http worker thread. It acquires the same file locks (protocol.json lock, etc.) that the sidecar would have. The locks are file-based (parking_lot mutex + fs lock pattern per memory `feedback_pipeline_mode_strict`), so cross-process serialization is preserved.

NOTE: if two sidecars POST simultaneously to `/mcp/assembly_line`, the tiny_http server handles them sequentially in the current loop pattern. For concurrent safety, the tiny_http loop should be changed to spawn-per-request OR the lock acquisition must be inside the handler (which it is). Current sync recv pattern is fine for the LOW-traffic /mcp endpoints; if traffic increases (Phase 4 covers project_send which is high-traffic), the loop will need a thread pool. NOT a Phase 1 blocker.

## Phase 1 acceptance criteria

After Phase 1 ships:

1. Sidecar `handle_assembly_line` is < 30 LOC (down from ~700)
2. New `desktop/src-tauri/src/mcp_handlers/assembly_line.rs` contains the moved logic
3. New `/mcp/assembly_line` endpoint in `start_speak_server` (or successor)
4. `assembly_line(get_state|enable|disable)` calls succeed from a running CC window
5. **Hot-reload test (the canary):** developer changes a behavior in `mcp_handlers::assembly_line`, restarts ONLY Vaak (NOT CC windows), calls `assembly_line(get_state)` from an existing CC session, observes the new behavior. **If this works, hot-reload architecture is validated.**
6. Cargo tests pass: existing assembly_line tests + new HTTP-route test
7. No behavior regression: SHA-13.4 force-reseed, SHA-V107.fix-1 cold-open carve-out, etc. all preserved

## Phases 2-5 (sketches; detailed specs per-phase)

**Pre-phase gate (added per evil-arch msg 2434 F3):** Each phase MUST start with a per-handler state-residency audit. For each handler migrating to Tauri, enumerate:
- Sidecar-local state the handler depends on (project_dir, session_id, role, instance — always; anything else is a yellow flag)
- File reads / writes (paths + locks)
- Hooks that depend on the sidecar process context (e.g., file-op-claim.py looks up session_id against bindings; moving the handler may break the lookup chain)
- Trust-model implications (e.g., currency Pass-gate's sender-side enforcement becomes centralized enforcement when handler moves to Tauri — that's a desirable change but the threat model shifts)

**Phase 2 — All currency_* tools migrated.** 18 handlers. Estimated ~2000 LOC moved out of sidecar, into `mcp_handlers/currency_*.rs`. Each gets `/mcp/currency_<tool>` endpoint. The currency.lock + balances.json + currency.jsonl interactions move with the handlers.

**Phase 2 state-residency audit prerequisites** (per evil-arch F3):
- `project_currency_gate_is_sender_side_enforced` — migration eliminates the stale-sidecar bypass (GOOD); document the trust-model shift to centralized enforcement
- `project_currency_edit_test_earns_dead` — file-op-claim.py hook lookup chain depends on sidecar process context; verify the lookup still works when the handler runs in Tauri (the hook still runs in the sidecar's lifecycle, but the handler it ultimately reaches via project_send proxy needs to honor the marker file)
- dev-challenger:0 invited to lead this audit before Phase 2 begins

**Phase 3 — oxford_* / delphi_* / assembly_* / discussion_control / audience_*.** Per-mode submodules under `mcp_handlers/`. ~3500 LOC moved. Each tool's pre-phase audit examines its event broadcast paths and any sidecar-cached state (e.g., active_oxford_debate read-cache).

**Phase 3.5 — tiny_http → thread-pool / async upgrade** (NEW per evil-arch msg 2434 F4). Extract the concurrency upgrade from Phase 4 to its own commit chain. Mixing infrastructure architectural change with handler-feature migration is the team's known highest-bug-risk pattern. Acceptance: existing /heartbeat /speak /collab/notify endpoints continue working under load test (e.g., 100 concurrent /heartbeat POSTs). Once Phase 3.5 lands cleanly, Phase 4 can proceed without compounded risk.

**Phase 4 — project_send + project_check + project_wait + project_status + project_join + project_leave + project_kick + project_buzz + protocol_mutate + get_protocol + list/create/switch_section + update_briefing + claim/release/claims.** Highest-traffic. ~6000 LOC moved. Depends on Phase 3.5 (tiny_http upgrade) landing first.

**Phase 5 — Auto-detect Tauri restart + re-handshake.** When the sidecar's POST fails with ECONNREFUSED or HTTP error, it should retry with exponential backoff (up to ~30s) instead of immediately erroring to Claude Code. When the Tauri restart completes, the sidecar resumes seamlessly. The MCP-side timeout per call needs to be tuned to accommodate the longest expected restart window.

**Phase 5 long-poll cursor durability** (NEW per evil-arch msg 2434 F5). project_wait holds the sidecar's HTTP POST open for ~55s. If Tauri restarts mid-poll, sidecar gets ECONNREFUSED on a half-completed poll. **Architect ruling: IDEMPOTENT RETRY with cursor in sidecar.** Each poll fully described by its `last_seen` argument; sidecar retries with the SAME `last_seen` on ECONNREFUSED. No disk persistence needed — cursor lives client-side (sidecar memory) and is included in every retry. Alternative considered: persist cursor to disk per poll in Tauri. Rejected as more complex without functional benefit.

**Final state:** vaak-mcp.rs is ~500 LOC. Tool surface registration + arg packaging + HTTP POST helper + backoff retry. Changes only when adding/removing/renaming a tool (rare). All business logic in `desktop/src-tauri/src/mcp_handlers/` modules.

## Migration sequencing & risk management

**Do NOT rewrite everything at once** (per directive). Each phase is a separate commit chain, separate test pass, separate ship.

**Risk: per-phase regressions.** Mitigation: tester:0 runs the existing cargo test suite per phase, plus a hot-reload-canary integration test (the Phase 1 acceptance criterion 5).

**Risk: dual code paths during migration.** Between Phase 1 and Phase 4, some tools are proxied and some are not. The sidecar handles both patterns. NOT a concern as long as the proxy boundary is clean per tool.

**Risk: file-lock semantics change.** The locks move from sidecar process to Tauri process. Cross-process semantics are unchanged (file-based locks). Same-process semantics within Tauri benefit from tighter coordination. NOT a regression risk if the locks are file-based throughout, which they are.

**Risk: high-traffic tool latency.** Phase 4 project_wait long-poll could starve the single-threaded tiny_http loop. Mitigation: upgrade to thread-pool or async (hyper/axum) BEFORE Phase 4. Phase 4 sub-step #0.

**Risk: state mismatch between sidecar's local state and Tauri's handler.** The sidecar's MCP_STATE includes session_id, role, instance, project_dir set at sidecar startup. Each POST must carry these. If they drift (e.g., sidecar's session_id changes mid-run), the Tauri handler sees stale state. Mitigation: the sidecar's state is set at MCP handshake and stable for the sidecar's lifetime. No drift concern in practice.

**Risk: HTTP server panics / crashes.** A panic in a handler currently propagates to the tiny_http worker thread. Mitigation: wrap each handler invocation in `std::panic::catch_unwind` and return a 500 with the panic message. Sidecar can retry. NOT a Phase 1 blocker; can be added in Phase 1.5 or alongside Phase 5's retry logic.

## Ownership

Per human msg 2415:
- **architect:0** owns the architecture (this spec)
- **developer:0 + developer:1** build (per-phase commit chains)
- **tester:0** verifies each phase (cargo test suite + hot-reload canary)

ui-architect:0 + ux-engineer:0 are NOT in scope; no UI changes in this architectural shift.

evil-architect:0 + dev-challenger:0 are encouraged to attack the spec adversarially BEFORE Phase 1 implementation begins — particularly on the locking surface analysis, the tiny_http thread model, and the migration sequencing.

## Open questions for the team

1. **Concurrency upgrade timing:** does Phase 1's tiny_http sync recv loop hold acceptable latency through Phase 3, or does the thread-pool / async upgrade need to land BEFORE Phase 1 to avoid two architectural changes back-to-back? My read: Phase 1 OK as-is; Phase 4 must precede with the upgrade.

2. **Sidecar state coupling:** is there any sidecar-local state (other than project_dir, session_id, role, instance) that a handler needs and which doesn't survive a Tauri restart? My read: the handlers I've inspected don't have such state — they read from disk. But a per-handler audit during migration is needed.

3. **Hot-reload canary test format:** what does tester:0 write for the Phase 1 acceptance criterion 5? Suggestion: a cargo integration test that spawns Tauri (or talks to a running Tauri), restarts it, makes a /mcp/assembly_line call before and after, asserts both responses are well-formed. NOT a unit test — an integration test.

4. **MCP timeout:** Claude Code has an MCP tool-call timeout (60s default per memory). If a Tauri restart takes >60s, the proxy POST times out and the tool call fails. Mitigation: Tauri restart is usually <10s; the retry loop in Phase 5 handles transient cases. Acceptable.

## Cross-references

- `feedback_stop_telling_human_to_restart_cc_windows` — the chronic pain this architecture closes
- `project_sidecar_relaunch_requires_claude_code_restart` — the activation chain this architecture eliminates
- `project_launch_wrapper_resume_masks_stale_sidecar` — the failure mode that goes away when sidecar persists across Tauri restarts
- `feedback_no_idle_after_first_slice` — phase-by-phase shipping discipline applies; each phase is a meaningful slice
- vision.md "Hot-reload architecture" section (queued for update once Phase 1 lands)

## Memory candidates

- `project_hot_reload_architecture_2026-05-28` — concrete chain commit + phase boundaries as they land
- `feedback_sidecar_business_logic_belongs_in_tauri` — design principle for future MCP tool work
