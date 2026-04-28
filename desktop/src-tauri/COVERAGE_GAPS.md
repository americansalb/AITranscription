# Test coverage gaps — Slice 2 (Assembly Line v6)

This file is the formal follow-on per tech-leader #941.5 alternative,
ratified by evil-architect #946 + dev-challenger #947. It documents
behavioral gaps that the apply-layer + atomicity smokes do NOT cover, the
reason the gap exists, and the work item that closes it.

## Slice 2 — `handle_protocol_mutate` CAS gate behavioral coverage

**Gap.** The CAS gate codes — `[StaleRev]`, `[MissingRev]` — fire from
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
