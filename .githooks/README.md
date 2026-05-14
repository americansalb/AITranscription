# Vaak Two-Controls Pre-Commit Hook

Two-controls v1 structural floor for git commits (spec 2026-05-14, finding #8).
Reads `.vaak/active-section` + the active section's `protocol.json:floor` and
gates `git commit` per the planning/execution phase + accepted plan_hash + scope.

## Install (per developer per clone)

```sh
# Activate the .githooks directory project-locally:
git config core.hooksPath .githooks

# Make the hook executable in the index (NTFS has no Unix exec bit; git
# stores it as an index attribute that survives cross-platform clones):
git update-index --chmod=+x .githooks/pre-commit
```

That's it. No global state, no shared install. Each developer runs the two
commands once after their first clone.

## Requirements

- Python ≥3.x on PATH as `python3`. python.org installer ships both
  `python.exe` and `python3.exe` via the py-launcher shim. Microsoft Store
  Python may register `python.exe` only — install python.org Python or
  add a `python3` alias if the hook errors with `env: python3: not found`.

## What it gates

- **`phase == "planning"`**: rejects commit unconditionally with
  `[planning_blocks_commit]`. Call `protocol_mutate(action: "accept_plan", ...)`
  before committing.
- **`phase == "execution"` + `plan_hash` set**:
  - Rejects with `[plan_hash_mismatch]` if the plan file's SHA-256 doesn't
    match the stored hash. Call `protocol_mutate(action: "revise_plan", ...)`
    to re-bind (architect/manager/human only).
  - Rejects with `[staged_outside_scope]` if any staged file is outside the
    plan's `<!-- scope: ... -->` block. Use `<!-- scope: * -->` for
    unrestricted plans.
- **`phase == "execution"` + no plan**: allows the commit (back-compat path
  for sections that haven't accepted a plan yet).

## Bypass

`git commit --no-verify` skips the hook. NOTE per spec §103: standard git
does NOT record `--no-verify` in commit metadata, and post-commit hooks
also skip on bypass — there is no reliable post-hoc bypass detection.
A separate marker-file mechanism is deferred to v1.5.3.
