# Talk. It does the work. It tells you what it did.

You speak. Claude Code does the work. It opens the result and tells you what it did and what it sees, in a voice of your choice. You never look.

This branch is the new home for that product. It starts empty on purpose.

## Branches

- `main` is this branch: the new product.
- `legacy` is a snapshot of the old `main` branch. It is **not** the full codebase.
- The most recent collaboration work is **not on main or legacy**. It lives on
  `feature/strict-turn-discipline`, which is 603 commits beyond main and runs
  through June 2026, and on `feature/al-vision-slice-1`. Do not delete those
  branches on the assumption that `legacy` holds everything.

Nothing on this branch imports from `legacy`. Pieces are ported deliberately, one at a time, with their tests.

## Ported from legacy, in this order

1. Accessibility tree capture and focus tracking, from `desktop/src-tauri/src/a11y`
2. Screen description, screen chat, and the computer-use loop, from `backend/app/services/screen_reader.py`
3. Terminal-process session id, from `mcp-speak`
4. Text-to-speech callers
5. Paste-into-focused-window dictation
6. macOS permission wizard
7. Voice rules and the detail scale, already in `CLAUDE.md`

Everything else stays on `legacy`.

## Building

The code is a Cargo workspace. `cargo test` at the root runs every test on the
platform you are on, and continuous integration runs the same on macOS,
Windows, and Linux for every push. `crates/platform` is the only place
platform-specific code lives; nothing outside it may name a platform API.
