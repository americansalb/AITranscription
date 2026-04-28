# Test coverage gaps — Slice 2 (Assembly Line v6)

This file is the formal follow-on per tech-leader #941.5 alternative,
ratified by evil-architect #946 + dev-challenger #947. It documents
behavioral gaps that the apply-layer + atomicity smokes do NOT cover, the
reason the gap exists, and the work item that closes it.

## Slice 2 — `handle_protocol_mutate` CAS gate behavioral coverage

**STATUS: CLOSED IN `3b8735a` (developer:0, board #950).** Dev:0 chose
the second option from this doc — extracted `do_protocol_mutate` as a
pure function over `(project_dir, section, action, args, rev_in)` and
added 7 CAS-gate behavioral tests against it. Doc retained as audit
record of what was deferred and how it was closed.

**Original gap (closed).** The CAS gate codes — `[StaleRev]`, `[MissingRev]` — fire from
`handle_protocol_mutate` (vaak-mcp.rs `fn handle_protocol_mutate`). The
gate is verified at the apply layer through unit tests (see
`protocol_slice2_tests::*` in vaak-mcp.rs at line ~3477+) but no test
exercises the wrapper's full lock + read + CAS + dispatch round-trip
because the wrapper depends on `get_or_rejoin_state` for project-dir
resolution. That helper is hard to mock without a refactor.

**Why deferred (not fixed in Slice 2).** Refactoring
`get_or_rejoin_state` to be mockable is itself a non-trivial change that
crosses the MCP transport boundary. Doing it inside Slice 2 would balloon
scope and delay Slice 3 (panel UI) without adding a missing correctness
property — the apply-layer code paths that compute the gate result ARE
covered, only the wrapper plumbing isn't.

**What closes it.** Tester:0's property-test PR (board reference #928,
follow-on to dev #927 testing-plan vote (e)). That PR adds either:

- a `protocol-property.test.mjs` integration harness that drives the
  full MCP round-trip via stdio JSON-RPC against a built `vaak-mcp.exe`
  in a tempdir project, OR
- a refactor that extracts `handle_protocol_mutate`'s body into a pure
  function over `(project_dir, section, action, args, rev_in)` so it can
  be unit-tested without `get_or_rejoin_state`.

Tester chooses approach. Either is acceptable to close this gap.

**Risk while gap is open.** Low. The CAS arithmetic is shared with the
tested apply layer (same `current.rev` u64 read, same `expected_rev` u64
arg). A regression in the wrapper that bypasses the gate would have to
diverge from the apply path's contract — visible in code review and at
the integration boundary against any caller that does
`get_protocol → mutate(rev)`.

**Pre-Slice-3 commitment (per #946 + #947 + tech-leader ratification).**
This document lands on `feature/al-vision-slice-1` BEFORE Slice 3 forks
or commits. Slice 3 is unblocked once this file is at origin. The
property-test PR is not blocking Slice 3 implementation — only blocking
Slice 6 (deprecation of legacy `assembly_line` / `discussion_control`
MCP tools) where the legacy compat round-trip becomes load-bearing.

---

# v6 vision MVP — deliberate-scope cuts (per architect #1013)

The v6 vision is shipped as MVP at branch tip `628d886`. Five items
were deliberately scoped out of MVP and are documented here as
auditable follow-ons (NOT silent cuts — each is named, reasoned, and
has a closer PR identified).

## Gap A — Oxford / Delphi / Red-Team preset mapping

**Surface.** `discussion_control` thin-wrap at `vaak-mcp.rs::handle_discussion_control`
routes ONLY `mode == "continuous"` through `protocol_mutate(open_round,
{topic, mode:"tally"})`. Modes `delphi`, `oxford`, `red_team` keep the
legacy state machine path because:

- Delphi has a blind-submit gate (vaak-mcp.rs ~L3786 `disc_active &&
  disc_format == "delphi"` enforcement in `handle_project_send`) whose
  semantics aren't captured by `consensus.mode = "vote"`.
- Oxford has explicit team assignment (`teams: {for: [...], against: [...]}`)
  with no equivalent in the protocol.json schema.
- Red-Team has a structured pre/post-vote tally with opinion-shift
  reporting (search_oxford_outputs in vaak-mcp.rs uses this).

**Why deferred.** Mapping these three modes is mechanical against the
spec §6 matrix: Delphi=(round-robin, vote), Oxford=(queue, vote),
Red-Team=(queue, vote w/ pre/post phases). But the legacy state machine
has features (team assignment, blind-submit gate, opinion-shift tally)
that need explicit schema fields beyond what `consensus.round` carries
today. Adding those fields = real Slice 7 design work, not a thin-wrap.

**What closes it.** Slice 7 — `architect:0` to draft schema additions
(consensus.teams, consensus.blind_submit_gate, consensus.pre_post_tally)
and route the three modes through `protocol_mutate`. Branch policy:
forks from `feature/al-vision-slice-1` after MVP merge.

**Risk while open.** Bounded — Oxford/Delphi/RedTeam invocations
silently keep writing to `.vaak/discussion.json`. That's identical to
the assembly_line dual-write architecture pre-`628d886` (legacy file
written, no protocol.json mirror), and no production code reads
`.vaak/discussion.json` after Slice 1 migration. Visible in get_state
projection: when delphi/oxford runs, `_via` field reads "discussion.json"
instead of "protocol.json" — drift detection signal.

## Gap B — PhaseRow React unit tests missing

**Surface.** `desktop/src/components/ProtocolPanel/ProtocolPanel.tsx`
PhaseRow component (Slice 5 wires) has zero unit tests. SeatChip (R2)
got 9 cases; PhaseRow got 0 despite ⏸/⏭/⏲ button click handlers,
disabled-when-atEnd states, and pause-pill rendering branching.

**Why deferred.** Identified by dev-challenger:0 #980 finding 3 as a
follow-on. Caught at the same point in the cycle as the SeatChip tests
were drafted, but folded into Slice 5 commit b4ae47e for ship momentum.
Coverage is real but not load-bearing — the click handlers go through
`mutate(action, args)` which is itself tested at the dispatch layer.

**What closes it.** Follow-on commit on `feature/al-vision-slice-1`:
`PhaseRow.test.tsx` adding ~6 cases (disabled-when-atEnd, pause toggle
correctly flips between pause/resume label, click fires expected mutate
action with right args, +15m extension button passes secs=900). Owner:
developer:0 or first React-capable seat that lands.

**Risk while open.** Low. Click handlers are 1-line dispatches;
regression would manifest immediately on UI use.

## Gap C — Auto-advance scheduler React-side observation tests

**Surface.** `auto_advance_if_outcome_met` (vaak-mcp.rs::Slice 6) fires
inside handle_get_protocol's `with_file_lock` window when a phase
predicate evaluates true. Backend has 0 explicit tests for this
auto-fire path (timer-based outcome predicates would need clock
manipulation).

**Why deferred.** Tester:0's `protocol-property.test.mjs` harness
(.vaak/tests/, board #937) is positioned to run the full MCP round-trip
against a built binary; that's the natural place for time-based outcome
tests because the harness controls the clock at the integration boundary.
Adding clock-injection at the apply layer would require refactoring
`evaluate_phase_outcome` to take a `now_secs` parameter, which is a
mechanical but real refactor.

**What closes it.** Tester's property-test PR (board #928 + #937).
Acceptable alternative: refactor `evaluate_phase_outcome` to accept
`now_secs: u64` parameter, default to `SystemTime::now()` at call
sites; add 3 unit tests covering timer-not-elapsed / timer-elapsed-
no-extension / timer-elapsed-with-extension.

**Risk while open.** Low. The predicate math is reused from
`apply_resume_plan`'s same arithmetic (epoch-secs subtraction with
saturating math), which is partially tested via the
`dispatch_pause_resume_accumulates_paused_secs` test.

## Gap D — Multilingual `Mic to ROLE` detector

**Surface.** `composer/micToDetector.ts::detectMicTo` regex is
English-only (spec §8 "Architectural limitations to acknowledge").
French "Micro à architecte", Spanish "Micrófono a arquitecto", etc.
do not match.

**Why deferred.** Spec §8 explicitly flags this as a known-limitation,
NOT a bug. v1 ships a locale-keyed regex registry; v0 (this MVP) ships
the English regex with the dropdown affordance as universal fallback.
The MicToHint UI's ambiguous-pick affordance is the multilingual
escape hatch.

**What closes it.** v1 follow-on: `micToDetector.ts` accepts a `locale`
parameter; regex registry keyed by BCP47 language tag. Owner: future
ux-engineer or i18n-capable seat.

**Risk while open.** Cosmetic — multilingual users' bare-text mic_to
mentions don't auto-suggest, but the dropdown picker still works.

## Gap E — Legacy MCP tool entry-point removal

**Surface.** `assembly_line` and `discussion_control` MCP tools are
still REGISTERED (vaak-mcp.rs tool list). Their bodies are now
thin-wrappers (post-`628d886`) but the tool names + signatures remain
callable, with deprecation eprintln warnings.

**Why deferred.** Spec §3.3 "Backward compat tail: legacy ... MCP tools
stay live for one release." Removing the entry points NOW would break
any caller that hasn't migrated. The compat tail is intentional.

**What closes it.** Release-after-MVP cycle. Drop the two `match
tool_name == "..."` branches from `handle_request`; drop the two tool
definitions from the registration JSON list. ~20 LOC delta.

**Risk while open.** None — deprecation is operator-visible via
eprintln; drift is bounded because the bodies route through
`protocol_mutate`.

---

**Closer schedule (board references):**
- Gap A: Slice 7, owner architect:0 (schema design) + developer:0 (impl)
- Gap B: follow-on commit on this branch, owner developer:0
- Gap C: tester:0 property-test PR (board #928/#937)
- Gap D: v1 i18n cycle, owner future seat
- Gap E: release-after-MVP cycle, owner developer:0 or maintainer
