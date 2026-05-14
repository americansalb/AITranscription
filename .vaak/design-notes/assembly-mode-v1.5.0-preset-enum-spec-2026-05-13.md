# Assembly Mode v1.5.0 — Preset Enum (Inaugural Pattern-(c) Typed-Enforcement PR)

Author: architect:0
Date: 2026-05-13
Status: spec for review; developer-lane implements after adversarial pre-review pass
Lineage: dev-challenger msg 441 (proliferation finding) → architect msg 443 (fold-and-schedule) → multi-writer audit Instance 8 → evil-architect msg 613 (inaugural-PR designation)

## Purpose

Replace the 25+ free-floating `"Assembly Line"` string literals (and the matching sibling-set for `"Default chat"`, `"Delphi"`, `"Oxford"`, `"Continuous"`, `"Red Team"`, `"Town hall"`, `"Brainstorm"`) in `desktop/src-tauri/src/bin/vaak-mcp.rs` with a typed `Preset` enum. Compile-time enforcement of every preset comparison and assignment. First worked example of pattern (c) from the multi-writer audit's cross-instance pattern section.

## Why this is the right inaugural

1. **Small surface.** Single file (vaak-mcp.rs), one enum definition, ~25-30 call-site edits. Mechanical sweep, no architectural decisions required during implementation.
2. **Tractable.** Existing string-literal call sites are already grepped by dev-challenger msg 441. No spec ambiguity.
3. **Concrete pattern-(c) demonstration.** Every future write of a preset value goes through the enum; the compiler rejects bypass attempts. Sets the precedent for the four heavier instances (heartbeat unification, preset+floor.mode coordination, sidecar-version mismatch detection, rejected-send visibility).
4. **Wire-compatible.** Serde serialization preserves the existing on-disk string values; no migration of `.vaak/sections/*/protocol.json` required.

## Enum definition (proposed)

```rust
// `Copy` deliberately omitted per evil-architect msg 629 finding 5: future preset
// additions may carry associated data (e.g., a `Custom` variant with configuration),
// at which point dropping Copy would require fixing every call site. Clone is cheap
// for unit variants; not pre-committing to Copy avoids the future-migration trap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    #[serde(rename = "Assembly Line")]
    AssemblyLine,
    #[serde(rename = "Default chat")]
    DefaultChat,
    Delphi,
    Oxford,
    #[serde(rename = "Red Team")]
    RedTeam,
    #[serde(rename = "Continuous Review")]
    Continuous,
    #[serde(rename = "Town hall")]
    TownHall,
    Brainstorm,
}
```

The renames preserve wire compatibility with existing `protocol.json:preset` values written by all prior code. Roundtrip property: for every variant V, `serde_json::from_str::<Preset>(&serde_json::to_string(&V)?)? == V`. Test this exhaustively in the PR — one assertion per variant. Verified against the wire-string call sites in `apply_set_preset` (vaak-mcp.rs:~3433) and the discussion-mode check at vaak-mcp.rs:3578.

**Unknown-string deserialization: strict error (per dev-challenger msg 624 finding 2).** When `protocol.json` contains a preset string that doesn't match any rename, serde returns `Err`. Callers MUST propagate the error rather than silently fall back to a default. Rationale: today's chain caught multiple silent-inertia bugs (dead read_assembly_state, rule 4 string-prefix, rule 4 theatrical halt). Strict error is the consistent pattern — loud failure over silent masking. Forward-compat for new presets requires adding the variant BEFORE writing the new wire-string (the inverse migration order from today's natural drift).

## Cross-file scope (resolved per developer msg 646)

Cross-file grep complete: 9 production sites outside vaak-mcp.rs, all in scope for this PR.

- **main.rs:** 6 production sites — lines 3206, 3251, 3258, 3267, 3284, 4650 (`proto.preset` reads/writes against `"Assembly Line"` / `"Default chat"` literals).
- **protocol.rs:** 2 production sites — lines 392, 406 (legacy migration writes). Test fixture at line 546 stays as wire-string per Finding 6.
- **launcher.rs:** zero sites.
- **collab.rs:** unchecked — flag for developer's grep at commit 1 to confirm zero or include.

**Enum location: `desktop/src-tauri/src/protocol.rs`.** protocol.rs already owns the `Protocol` struct (which currently has `preset: String`) and is imported by both `main.rs` and `vaak-mcp.rs` (the `bin/` target). vaak-mcp.rs cannot be the canonical location because main.rs can't import from a bin target. protocol.rs is the natural home for the typed primitive.

Same verify-before-assert discipline as `feedback_audit_both_write_and_read_sides`: write-side has the enum primitive in protocol.rs; cross-file read sites in main.rs + vaak-mcp.rs + protocol.rs all migrate in this PR's scope.

## Migration sequence (revised per developer msg 646)

Single-monster-commit migration is bisect-hostile. Sequence as 6 commits, each independently testable and revertable:

1. **Define `Preset` enum in `protocol.rs` + serde tests with existing wire-string fixtures.** Adds the enum, derives (no Copy per Finding 5), and the per-variant deserialize-from-existing-wire-string assertions (closes Finding 8). Compile-passing, zero behavior change. cargo test green.
2. **Migrate `apply_set_preset` matrix in vaak-mcp.rs to `Preset`.** The canonical preset-to-(floor_mode, consensus_mode) map becomes typed at the key. Callers still pass strings at the public boundary; internal lookup converts via deserialization. One step removed from raw strings.
3. **Migrate vaak-mcp.rs read sites** — the ~9 sites where the 6246015 PRESET_* const wedge currently sits. Replace const comparisons with `Preset` variant matches.
4. **Migrate main.rs production sites** — the 6 sites at lines 3206, 3251, 3258, 3267, 3284, 4650. Each is a `proto.preset` read/write against literal strings; migration is mechanical.
5. **Migrate protocol.rs production sites** — the 2 legacy-migration writes at lines 392, 406. Test fixture at line 546 stays as wire-string per Finding 6.
6. **Cleanup: delete the PRESET_* consts the 6246015 wedge introduced.** Enum supersedes. The wedge becomes deletable cleanly because consts and enum coexisted only briefly during commits 3-5; commit 6 retires the const layer.

Each commit's tech-leader runtime-trace verification: read-side comparisons in that region use enum variants; write-side writes through serde; the cluster is internally consistent. If any commit fails the gate, revert that commit only; prior commits remain.

## Call-site migration

The 25+ existing sites in vaak-mcp.rs split into three categories:

1. **String-equality checks.** `preset == "Assembly Line"` → `preset_typed == Preset::AssemblyLine`. Compiler-enforced exhaustive match for future preset additions.
2. **String constructor for protocol.json writes.** `serde_json::Value::String("Assembly Line".into())` → `serde_json::to_value(Preset::AssemblyLine).unwrap()`. Wire output unchanged.
3. **Test fixtures.** Already-string literals in `#[test]` blocks (lines 4385+ per dev-challenger's grep). Migrate to typed values for consistency, but the wire-string in JSON fixtures stays unchanged.

The `apply_set_preset` function at vaak-mcp.rs:3433 owns the canonical preset-string map today. Migration target: `apply_set_preset` accepts a `Preset` enum argument; callers that currently pass `"Assembly Line"` get the enum value; the canonical map becomes `Preset → (floor_mode, consensus_mode)` instead of `string → (string, string)`.

## What this fixes (the lived class-of-bug demonstration)

Today (2026-05-13) the team caught four instances of write-side-primitive-vs-read-side-fragile patterns in vaak-mcp.rs — all of them involving string comparisons or derived signals that drifted from the writer's actual primitive. The preset literal proliferation is the same class at scale: 25+ independent string-equality checks, no source of truth, every one a future drift candidate. Migrating to `Preset` makes every preset-related comparison compiler-checked. Future renames produce 25 compile errors guiding the sweep; current pattern produces 24 silent drifts.

## Acceptance test

1. `cargo check --bin vaak-mcp` passes after the migration.
2. `cargo test --bin vaak-mcp` passes — preset-string equality tests still work because serde rename preserves wire output.
3. Roundtrip test: every `Preset` variant serializes and deserializes back to the same variant. One assertion per variant.
4. **Existing-wire-string deserialization test (per dev-challenger msg 635 finding 8).** Roundtrip tests are bug-invisible when both writer and reader agree on the same broken rename (the writer produces what the reader expects). Add fixture-based assertions that read the EXISTING on-disk wire strings the codebase actually writes today (independent source of truth):
   ```rust
   let cases = [
       ("Assembly Line",     Preset::AssemblyLine),
       ("Default chat",      Preset::DefaultChat),
       ("Delphi",            Preset::Delphi),
       ("Oxford",            Preset::Oxford),
       ("Red Team",          Preset::RedTeam),
       ("Continuous Review", Preset::Continuous),
       ("Town hall",         Preset::TownHall),
       ("Brainstorm",        Preset::Brainstorm),
   ];
   for (wire, expected) in &cases {
       let parsed: Preset = serde_json::from_value(serde_json::json!(wire)).unwrap();
       assert_eq!(parsed, *expected, "wire string {:?} should deserialize to {:?}", wire, expected);
   }
   ```
   This catches Finding 1's bug class (missing rename for Continuous → Continuous Review) even if the corrigendum is incomplete. Mandatory in the PR's test slate.
5. Behavioral test: post-migration, the existing 8 preset transitions in `apply_set_preset` still produce identical `floor.mode` and `consensus.mode` writes to protocol.json. Compare via fixture diff against pre-migration output.
6. Strict-error test: deserializing an unknown wire string (e.g., `"UnknownPreset"`) returns `Err`, not `Preset::DefaultChat` or another variant. Confirms the strict-error decision from dev-challenger finding 2 is upheld in code.

## Frontend impact (per ui-architect msg 633 finding 7)

The desktop frontend has three categories of preset-string usage:

1. **Pass-through display sites:** `ProtocolPanel.tsx:223` and `:327` render `protocol.preset` directly — whatever serde produces gets displayed. Wire preservation is sufficient; no UI change required.
2. **Hardcoded preset literals (break if wire changes):** `PhasePlanEditor.tsx:20` (`preset: 'Debate'`), `:25` (`preset: 'Brainstorm'`), plus test fixtures at `ProtocolPanel.test.tsx:29` and `PhaseRow.test.tsx:29,34,35`. Five frontend sites that need the wire strings preserved exactly. Wire preservation is sufficient.
3. **UI display labels (separate lifecycle from wire):** `CollabTab.tsx:2714` shows "Assembly Line" as button text when assembly is inactive. This is a user-readable label, not a comparison string. Stays readable regardless of any future wire-format migration.

Implication for this PR: wire-format preservation is sufficient for the desktop frontend; no UI changes needed. The `CollabTab.tsx:2714` "Assembly Line" UI label is a separate display string from the wire format and should stay user-readable regardless of any future wire-format migration. If a future PR renames a wire string for any reason, the hardcoded frontend literals in category 2 must be updated in the same PR — but that's not this PR's scope.

## Pattern (c) property this commit demonstrates (qualified per dev-challenger msg 624 finding 3)

Every CURRENT call site in vaak-mcp.rs uses the `Preset` enum after this PR. Comparisons against preset values are compile-time exhaustive matches; new preset additions edit the enum (one source of truth) and produce exhaustive-match warnings at every existing site (guided sweep). On-disk wire compatibility is preserved through serde renames.

**Honest limitation:** the Rust compiler cannot prevent a NEW future call site from constructing `serde_json::Value::String("Some Preset")` directly via raw `json!()` macros and bypassing the enum. The pattern-(c) property here is "every current site is enforced; future sites must follow the discipline." Tech-leader's runtime-trace gate (msg 424) is the interim enforcement against new bypasses.

A typed-wrapper module that makes bypass uncompilable (e.g., wrapping the preset field in a private newtype that only accepts `Preset` values, with a public constructor) is a v1.5.0.1 follow-up if/when a new caller introduces an untyped path. Not in scope for this PR to keep the inaugural surface small and reviewable.

This is the property the multi-writer audit's cross-instance pattern names as "compile-time impossible to bypass" — demonstrably achievable on current call sites; future-proof only with the v1.5.0.1 wrapper layer.

## Transitional asymmetry (per dev-challenger msg 624 non-finding)

After this PR, `Preset` is typed but its outputs in `apply_set_preset` (the `floor.mode` and `consensus.mode` values it produces) remain stringly-typed. That asymmetry is intentional for the inaugural scope — Instance 4 (preset + floor.mode coordination, multi-writer audit Instance 4) is the next slice and consumes this PR's typed primitive to extend the typing to the output side. The asymmetry exists for one v1.5.x cycle by design.

## Out of scope for this PR (deferred to subsequent v1.5.x)

- Heartbeat unification (multi-writer audit Instance 1) — different file, larger surface.
- preset+floor.mode coordination (Instance 4) — depends on this PR for the typed primitive, but the state-machine work is its own slice.
- Sidecar-version mismatch detection (Instance 9) — different code surface (client-side).
- Rejected-send visibility (Instance 10) — different data model (two timestamps per binding).

Each of the above gets its own v1.5.x PR following this template. This one establishes the pattern.

## Pre-review owed

Evil-architect adversarial pass on the enum naming, serde rename strategy, and migration sequence before developer ships. Dev-challenger: continue queue from msg 609 option 1 (pre-review of v1.5 designs) — this is the first concrete v1.5 design to attack.
