# RISHI

Working name. It is written in exactly two places: the file `PRODUCT_NAME` at
the repository root, which code reads at compile time, and this heading. To
rename the product, change those two. No crate, file, identifier, or message
contains the name, and a test fails if it ever does.

## Talk. It does the work. It tells you what it did.

You speak. It works, or Claude Code works. It tells you what happened and what
is on the screen, in a voice of your choice. You never look.

Version one has no window. The interface is a menu bar menu, one key, and
speech. Everything is set by voice, every state can be said in one sentence,
and every error is a sentence too.

## What is here

- `crates/logic` is pure logic: no operating system, no network, no clock.
  The screen schema, the spoken sentence builder, and the tree-to-text
  renderer. Every test in it runs on Linux, against fixtures checked into
  `crates/logic/tests/fixtures`.
- `crates/platform` is the only place operating-system code lives. It reads
  the accessibility tree and tracks keyboard focus on macOS and Windows, one
  implementation each, behind one interface. Linux is an honest stub that says
  so aloud.

Nothing here is ported from the old code. The branches `legacy`,
`feature/strict-turn-discipline`, and `dev-local` hold the previous codebase
and are reference material only: read them for system call details, never
copy from them.

## Rules that are code

Each of these is a test that fails when broken, not a sentence someone has to
remember.

- A secure field never carries a value, and no text ever shows one.
- The product name is data. It appears nowhere in code.
- Every platform error has a spoken sentence.
- The same tests run on macOS, Windows, and Linux on every push. Parity is a
  gate, not a goal.

## The plan

Six milestones. Each ends with something you can hear or use, each is green
on all three runners before the next starts, and each ships with a recorded
transcript of a real session.

1. Fresh foundations: this schema, this platform layer, these tests.
2. Speech out: system voices, a streaming sentence splitter, markdown to
   speech with a fixture corpus of real Claude Code output, a say command, and
   the Claude Code Stop hook it writes for itself.
3. Talks first: the app binary, the menu, a spoken hello within one second of
   launch, lazy spoken permission flow, the "are you working" self-check, and
   an error catalog where every error speaks.
4. Look: capture on both platforms into this schema, the direct model client
   with streaming, the key from the clipboard into the keychain, "what is on
   my screen" with follow-up questions, screenshots only on request.
5. Voice in: one talk key that needs no permission, system speech recognition
   with vocabulary from the screen, echo control, end-of-speech detection,
   screen reader detection and the two modes.
6. Act, then ship: computer use with narrate-and-confirm, input abort, modal
   detection, step limits; then signing, installer, auto-update, a weekly live
   canary, Homebrew and winget.

## Building

The code is a Cargo workspace. `cargo test` at the root runs every test on the
platform you are on, and continuous integration runs the same on macOS,
Windows, and Linux for every push. Reading a real screen needs the
Accessibility permission on macOS and nothing on Windows.
