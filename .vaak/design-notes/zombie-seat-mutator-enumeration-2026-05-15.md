# Zombie-Seat Mutator Enumeration — 2026-05-15

Deliverable for architect msg 2824 zombie-seats spec drafting. Tables all writers/readers of the rotation_order class-of-bug surface across the Vaak codebase, plus the sessions.json:bindings status lifecycle.

Owner: developer:1. Companion to commits 09a29dd (TS filter), 7f658c3 (Rust filter), 0e73ba8 (status-exclusion tightening).

---

## 1. Authoritative State Paths

Live source: `.vaak/sections/<active-section>/protocol.json`
Active section read from: `get_active_section(project_dir)` (vaak-mcp.rs)
Read helper: `read_protocol_for_section_value(project_dir, section)`

Legacy stub: `.vaak/protocol.json` (top-level)
- Migration shim from 2026-05-14; `last_writer_action = "migrate_from_legacy"`
- Verified empty rotation_order on the current project (rev_at 2026-05-14T06:13:38Z)
- Tester msg 2786 misread this file as authoritative — corrected via msg 2816

Asymmetry consequence: any zombie-seats fix must operate on the section-scoped file, NOT the top-level stub. Sweep mechanisms iterating `.vaak/protocol.json` only would no-op on the live drift.

---

## 2. rotation_order Writers

| Site | Mutation | Teardown Path | Status |
| --- | --- | --- | --- |
| vaak-mcp.rs:8001-8016 (`handle_project_join` append branch) | APPEND if not present | Explicit JOIN | Dedup-only; never removes prior duplicates from rejoins |
| vaak-mcp.rs:9562-9568 (`handle_project_leave` prune) | `arr.retain(seat != leaver_label)` | Explicit LEAVE | ✅ Wired (human msg 2299 "leaving glitch" fix) |
| vaak-mcp.rs:9666-9684 (`handle_project_kick` prune) | `arr.retain(seat != target_label)` | Explicit KICK | ✅ Wired (same fix landed cross-handler) |
| vaak-mcp.rs:3210 (`read_protocol_for_section_value` legacy migration) | `floor.insert("rotation_order", arr.clone())` | One-shot migrate | Reads legacy `assembly.json`; runs once on first read after migration |
| collab.rs:1140 (`default proto`) | `"rotation_order": []` | Default-init | ✅ Empty init |
| collab.rs:1189 (`collab_init` seed with active) | `"rotation_order": order` | Init seed | Uses `active_assembly_seats` filter |
| collab.rs:1549 (`roster_remove_slot` cross-section prune) | `arr.retain(seat != target_label)` over all section files | Roster removal | ✅ Iterates `sections/*/protocol.json` — correct scope |
| launcher.rs:2805 (initial launcher seed) | `"rotation_order": []` | Launcher init | ✅ Empty init |
| main.rs:3253 (`protocol_state` enable) | `proto.floor.rotation_order = active_seats` | Tauri enable | Re-seeds at enable |
| main.rs:3260 (`protocol_state` disable) | `proto.floor.rotation_order = vec![]` | Tauri disable | Reset on disable |
| main.rs:5510-5535 (watchdog mic_released) | **NO rotation_order write** — only `current_speaker = null` | Implicit watchdog teardown | **GAP** — dead-speaker entry persists indefinitely |

### GAP: Implicit Teardown Paths

| Path | rotation_order Cleanup? | Notes |
| --- | --- | --- |
| Watchdog mic_released (max_floor_exceeded) | NO | main.rs:5510-5535 clears current_speaker only |
| Watchdog mic_released (floor_stall) | NO | Same code path; same gap |
| run_stop_hook (vaak-mcp.rs:12819) | NO | Hook is "continuation-injector," not a teardown — verified via grep, no rotation_order or bindings writes |
| Claude Code window close (SIGTERM-equiv) | NO | MCP child gets terminated; no chance to write cleanup |
| OS crash / kill -9 | NO | Same as window close |
| auto-respawn (launcher) | Unverified — needs check whether spawned sidecars push status writes |

### Live Empirical Drift

Today's snapshot at `.vaak/sections/5-12/protocol.json`:
- `rotation_order` carries 11 seats: architect:0, developer:0, developer:1, tester:0, dev-challenger:0, evil-architect:0, ux-engineer:0, ui-architect:0, platform-engineer:0, moderator:0, audience:0
- Section `started_at = 2026-05-14T08:43:30Z` — ~2 days of accumulated drift
- `sessions.json:bindings` carries 6 active seats only — the other 5 are zombies (no binding entry at all)

Five seats have NO binding in sessions.json:bindings (vs section rotation_order showing them) — meaning they never went through explicit project_leave/project_kick teardown. They got into rotation_order via project_join → at some point closed without explicit teardown → left rotation_order entry behind.

---

## 3. rotation_order Readers

| Site | Use | Filter? |
| --- | --- | --- |
| vaak-mcp.rs:2674-2675 (`read_assembly_state`) | Returns asm.rotation_order | No filter (raw read) |
| vaak-mcp.rs:2838-2849 (`next_assembly_speaker`) | Rotation advance | YES — filters via `active_assembly_seats` predicate (presence+freshness) |
| vaak-mcp.rs:2923-2923 (`handle_assembly_line` get_state) | Tool response to caller | No filter (raw read) |
| vaak-mcp.rs:8810 (mic_landed body construction) | `[YOUR TURN]` Rotation: line | **YES** — 7f658c3 + 0e73ba8 `seat_has_binding` (presence + status exclusion) |
| main.rs:3210, 3286 (protocol_state status return) | Tauri command response | No filter |
| ProtocolPanel.tsx:318 (CompactMicLine pill row) | UI rotation strip | **YES** — 09a29dd `heartbeats[seat]?.connected === true` |
| AssemblyControls.tsx:89 (renderStatusLine) | UI "Next:" preview | **YES** — 09a29dd `activeSeatSet.has(seat)` |

### Filter Predicate Divergence (Per UI-arch msg 2790 + Architect msg 2808 Canonical)

Canonical predicate per architect msg 2808: `binding_exists AND status NOT IN {"revoked", "left"}`

| Site | Predicate | Drift From Canonical? |
| --- | --- | --- |
| Rust vaak-mcp.rs:8830-8853 (0e73ba8) | `binding_exists AND status NOT IN {"revoked", "left"}` | **MATCH** (canonical implementation) |
| TS ProtocolPanel.tsx (09a29dd) | `heartbeats[seat]?.connected === true` | **STRICTER** — `connected` resolves to `status == "active"` per main.rs:3475, so excludes `"idle"`, `"disconnected"`, missing-status. Canonical accepts `"idle"`. |
| TS AssemblyControls.tsx (09a29dd) | `activeSeatSet.has(seat)` | **Likely stricter** — `activeSeats` from `list_active_seats_cmd`; need to verify Rust handler's predicate. |

→ Action: TS sites need loosening to match canonical (accept `"idle"`), or canonical needs strengthening to reject `"idle"`. Architect spec call.

`heartbeats.connected` derivation: main.rs:3475
```rust
"connected": b.get("status").and_then(|s| s.as_str()).map(|s| s == "active").unwrap_or(false)
```
Confirmed: connected ↔ status=="active" only.

---

## 4. sessions.json:bindings Status Lifecycle

| Status | Writer | Trigger |
| --- | --- | --- |
| `"active"` | vaak-mcp.rs:7835 (`bindings[idx]["status"] = "active"`) on project_join | Initial bind; rebinds on rejoin |
| `"revoked"` | vaak-mcp.rs:9646 (`obj.insert("status", "revoked")`) in handle_project_kick | Kick; entry KEPT in bindings |
| `"disconnected"` | vaak-mcp.rs:1052 (`binding["status"] = "disconnected"`) when activity flips to "disconnected" | Activity-driven (likely Tauri-side detection of MCP child death) |
| `"idle"` | Unverified — referenced as filter pass value at vaak-mcp.rs:2216-2217 + 2751-2752 — needs writer trace |
| (no `"left"`) | handle_project_leave physically REMOVES the entry (vaak-mcp.rs:9544-9548 `bindings.retain`) — no `status="left"` ever written today |

### Status Transitions

```
join → active
active → revoked (kick)
active → disconnected (activity=disconnected)
* → (entry removed) (leave)
```

### Asymmetric Teardown Pattern (UI-arch msg 2806 + dev-challenger msg 2830 omission-paths memory)

- handle_project_leave: PHYSICALLY REMOVES entry
- handle_project_kick: MARKS status="revoked", KEEPS entry
- activity-disconnect: MARKS status="disconnected", KEEPS entry

Same pattern across audit-omission classes. Zombie-seats spec should choose a convention:
- (a) Both remove (clean simplicity, no audit trail)
- (b) Both mark (audit trail preservation; consumers must filter)
- (c) Status-of-the-art today is mixed — document and accept

Recommended (b) per audit-trail principle: if option (a), audit consumers (UI rendering kicked-history, team-status reports) lose data.

---

## 5. Heartbeat Freshness Constants

| Constant | Value | Location | Use |
| --- | --- | --- | --- |
| `ASSEMBLY_SEAT_FRESHNESS_SECS` | (Unread — needs check) | vaak-mcp.rs near 2757 | `active_assembly_seats` filter for rotation-honor decisions |
| `ACTIVITY_TTL_SECS` | `60` | vaak-mcp.rs:8824 (function-local const) | mic_landed Rotation strip activity-decay (TTL after which `stored_activity` decays to "idle") |

Action: enumerate any other freshness constants if zombie-seats spec needs them at rotation-advance vs display-render granularity.

---

## 6. Per-Section Migration

Each section maintains its own `protocol.json` under `.vaak/sections/<section>/`. When the active section changes (via `mcp__vaak__switch_section`), the new section's rotation_order is read fresh. There is NO migration that prunes stale entries across section switches — section "5-12" today carries 2 days of accumulated drift because it was the active section across multiple sessions.

Verification gap: does `switch_section` reset rotation_order for the destination section, or accept whatever state is on disk? Unverified — flagged for zombie-seats spec.

---

## 7. Proposed Sweep Mechanism (Spec Scope, Not Implemented)

Per evil-arch msg 2750/2752/2788/2820 framing. Defense at the data layer.

Candidate triggers:
1. **On every project_send** (or every N seconds via watchdog tick): iterate active section's `floor.rotation_order`, drop entries where the seat doesn't appear in `sessions.json:bindings` with status NOT IN {revoked, left, disconnected}.
2. **On project_join**: if rejoining seat is already in rotation_order, leave it (current behavior); but ALSO sweep entries for seats with no binding at all (orphaned from prior session).
3. **On watchdog mic_released**: actively prune the released seat from rotation_order at the same time current_speaker is nulled — fixes the GAP identified in §2.

Cost: each sweep is one file lock + one sessions.json read + one protocol.json read+write. Negligible at idle; once per rotation tick is fine.

---

## 8. Filter Contract Cross-Reference

The canonical predicate per architect msg 2808 is `presence(seat) = binding_exists AND status NOT IN {"revoked", "left"}`.

Forthcoming filter-contract spec (folded into zombie-seats spec per architect msg 2776 + UI-arch msg 2774):
- All four predicates from evil-arch msg 2772 (presence, engagement, rotation-honor, kick-eligibility) with composition table
- Authoritative tracker: `sessions.json:bindings` for `presence`; `last_alive_at_ms` from `.vaak/sessions/<role>:<inst>.json` for `engagement`
- CI fixture exercising all three filter sites against canonical input (per architect msg 2800 sequence step 3)

---

## 9. Action Items For Zombie-Seats Spec Drafting

1. **Watchdog mic_released rotation_order prune** — main.rs:5510-5535 needs same `arr.retain` shape as kick/leave handlers; ~5-8 LOC.
2. **Sweep mechanism** — on project_send tick or watchdog tick, prune orphan entries; ~15-20 LOC.
3. **TS filter alignment** — loosen ProtocolPanel + AssemblyControls to canonical (accept `"idle"`), OR strengthen canonical to reject `"idle"`. Architect call.
4. **Asymmetric teardown convention** — pick (a)/(b)/(c) per §4.
5. **Per-section switch_section behavior** — verify rotation_order migration semantics; spec the desired behavior.
6. **CI fixture** — three sites × canonical input → identical output assertion.
7. **Filter-contract folding** — single zombie-seats spec absorbing four predicates + composition table.

---

## 10. References

- Commits: 09a29dd (TS filter), 7f658c3 (Rust filter), 0e73ba8 (status-exclusion tightening)
- Memories firing this rotation: feedback_audit_class_not_just_symbol, feedback_verify_before_asserting (firing 7×), feedback_audit_both_write_and_read_sides, project_assembly_mode_gaps_2026_05_04, project_multi_writer_audit_2026-05-13
- Discussion thread: msgs 2747 (human) → 2750/2752 (evil-arch) → 2754 (UI-arch) → 2760/2766/2776 (architect) → 2784/2816/2826 (dev:1) → 2792/2820 (evil-arch un-corrections) → 2806 (UI-arch empirical) → 2808/2824 (architect arbitration) → 0e73ba8 (ship) → 2828 (tester PASS)
