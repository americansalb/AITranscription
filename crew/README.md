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
