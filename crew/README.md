# crew

Three things, in order. A spec written from what you actually said. A review by
a model that did not write the code and cannot see the conversation that
produced it. A gate that is code rather than an agent.

```
crew spec "what you want"   turn your request into numbered requirements
crew review                 review the diff against that spec
crew check                  decide whether the work is done
crew status                 say where this change stands
```

## Why it is shaped this way

A single session cannot review itself. Everything already in its context is a
premise rather than a hypothesis, so it defends the approach it committed to
fifty turns ago. Spawning a helper does not fix this either: the helper's whole
context is the brief the parent wrote, so the parent's blind spot is copied into
it, and a reviewer handed your conclusion tends to hand it back confirmed.

So the contamination vector is the brief, not the process boundary. Three rules
follow, and they are the whole design:

- **The session that wrote the code never writes the reviewer's prompt.**
  `review.build_brief` takes exactly two strings, the spec and the diff. Nothing
  else can reach the reviewer.
- **The reviewer reads the original request, not a restatement of it.** The spec
  is written before the work starts and is the one artifact that predates the
  drift.
- **Roles never talk to each other.** Each one reads files and writes files. The
  command line is the only thing that carries an artifact from one to the next.
  There is no message board, no queue, and no turn taking.

The gate is deliberately not a model. Whether a spec exists, whether every
requirement got a verdict, and whether the tests passed are all decidable by
reading files, and a model asked to decide them can be talked out of the answer.

## The three roles

**Interpreter.** Turns what you said into one paragraph of intent, numbered
requirements each judgeable as met or unmet, what is out of scope, and the
ambiguities that actually change the work with the assumption it would make for
each. It writes `.crew/spec.md` as a draft. You read it, fix what is wrong, and
change the status line to approved. Nothing proceeds until you do.

**Reviewer.** Reads the approved spec and the diff, and nothing else. Returns a
verdict on every requirement with evidence, plus objections with a severity.
Praise is forbidden by the stance, so an empty objection list means it found
nothing rather than that it was being polite.

Point it at a vendor that did not write the code:

```
crew review --provider openai
```

**Gate.** Fails when the spec is unapproved, a requirement has no verdict, a
requirement is unmet, a high severity objection stands, or the tests fail. Exits
non-zero so continuous integration can run it.

## Install

```
pip install -e ".[dev]"          # anthropic and pydantic
pip install -e ".[dev,openai]"   # to review across vendors
pytest
```

Set `ANTHROPIC_API_KEY`, or sign in with `ant auth login`. `OPENAI_API_KEY` is
needed only for the second provider. Override models with
`CREW_ANTHROPIC_MODEL` and `CREW_OPENAI_MODEL`.

## Configuration

Everything lives in `.crew/`: the spec, the last review, and an optional
`config.json` with one key.

```json
{ "test_command": "pytest" }
```

## Rules for changing this tool

Every addition removes or folds something. The state of a run must be sayable
aloud in under a minute, which is what `crew status` is for. Nothing ships
without a test that runs in continuous integration.

## The shared truth

Agents are transient. What persists is `.crew/truth.jsonl`, an append-only
log that every agent reads before working and appends to while working, so
the next agent starts where the last one ended. `.crew/truth.md` is a view
generated from it on every write; never edit the view by hand.

Four rules are code, enforced on every write:

- Entries carry provenance: author as `role@model`, time, and evidence.
- Nothing is overwritten. Retiring keeps history; status is derived.
- A contradiction is an objection about another entry. While it is live,
  the target is contested. Resolving it records whether it was upheld.
- Promotion to verified needs evidence.

```
export CREW_AS=implementer@claude-opus-5
crew truth add claim "The capture takes 40 ms on the Mail window." --evidence "commit 914407b"
crew truth add objection "Only on a small window." --about e1a2b3c --severity medium --as skeptic@gpt-5
crew truth resolve e4d5e6f --upheld --reason "Chrome measured at 900 ms."
crew truth verify e1a2b3c --evidence "timing test in CI"
crew truth add decision "Which recognizer goes first?" --option Groq --option OpenAI
crew truth add position Groq --about e7a8b9c --round 1 --confidence 0.7
crew truth add outcome Groq --about e7a8b9c --evidence "bake-off 2026-10-10"
crew truth check     # validates the whole log; exit 1 on any problem
crew truth view      # the readable truth
crew truth score     # the scoreboard
```

The scoreboard measures every author against outcomes, never approval:
accuracy of final positions, calibration as a Brier score, conformity flips
(right alone in round one, wrong after seeing others), productive updates
(the reverse), objections upheld over objections resolved, claims verified,
and tokens per useful contribution. It is computed from the log by code.
