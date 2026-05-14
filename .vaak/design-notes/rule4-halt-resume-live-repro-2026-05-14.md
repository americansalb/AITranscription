# Rule 4 halt-resume — live reproduction, 2026-05-14

Authored by tester:0 during the 30h autonomous window opened by human msg 725. Documents the live reproduction of the rule 4 halt-clear failure that paralyzed the team for ~30 minutes around 01:11Z – 01:45Z and was independently confirmed by tester:0, evil-architect:0, ui-architect:0, and developer:0.

## Diagnosis (converged through three corrections)

Per ui-architect msg 728: `vaak-mcp.rs:6310-6373` code path is correct — writes `floor_resumed_after_human` when `state.halted_for_human` flips false on a human post. Per ui-architect grep: zero `floor_resumed_after_human` events on `.vaak/sections/5-12/board.jsonl` across 724 messages, despite 11+ `floor_halted_for_human` events on the same board.

**Sharpened diagnosis (tester msg 745 code-trace correction):** the resume logic at vaak-mcp.rs:6297-6375 is gated by `if asm_active && state.role == "human"` at line 6302. The `state.role == "human"` check confines this code path to **the human's vaak.exe process specifically** — AI agent sidecars never have `state.role == "human"`, so they never run the resume write regardless of build version. The "stale sidecars in rotation" framing was directionally wrong; the actual gap is the human's binary build.

Conclusion: **deployment gap is specifically the human's running vaak.exe**, not the multi-cohort agent sidecars. The human's currently-running vaak.exe predates `fdae942`, so when human:0 posts a message, their own sidecar processes the send but the v1.0.5 resume-write code does not exist in their binary. The halt-clear side effect via `al_auto_advance` (which AI sidecars write) is the only observed organic clearing — and its timing is incidental, not the design intent.

This sharper diagnosis matters for the v1.5.1 spec acceptance gate: A1 ("Human posts a plain message... Within 5 seconds, halted_for_human == false") will fail today regardless of which AI sidecars are in rotation; it depends entirely on the human's vaak.exe build being ≥ `fdae942`. Architect msg 749 folded this into the spec (A1 is now explicitly out-of-PR-scope; the fix is the pending vaak.exe reinstall, not new code).

## Timeline (msg ids from `.vaak/sections/5-12/board.jsonl`)

| time (UTC) | msg | actor | event |
|---|---|---|---|
| 01:09:20 | 716 | human:0 | directive "review everything" — should clear halt armed by msg 713 |
| 01:11–01:15 | — | evil-architect:0 | three project_send attempts rejected `[FloorHalted]` (per evil-arch msg 723) |
| 01:11–01:18 | — | tester:0 | multiple project_send attempts rejected `[FloorHalted]` |
| 01:12:59 | 717 | human:0 | directive proposing typed turn semantics — did NOT clear halt |
| 01:19:20 | 718 | human:0 | directive "assembly on architect on you" — appears to clear halt for next sender |
| 01:19:44 | 719 | architect:0 | sent successfully; included new yield_to(target=human), re-armed halt via msg 720 |
| 01:19:44 | 720 | system | floor_halted_for_human(triggered_by=architect:0) |
| 01:22:23 | 721 | system | mic_released(from=architect:0, reason=floor_stall) — mic free, halt persists |
| 01:41:19 | 722 | human | mic_released(reason=human_force_release) — mic free, halt still persists |
| 01:41:45 | 723 | evil-architect:0 | sent successfully; new yield_to(target=human), re-armed halt via msg 724 |
| 01:42:18 | 725 | human:0 | directive "fix it all" — did NOT clear halt |
| ~01:45:00 | rev 451 | developer:0 | manually set `halted_for_human=false` in protocol.json |
| 01:45:01 | 726 | developer:0 | sent successfully, broadcasting the manual fix |

## Confirmed symptoms

1. **Free-form `to:all type:directive` from human:0 does not clear the halt.** Three human directives (716, 717, 725) all failed to clear active halts.
2. **Assembly-routing directive (msg 718 "assembly on X on Y") DOES clear the halt** as observed, though the mechanism is most likely `al_auto_advance` firing on the directive's mic-routing payload rather than a direct resume write.
3. **Human force-release of the mic (msg 722) does NOT clear the halt.** Mic state and halt state are independent state machines.
4. **`al_auto_advance` writes by post-fdae942 sidecars clear the halt as a side effect.** This is the only observed organic recovery path during a stuck state — and it requires a post-fdae942 sidecar to be active in the rotation.

## Reproduction steps

1. Any agent yields to human with `metadata.yield_to.target = "human"` (legacy yield_to with `_legacy_compat:true` also triggers).
2. System writes a `floor_halted_for_human` event; `protocol.json` floor state has `halted_for_human:true`.
3. Human posts any `to:all type:directive` message. If no post-fdae942 sidecar is in rotation, the halt persists indefinitely.
4. Other agents' `project_send` calls reject with `[FloorHalted] Floor halted for human:0; rotation resumes after the human posts. This send is not lost — re-send after the human responds.`

## Manual workaround (used by developer:0 at rev 451)

Edit `.vaak/sections/<section>/protocol.json` directly:
- Set `floor.halted_for_human` from `true` to `false`
- Increment `rev` by 1
- Update `rev_at` to current ISO timestamp
- Update `last_writer_action` to a descriptive string (developer used "halt_cleared_under_human_fix_it_all_mandate")
- Update `last_writer_seat` to the editing agent's seat

The file write is atomic; no file lock needed for this single-field flip. Other agents will pick up the new state on their next `get_protocol` or `project_send` attempt.

## Permanent fix (human's vaak.exe reinstall)

Reinstalling vaak.exe replaces the **human's** running binary with a post-fdae942 build. Once the human's vaak.exe contains the v1.0.5 `floor_resumed_after_human` write logic (gated to `state.role == "human"` per vaak-mcp.rs:6302), every human post will trigger the resume event correctly. AI agent sidecar versions are irrelevant — they never execute the resume code path regardless of build.

The wrap doc's pending decision #1 — deploy the existing build at `desktop/src-tauri/target/release/vaak-desktop.exe` — is the permanent fix. No additional code changes needed; v1.0.5 is already on disk waiting to be installed.

Until that deploy, treat any `floor_halted_for_human` event as a candidate stuck state and apply the manual workaround if the halt persists past one human post.

## Relation to msg 717 proposals

Evil-architect msg 723 gap 2 (human-as-hard-interrupt for proposal 1) is the structural fix for this entire failure class. Once a human `project_send` automatically force-releases the current mic AND clears any halt, regardless of declared turn state, the deployment-gap failure mode disappears because the halt-clear no longer depends on the speaker's sidecar version. The current implementation makes the halt-clear path the speaker's responsibility (the agent writing `al_auto_advance` clears it); the proposed fix makes it the sidecar's responsibility on any human send.

## Measurement caveats (per evil-architect msg 775)

When measuring proposal 3 (passing-by-default culture) adoption on this section's board, the audit numbers carry three confounds that prevent treating any short-window pass-rate spike as adoption signal:

1. **Phase bias.** Passes are the natural shape of "burn cycle wind-down — nothing to add" regardless of any briefing rule. Mid-debate windows in the same session (msgs 723→738) produced zero passes against four substantive challenges. Pass discipline is designed to fire in active-discussion contexts where roles have the option to pile on; in wind-down everyone has the option to do nothing. Adoption signal must be measured across debate cycles, not within a single wind-down.

2. **Wire-protocol mismatch.** The first observed pass messages (754, 756, 769, 771, 777, 779, 781) all used `metadata.turn_type: "passing"` — a proposal 1 wire-protocol field whose spec landed on disk at `.vaak/design-notes/assembly-mode-v1.5.1-typed-turn-declaration-spec-2026-05-14.md` but whose implementation is NOT shipped (v1.5.1 commit 1 was the legacy-compat removal at 903e582, not the typed-turn enum). The team voluntarily adopted an unshipped field via convention. That measures cultural-discipline coordination, not briefing-rule adoption — and the proposal-3 briefing edit at 26ec289 doesn't reference the `turn_type` field at all.

3. **Briefing-propagation gap.** Active agent sessions hold their original briefings loaded at session start. The proposal 3 briefing changes (briefingGenerator.ts at 26ec289 + `.vaak/roles/*.md` workspace edits + the adversarial-tag fix at workspace-config layer) do not reach existing agents until they re-read their briefings. The passes observed during this rotation came from team-directive compliance (msgs 723/737/765/767 said "pass"), not from briefing-rule adoption. Re-running audit_pass_rate.py after the human's next vaak.exe restart (which forces fresh briefing reads on agent re-spawn) is the measurement that actually tests briefing-driven adoption.

**Honest framing:** Directive-driven adoption works in under 1 hour from a team-coordinated pass-discipline announcement. Briefing-driven adoption requires session restart + a fresh discussion cycle and remains untested as of this doc's date.

## Relation to multi-writer audit doc

This case is a concrete failure under Instance 9 (binary deployment per-process / fragmented cohort) in `.vaak/design-notes/multi-writer-audit-2026-05-13.md`. The fragmented-cohort thesis: behavioral contracts that depend on a specific writer (here, the human's sidecar) silently fail when that writer's binary version lags the contract. The "single-writer gap" in this case is sharper than the typical multi-writer audit instance — there is only one writer for this code path (the human's vaak.exe), and its version determines whether the contract holds at all. Worth folding as a worked example into the audit doc.
