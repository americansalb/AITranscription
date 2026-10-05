---
name: deliberate
description: Convene the council on a real decision and record the result in the shared truth. Use when the user says "deliberate", "convene the council", "debate this", or asks which of several options to choose and the choice has stakes.
---

# Deliberate

The council decides a question in three rounds and writes the whole run into
the shared truth. You convene it; you do not sit on it, and you do not
summarize or soften its result.

## Steps

1. State the question in one sentence and list the options, each a short
   phrase. If the user gave no options, run without them and the seats will
   propose the ballot.
2. Run it from the repository root. The `crew` package lives in `crew/`:

   ```
   cd crew && python3 -m crew --root .. deliberate "QUESTION" --option "A" --option "B"
   ```

   Add `--seat role@provider` flags only if the user asked for particular
   seats. Otherwise the seats come from `.crew/council.json`, then from the
   model families with keys in the environment.
3. Read the output. Exit code 0 means a decision was adopted. Exit code 1
   means the council split: report the top options and the crux exactly as
   printed, and ask the user to decide. Never invent a consensus.
4. Report to the user, in this order: the decision or the split, the weighted
   agreement, each seat's final position with whether it moved, every
   objection and whether its holder upheld or rejected it, and any warning.
   Quote the council; do not add your own vote.
5. When the user later learns how the decision turned out, record it:

   ```
   cd crew && python3 -m crew --root .. truth add outcome "WHAT PROVED RIGHT" --about DECISION_ID --evidence "..." --as founder@human
   ```

   That is what turns the run into calibration numbers for every seat.

## Rules

- Before any work that touches a decision, read `.crew/truth.md` first.
- Write a claim, question, or task into the truth the moment you learn it:
  `python3 -m crew --root .. truth add claim "..." --evidence "..." --as ROLE@MODEL`
  with your role and model as the author.
- Never edit `.crew/truth.md` or `.crew/truth.jsonl` by hand. Only the
  commands write them.
- Three seats and three rounds is the default and the cost ceiling. Use
  `--rounds 1` for a quick blind vote when the stakes are low.
