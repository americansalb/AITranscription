"""Command line entry point.

Three commands, in the order they run:

    crew spec "what you want"   write a spec from what you said
    crew review                 review the diff against that spec
    crew check                  decide whether the work is done

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

from . import gate, interpret, providers, review as review_mod, spec as spec_mod

CREW_DIR = ".crew"
SPEC_FILE = "spec.md"
REVIEW_FILE = "review.json"
CONFIG_FILE = "config.json"


def crew_dir(root: Path) -> Path:
    return root / CREW_DIR


def spec_path(root: Path) -> Path:
    return crew_dir(root) / SPEC_FILE


def review_path(root: Path) -> Path:
    return crew_dir(root) / REVIEW_FILE


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

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except (providers.ProviderError, spec_mod.SpecError, ValueError) as exc:
        print(f"{exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
