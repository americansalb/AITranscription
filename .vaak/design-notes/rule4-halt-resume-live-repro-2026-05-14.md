# Rule 4 halt-resume — live reproduction, 2026-05-14

Authored by tester:0 during the 30h autonomous window opened by human msg 725. Documents the live reproduction of the rule 4 halt-clear failure that paralyzed the team for ~30 minutes around 01:11Z – 01:45Z and was independently confirmed by tester:0, evil-architect:0, ui-architect:0, and developer:0.

## Diagnosis (already converged)

Per ui-architect msg 728: `vaak-mcp.rs:6310-6373` code path is correct — writes `floor_resumed_after_human` when `state.halted_for_human` flips false on a human post. Per ui-architect grep: zero `floor_resumed_after_human` events on `.vaak/sections/5-12/board.jsonl` across 724 messages, despite 11+ `floor_halted_for_human` events on the same board.

Conclusion: **deployment gap, not code bug.** Pre-v1.0.5 (`fdae942`) sidecar processes do not contain the resume-event write logic. Active agent sessions retain the sidecar process they were spawned with (per `feedback_sidecar_rebuild_per_process_stale`), so any session predating `fdae942` is a permanent halt-trap for the role it owns. The halt only clears when one of the post-fdae942 sidecars triggers `al_auto_advance`, which has a halt-clear side effect — but the timing of that side effect is not the design intent (immediate-on-human-post). It is whatever timing `al_auto_advance` has.

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

## Permanent fix (next vaak.exe reinstall by human)

Reinstalling vaak.exe rolls every active agent's sidecar to a post-fdae942 build. Once every sidecar in the rotation contains the `floor_resumed_after_human` write logic, every human post triggers the resume event correctly. This is the deployment gap closing organically.

Until then, treat any `floor_halted_for_human` event as a candidate stuck state and apply the manual workaround if the halt persists past one human post.

## Relation to msg 717 proposals

Evil-architect msg 723 gap 2 (human-as-hard-interrupt for proposal 1) is the structural fix for this entire failure class. Once a human `project_send` automatically force-releases the current mic AND clears any halt, regardless of declared turn state, the deployment-gap failure mode disappears because the halt-clear no longer depends on the speaker's sidecar version. The current implementation makes the halt-clear path the speaker's responsibility (the agent writing `al_auto_advance` clears it); the proposed fix makes it the sidecar's responsibility on any human send.

## Relation to multi-writer audit doc

This case is a concrete failure under Instance 9 (binary deployment per-process / fragmented cohort) in `.vaak/design-notes/multi-writer-audit-2026-05-13.md`. The fragmented-cohort thesis: when different sidecar versions coexist, behavioral contracts that depend on writer-side participation (like the resume event) silently fail for the older cohort. Architect-lane should consider folding this as a worked example into the audit doc.
