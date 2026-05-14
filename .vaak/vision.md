# Vaak Architecture Vision — feature/al-vision-slice-1 branch

Living document. Owned by: architect. Last updated: 2026-05-13.

## Scope

**This branch:** `feature/al-vision-slice-1` — Assembly Line v1.0 corrected. Fixes the routing class of bug where speaker prose (`yield_to.target`) overrode the canonical rotation order, structurally excluding peers from multi-round assemblies.

**Out of scope on this branch:** V2 Collab redesign. The V2 effort is tracked separately on `pr-pipeline-bundle` with a comprehensive 3158-line spec (`COLLABORATE_V2_SPEC.html`, committed at `9cdf4bd`, last updated 2026-04-25) and its own vision document (also at `9cdf4bd`:`.vaak/vision.md` v7). Per human directives id 729 + id 740, V2 and the current collab system are two separate architectures that coexist; this branch maintains current collab without modifying V2's design surface.

## What shipped on this branch (2026-05-13)

A 4-commit chain in `desktop/src-tauri/src/bin/vaak-mcp.rs` plus a frontend regression fix in `desktop/src/main.tsx`. Single feature: Assembly Line v1.0 corrected.

- `453228c` — rule 2 (strict rotation_order; `yield_to.target` is courtesy hint not authority) + rule 4 (human-stall on yield-to-human).
- `e582e6e` — rule 3a (AI `project_leave` gated during active assembly; `project_join` append-on-join is preserved as the late-summoner mechanism).
- `1c26267` — `project_status` returns `rotation_order`, `current_speaker`, `mic_held_secs` (acceptance-test surface).
- `7895a03` — `mic_held_secs` reads `proto.rev_at` (per-mic-advance) instead of `proto.floor.started_at` (per-assembly-enable). Caught at adversarial review by tech-leader:0.
- Plus: `8f2b97a` (UX view-button toast), `4c2cfc6` (launcher PID/window descendant walk), `a627daf` (activity-field + TTL), `84f6c15` (rotation-with-activity in [YOUR TURN]), `c43f917` (ToastProvider regression fix).

Spec on disk: `.vaak/design-notes/assembly-mode-v1.0-corrected-spec-2026-05-13.md`.

## The bug fixed (lived live during the design assembly)

During the 10-round design assembly that produced this spec, `architect:0` redefined "active roles" in prose at round 1 close — declaring three when `rotation_order` had four. Every speaker yielded within the 3-clique. The system honored those yields because `yield_to.target` was respected over `rotation_order`. `evil-architect:0` was structurally excluded from all 10 rounds despite being the conformity-break role the human explicitly summoned to prevent that outcome. Rules 2 + 3 + 3a make this exact failure mode mechanically impossible going forward.

## Class of bug this branch only partially addresses

Multi-writer shared state — multiple paths writing to overlapping fields with no single owner or atomic-write contract. Today's `yield_to.target` vs `rotation_order` is one instance; the dual heartbeat trackers (`sessions.json:last_heartbeat` vs `.vaak/sessions/*.json:last_alive_at_ms`) is a second, still live and exposed. Full audit in `.vaak/design-notes/multi-writer-audit-2026-05-13.md`. The recommended worked-example fix (consolidating dual heartbeat trackers) is the next architectural slice after v1.0 is observed in production. Human emphasized 2026-05-13: "don't fucking forget it."

## Deferred to v1.5 or later

- `pass-with-reason` action on [YOUR TURN] (silent stalling vs explicit pass).
- `responds_to` field on `contribute` (engagement-form enforcement).
- Rotating opener with head pointer (vs implicit closer-picks-next).
- Scratchpads with per-assembly lifecycle (off-mic productive thinking).
- Brick view summary UI (post-assembly synthesis surface).
- Generic Pending Decisions panel (consolidating blocking-on-human items).
- Silent-listen window after human directives (anti-pile-on).
- Expansion-before-reference gate (read-what-you-attack discipline).
- `proposal_assembly` message type (AI proposes; human approves).
- Work-mode floor budget (vs discussion-mode 60s).
- Status-message mic bypass (status-type messages should not be gated, observed 2026-05-13 during this session).
- Phase signaling (per spec at `.vaak/design-notes/phase-pill-spec-2026-05-13.md`, parked behind ≥1 live-assembly observation cycle of the activity field).
- Moderation tooling (`mic_skip`, `mic_redirect`, `speaker_warn`, `assembly_pause`, `assembly_resume`) — parked behind moderator:0 experiment to surface real friction.

## Non-negotiable constraints inherited from prior architect work

- Per human id 23 + id 39 (**UI is ground truth**): every silent failure mode in the current collab system is an instance of this principle being violated. The view-button silent-failure UX patch (commit `8f2b97a` + dist rebuild) and the regression fix `c43f917` both descend from this constraint.
- Per human id 729 + id 740 (**no conflation**): V2 design lives on `pr-pipeline-bundle`. Current-branch fixes must not import V2 concepts; V2 must not depend on modifying current-branch code.
- Per human 2026-05-13 (**fix here as foundation**): the v1.0 fix on this branch is intended as a stable substrate the team can use, and may inform whether V2 is needed at all — but doesn't itself constitute V2.
