"""Command line entry point.

Three commands, in the order they run:

    crew spec "what you want"   write a spec from what you said
    crew review                 review the diff against that spec
    crew check                  decide whether the work is done

A council that decides a question in three rounds and writes the whole run
into the truth:

    crew deliberate "question" --option A --option B --seat skeptic@groq

And the shared truth, which every agent reads before working and appends to
while working:

    crew truth add claim "..." --as tester@claude-opus-5 --evidence ...
    crew truth verify <id> --evidence ...
    crew truth resolve <id> --upheld --reason ...
    crew truth retire <id> --superseded-by <id>
    crew truth check | view | score

Nothing here talks to anything except files and one provider at a time. Roles
never message each other; this module is the only thing that moves an artifact
from one to the next.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

import os

from . import deliberate as deliberate_mod, gate, interpret, providers, review as review_mod, spec as spec_mod, truth as truth_mod

CREW_DIR = ".crew"
SPEC_FILE = "spec.md"
REVIEW_FILE = "review.json"
CONFIG_FILE = "config.json"
TRUTH_FILE = "truth.jsonl"
TRUTH_VIEW = "truth.md"
COUNCIL_FILE = "council.json"
DELIBERATIONS_DIR = "deliberations"


def crew_dir(root: Path) -> Path:
    return root / CREW_DIR


def spec_path(root: Path) -> Path:
    return crew_dir(root) / SPEC_FILE


def review_path(root: Path) -> Path:
    return crew_dir(root) / REVIEW_FILE


def truth_path(root: Path) -> Path:
    return crew_dir(root) / TRUTH_FILE


def truth_view_path(root: Path) -> Path:
    return crew_dir(root) / TRUTH_VIEW


def load_config(root: Path) -> dict:
    path = crew_dir(root) / CONFIG_FILE
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def load_spec(root: Path) -> spec_mod.Spec | None:
    path = spec_path(root)
    if not path.exists():
        return None
    return spec_mod.parse(path.read_text(encoding="utf-8"))


def load_review(root: Path) -> review_mod.Review | None:
    path = review_path(root)
    if not path.exists():
        return None
    return review_mod.Review.model_validate_json(path.read_text(encoding="utf-8"))


def collect_diff(root: Path, base: str | None) -> str:
    """The work as it stands, as a stranger would read it."""
    commands = (
        [["git", "diff", f"{base}...HEAD"], ["git", "diff", base]]
        if base
        else [["git", "diff", "HEAD"], ["git", "diff", "--cached"]]
    )
    for command in commands:
        result = subprocess.run(command, cwd=root, capture_output=True, text=True)
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout
    return ""


def run_tests(root: Path, command: str | None) -> bool | None:
    if not command:
        return None
    print(f"Running tests: {command}")
    result = subprocess.run(command, cwd=root, shell=True)
    return result.returncode == 0


def cmd_spec(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    request = args.request
    if request == "-":
        request = sys.stdin.read()

    context = ""
    if args.context:
        context_path = Path(args.context)
        if not context_path.exists():
            print(f"Context file not found: {context_path}", file=sys.stderr)
            return 2
        context = context_path.read_text(encoding="utf-8")

    user = interpret.build_request(request, context)
    draft = providers.ask(
        args.provider, interpret.INTERPRETER_SYSTEM, user, interpret.DraftSpec
    )
    spec = interpret.to_spec(draft)

    target = spec_path(root)
    if target.exists() and not args.force:
        print(f"{target} already exists. Pass --force to overwrite it.", file=sys.stderr)
        return 2

    crew_dir(root).mkdir(parents=True, exist_ok=True)
    target.write_text(spec_mod.render(spec), encoding="utf-8")

    print(spec.spoken_summary())
    for assumption in spec.assumptions:
        print(f"  {assumption.id}. {assumption.text}")
    print()
    print(f"Written to {target}.")
    print("Read it, correct anything wrong, then change Status to approved.")
    return 0


def cmd_review(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()

    spec = load_spec(root)
    if spec is None:
        print("No spec found. Run crew spec first.", file=sys.stderr)
        return 2

    diff = collect_diff(root, args.base)
    if not diff.strip():
        print("No changes to review.", file=sys.stderr)
        return 2

    # The reviewer sees the spec as written and the diff. Nothing else.
    brief = review_mod.build_brief(spec_path(root).read_text(encoding="utf-8"), diff)
    result = providers.ask(args.provider, brief.system, brief.user, review_mod.Review)

    crew_dir(root).mkdir(parents=True, exist_ok=True)
    review_path(root).write_text(result.model_dump_json(indent=2), encoding="utf-8")

    print(f"Reviewed by {args.provider}.")
    print(review_mod.spoken_summary(spec, result))
    for verdict in result.requirements:
        if verdict.verdict != "met":
            print(f"  {verdict.id} {verdict.verdict}: {verdict.evidence}")
    for objection in result.objections:
        print(f"  {objection.severity}: {objection.claim}")
        print(f"    evidence: {objection.evidence}")
    print()
    print(f"Written to {review_path(root)}.")
    return 0


def cmd_check(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    config = load_config(root)

    spec = load_spec(root)
    review = load_review(root)
    command = args.tests if args.tests is not None else config.get("test_command")
    tests_passed = run_tests(root, command)

    result = gate.evaluate(spec, review, tests_passed)

    print(result.spoken_summary())
    for failure in result.failures:
        print(f"  failed: {failure}")
    for warning in result.warnings:
        print(f"  warning: {warning}")
    return 0 if result.passed else 1


def cmd_status(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    spec = load_spec(root)
    review = load_review(root)

    if spec is None:
        print("No spec. Nothing is being tracked here.")
        return 0

    print(spec.spoken_summary())
    if review is None:
        print("No review has been run yet.")
    else:
        print(review_mod.spoken_summary(spec, review))
    return 0


# -- the shared truth ----------------------------------------------------------


def author_from(args: argparse.Namespace) -> truth_mod.Author:
    text = getattr(args, "as_", None) or os.environ.get("CREW_AS", "")
    if not text and getattr(args, "command", "") == "deliberate":
        text = "council@crew"
    if not text:
        raise truth_mod.TruthError(
            "Say who is writing: --as role@model, or set CREW_AS in the environment."
        )
    return truth_mod.Author.parse(text)


def refresh_view(root: Path) -> None:
    """Regenerate the readable view after every successful write."""
    state, problems = truth_mod.load(truth_path(root))
    if not problems:
        truth_view_path(root).write_text(truth_mod.render_view(state), encoding="utf-8")


def cmd_truth_add(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    text = args.text
    if text == "-":
        text = sys.stdin.read().strip()
    author = author_from(args)
    entry = truth_mod.Entry(
        id=truth_mod.new_id(),
        kind=args.kind,
        text=text,
        author=author,
        time=truth_mod.now(),
        confidence=args.confidence,
        evidence=args.evidence or [],
        about=args.about,
        round=args.round,
        severity=args.severity,
        options=args.option or [],
        tokens=args.tokens,
        tags=args.tag or [],
    )
    event = truth_mod.Event(op="add", time=entry.time, author=author, entry=entry)
    truth_mod.append(truth_path(root), event)
    refresh_view(root)
    print(f"Added {entry.kind} {entry.id}.")
    return 0


def _truth_op(args: argparse.Namespace, op: str, **fields) -> int:
    root = Path(args.root).resolve()
    author = author_from(args)
    event = truth_mod.Event(op=op, time=truth_mod.now(), author=author, target=args.target, **fields)
    truth_mod.append(truth_path(root), event)
    refresh_view(root)
    state, _ = truth_mod.load(truth_path(root))
    print(f"{args.target} is now {state.status(args.target)}.")
    return 0


def cmd_truth_verify(args: argparse.Namespace) -> int:
    return _truth_op(args, "verify", evidence=args.evidence)


def cmd_truth_resolve(args: argparse.Namespace) -> int:
    upheld = True if args.upheld else False if args.rejected else None
    return _truth_op(args, "resolve", upheld=upheld, reason=args.reason)


def cmd_truth_retire(args: argparse.Namespace) -> int:
    return _truth_op(args, "retire", reason=args.reason, superseded_by=args.superseded_by)


def cmd_truth_check(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    state, problems = truth_mod.load(truth_path(root))
    print(truth_mod.spoken_summary(state, problems))
    for problem in problems:
        print(f"  {problem}")
    return 1 if problems else 0


def cmd_truth_view(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    state, problems = truth_mod.load(truth_path(root))
    if problems:
        print(truth_mod.spoken_summary(state, problems), file=sys.stderr)
        return 1
    view = truth_mod.render_view(state)
    if args.write:
        crew_dir(root).mkdir(parents=True, exist_ok=True)
        truth_view_path(root).write_text(view, encoding="utf-8")
        print(f"Written to {truth_view_path(root)}.")
    else:
        print(view, end="")
    return 0


def cmd_truth_score(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    state, problems = truth_mod.load(truth_path(root))
    if problems:
        print(truth_mod.spoken_summary(state, problems), file=sys.stderr)
        return 1
    print(truth_mod.render_scoreboard(truth_mod.scoreboard(state)), end="")
    return 0


# -- deliberation ------------------------------------------------------------------


def load_council(root: Path) -> dict:
    path = crew_dir(root) / COUNCIL_FILE
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def cmd_deliberate(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    council = load_council(root)
    specs = args.seat or council.get("seats") or []
    seats = [deliberate_mod.Seat.parse(s) for s in specs] or deliberate_mod.default_seats()
    custom_roles = council.get("roles") or {}

    context = ""
    if args.context:
        context_path = Path(args.context)
        if not context_path.exists():
            print(f"Context file not found: {context_path}", file=sys.stderr)
            return 2
        context = context_path.read_text(encoding="utf-8")

    state, problems = truth_mod.load(truth_path(root))
    if problems and not args.no_truth:
        print(truth_mod.spoken_summary(state, problems), file=sys.stderr)
        return 1
    if not args.no_truth and truth_view_path(root).exists():
        context += "\n\nThe shared truth so far:\n" + truth_view_path(root).read_text(encoding="utf-8")

    print("Seats: " + ", ".join(f"{s.role} on {s.provider} ({s.model_name})" for s in seats))
    if args.dry_run:
        system, user = deliberate_mod.round1_brief(seats[0], args.question, args.option or [], context, custom_roles)
        print("\nRound one brief for the first seat, system:\n" + system)
        print("\nUser:\n" + user)
        print("\nDry run. Nothing was asked and nothing was written.")
        return 0

    weights = {} if args.no_truth else deliberate_mod.weights_from_scoreboard(state)
    record = deliberate_mod.run(
        args.question, args.option or [], seats, context=context, rounds=args.rounds,
        threshold=args.threshold, weights=weights, custom_roles=custom_roles,
    )

    record_path = None
    if not args.no_truth:
        records = crew_dir(root) / DELIBERATIONS_DIR
        records.mkdir(parents=True, exist_ok=True)
        stamp = truth_mod.now().replace(":", "").replace("+0000", "Z")
        record_path = records / f"{stamp}-{truth_mod.new_id()}.json"
        deliberate_mod.record_to_truth(truth_path(root), record, author_from(args), record_path)
        record_path.write_text(record.to_json(), encoding="utf-8")
        refresh_view(root)

    print(record.spoken_summary())
    for s in record.seats:
        moved = "" if s.round3 is None or s.final_position == s.round1["position"] else f" (was {s.round1['position']})"
        print(f"  {s.label} {s.role} on {s.provider}: {s.final_position}, confidence {s.final_confidence:.0%}{moved}")
    for o in record.objections:
        answer = "unanswered" if o.accepted is None else ("upheld" if o.accepted else "rejected")
        print(f"  objection by {o.by_role} against {o.to_label} ({o.severity}, {answer}): {o.claim}")
    for warning in record.warnings:
        print(f"  warning: {warning}")
    if record.decision_id:
        print(f"Recorded as {record.decision_id}.")
    return 0 if record.decided else 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="crew",
        description="Write a spec from what you said, review the work against it, "
        "and gate on the result.",
    )
    parser.add_argument("--root", default=".", help="Project directory. Defaults to here.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    p_spec = subparsers.add_parser("spec", help="Turn a request into a numbered spec.")
    p_spec.add_argument("request", help="What you want, in your own words. Use - for stdin.")
    p_spec.add_argument("--context", help="A file of project vocabulary, for terms only.")
    p_spec.add_argument("--provider", default=providers.DEFAULT_PROVIDER,
                        choices=providers.available())
    p_spec.add_argument("--force", action="store_true", help="Overwrite an existing spec.")
    p_spec.set_defaults(func=cmd_spec)

    p_review = subparsers.add_parser("review", help="Review the diff against the spec.")
    p_review.add_argument("--provider", default=providers.DEFAULT_PROVIDER,
                          choices=providers.available(),
                          help="Use a provider that did not write the code.")
    p_review.add_argument("--base", help="Compare against this git ref instead of HEAD.")
    p_review.set_defaults(func=cmd_review)

    p_check = subparsers.add_parser("check", help="Gate on spec, review, and tests.")
    p_check.add_argument("--tests", help="Test command to run. Overrides the config file.")
    p_check.set_defaults(func=cmd_check)

    p_status = subparsers.add_parser("status", help="Say where this change stands.")
    p_status.set_defaults(func=cmd_status)

    p_delib = subparsers.add_parser("deliberate", help="Decide a question with a council, in three rounds.")
    p_delib.add_argument("question", help="The question, in one sentence.")
    p_delib.add_argument("--option", action="append", help="An option. Repeatable. Without options the seats propose the ballot.")
    p_delib.add_argument("--seat", action="append", help="role@provider or role@provider:model. Repeatable. Defaults to .crew/council.json, then one seat per model family with a key.")
    p_delib.add_argument("--context", help="A file every seat reads, identically.")
    p_delib.add_argument("--rounds", type=int, default=3, choices=[1, 2, 3], help="1 is a blind vote, 2 adds objections, 3 adds revision.")
    p_delib.add_argument("--threshold", type=float, default=deliberate_mod.DEFAULT_THRESHOLD, help="Weighted agreement needed to decide. Default two thirds.")
    p_delib.add_argument("--no-truth", action="store_true", help="Do not read or write the shared truth.")
    p_delib.add_argument("--dry-run", action="store_true", help="Show the seats and the first brief. Ask nothing, write nothing.")
    p_delib.add_argument("--as", dest="as_", help="Who convened the council, as role@model. Defaults to CREW_AS, then council@crew.")
    p_delib.set_defaults(func=cmd_deliberate)

    p_truth = subparsers.add_parser("truth", help="The shared truth every agent reads and appends to.")
    truth_sub = p_truth.add_subparsers(dest="truth_command", required=True)

    def who(p: argparse.ArgumentParser) -> None:
        p.add_argument("--as", dest="as_", help="Who is writing, as role@model. Defaults to CREW_AS.")

    p_add = truth_sub.add_parser("add", help="Append an entry.")
    p_add.add_argument("kind", choices=["claim", "decision", "question", "task", "objection", "position", "outcome"])
    p_add.add_argument("text", help="The entry, in one sentence. Use - for stdin.")
    who(p_add)
    p_add.add_argument("--confidence", type=float, help="0 to 1. How sure the author is.")
    p_add.add_argument("--evidence", nargs="*", help="Commits, tests, files, sources.")
    p_add.add_argument("--about", help="The entry this is about. Required for objection, position, outcome.")
    p_add.add_argument("--round", type=int, help="Deliberation round, for positions.")
    p_add.add_argument("--severity", choices=["high", "medium", "low"], help="For objections.")
    p_add.add_argument("--option", action="append", help="An option, for decisions. Repeatable.")
    p_add.add_argument("--tokens", type=int, help="Tokens spent producing this entry.")
    p_add.add_argument("--tag", action="append", help="A tag. Repeatable.")
    p_add.set_defaults(func=cmd_truth_add)

    p_verify = truth_sub.add_parser("verify", help="Promote a claim or decision to verified. Needs evidence.")
    p_verify.add_argument("target")
    p_verify.add_argument("--evidence", nargs="+", required=True)
    who(p_verify)
    p_verify.set_defaults(func=cmd_truth_verify)

    p_resolve = truth_sub.add_parser("resolve", help="Close an objection, question, or task.")
    p_resolve.add_argument("target")
    outcome = p_resolve.add_mutually_exclusive_group()
    outcome.add_argument("--upheld", action="store_true", help="The objection stood.")
    outcome.add_argument("--rejected", action="store_true", help="The objection did not stand.")
    p_resolve.add_argument("--reason", required=True)
    who(p_resolve)
    p_resolve.set_defaults(func=cmd_truth_resolve)

    p_retire = truth_sub.add_parser("retire", help="Move an entry to history. Nothing is deleted.")
    p_retire.add_argument("target")
    p_retire.add_argument("--superseded-by", dest="superseded_by", help="The entry that replaces it.")
    p_retire.add_argument("--reason")
    who(p_retire)
    p_retire.set_defaults(func=cmd_truth_retire)

    p_tcheck = truth_sub.add_parser("check", help="Validate the whole log.")
    p_tcheck.set_defaults(func=cmd_truth_check)

    p_tview = truth_sub.add_parser("view", help="Render the readable truth.")
    p_tview.add_argument("--write", action="store_true", help="Write .crew/truth.md instead of printing.")
    p_tview.set_defaults(func=cmd_truth_view)

    p_tscore = truth_sub.add_parser("score", help="Measure every author against outcomes.")
    p_tscore.set_defaults(func=cmd_truth_score)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except (providers.ProviderError, spec_mod.SpecError, truth_mod.TruthError, deliberate_mod.DeliberationError, ValueError) as exc:
        print(f"{exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
