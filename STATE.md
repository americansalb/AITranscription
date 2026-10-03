# State

Read this file before anything else, in every session. It is the durable
memory of this project. Conversations get compacted and lost; this file does
not. Every decision is written here in the same turn it is made, with the
date. When memory and this file disagree, this file wins.

## What this is

An eyes-free assistant. You speak, it works or Claude Code works, and it
tells you what happened and what is on the screen in a voice of your choice.
Three users in order of certainty: the founder with Claude Code, blind
knowledge workers, and sighted people with their eyes elsewhere. The working
name is in the file PRODUCT_NAME and nowhere else in code.

## Decided

- 2026-09-25. Fresh start. Nothing is ported from the old code. The branches
  `legacy`, `feature/strict-turn-discipline`, and `dev-local` are reference
  material only.
- 2026-09-25. Version one has no window. Menu bar menu, one key, speech.
  Every setting by voice. No Tauri, no webview, no Node. Pure Rust.
- 2026-09-25. Two crates: `logic` (pure, tested on Linux, holds the screen
  schema, sentence builder, renderer, product name) and `platform` (the only
  place operating-system code lives). Same tests on macOS, Windows, Linux on
  every push. Parity is a gate.
- 2026-09-25. Rules that are tests: secure fields never carry a value; the
  product name appears nowhere in code; every platform error has a spoken
  sentence.
- 2026-09-25. Six milestones: 1 foundations (done), 2 speech out with the
  Claude Code Stop hook, 3 the app binary that talks within one second and
  the spoken permission flow, 4 look (capture on both platforms, direct
  model client, key from clipboard to keychain), 5 voice in, 6 act then ship.
- 2026-09-25. Speech recognition: not the system recognizers (parity and
  accuracy). Groq whisper-large-v3-turbo first ($0.04 per hour, ten second
  minimum billing, prompt of up to 224 tokens for screen vocabulary), OpenAI
  gpt-4o-transcribe second, a local model third for offline use. The model
  corrects transcripts using names visible on the screen. A corpus of the
  founder's own recordings becomes a permanent regression test.
- 2026-09-25. Speech output tiers: system voice for narration, focus, long
  reading, and errors (free, instant, offline, fast rates); Kokoro on the
  device as the free real voice for replies on both platforms (milestone 3);
  ElevenLabs Flash streaming as the paid premium; if a premium voice has not
  started within 700 ms the system voice takes the sentence silently.
  ElevenLabs Conversational AI is not used (about $0.08 a minute, roughly
  $89 per million characters). Prices checked 2026-09-25: ElevenLabs Flash
  $50 per million characters, Deepgram Aura-2 $30, Inworld TTS-2 $25 falling
  to $5 at volume, OpenAI tts-1 $15, Inworld Flash $15.
- 2026-09-25. Keys never enter the repository. The founder's Anthropic key is
  on their machine. A scan of all 1,115 commits found no committed key.
- 2026-09-25. The founder has a Windows machine, so real Windows capture is
  tested from milestone 4 on.
- 2026-09-25. Claude Code is used in the terminal, the desktop app, and the
  browser. The Stop hook covers the first two. Browser sessions run in the
  cloud and cannot speak on the Mac; that gap is stated, not hidden.
- 2026-09-25. The repository is public. The founder was told. No decision yet.

## Open

- Can the founder see the screen, and what reads it to them today? They had
  not heard of VoiceOver, which suggests they are not a screen reader user.
  This changes who tests the first minute, not what is built. Blind testers
  are needed early either way.
- Which macOS version and chip. Decides whether a local model can be the
  default on the founder's own Mac.
- Apple Developer Program enrollment. Unsigned builds lose the Accessibility
  permission on every rebuild.
- Access to the `americansalb/learn` repository, which holds Gujarati and
  Kurdish voice models the founder trained. Attach was blocked by the session
  permission gate on 2026-10-03.
- Company name, whether to register domains, whether to rename the `crew`
  tool, whether to make the repository private.

## Requested 2026-10-03, not yet decided

The founder is tired of talking to one agent at a time and losing everything
to compaction. They want the multi-agent deliberation idea rebuilt, better:
agents with roles working in parallel, decisions made from structured debate
with a record. Open: whether it lives inside Claude Code (workflows,
subagents, files in git) or as a standalone app; which models sit on the
council. This file is the first piece of the answer to compaction.

## Where things are

- `new-main`: this product. Green CI on three runners.
- `main` = `legacy`: the old codebase. Render deploys from the default
  branch, so `main` is not replaced until Render points at `legacy`.
- `feature/strict-turn-discipline`: the newest old work, 603 commits ahead
  of main, including the old deliberation system, the Delphi gate, and the
  turn-gate hook. `dev-local` has 130 more commits. Do not delete either.
- `claude/sleepy-goldberg-9s5ztv`: the `crew` tool (spec, independent
  review, gate as code), 29 tests.

## How to resume

1. Read this file.
2. `git log --oneline -15 new-main` for what landed since.
3. `cargo test --workspace` must be green before any new work.
4. Next step is milestone 2 unless this file says otherwise.
