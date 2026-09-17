"""The gate.

This is the role a person would call the manager: making sure each part of the
job actually happened. It is deliberately not an agent. "Did the spec get
written, did the review run, did every requirement get a verdict, did the tests
pass" are all decidable by reading files, and a model asked to decide them can
be talked out of the answer.

Pure functions. No network, no model, no persuasion.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .review import Review, blocking_objections, missing_verdicts, unmet
from .spec import Spec


@dataclass
class GateResult:
    failures: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return not self.failures

    def spoken_summary(self) -> str:
        if self.passed and not self.warnings:
            return "Gate passed. Every check is clear."
        if self.passed:
            return f"Gate passed with {len(self.warnings)} warnings."
        return f"Gate failed on {len(self.failures)} checks."


def evaluate(
    spec: Spec | None,
    review: Review | None,
    tests_passed: bool | None,
) -> GateResult:
    """Decide whether this change is done.

    `tests_passed` is None when no test command is configured, which is a
    warning rather than a pass.
    """
    result = GateResult()

    if spec is None:
        result.failures.append("No spec was written. Run crew spec first.")
        return result

    if not spec.is_approved:
        result.failures.append(
            f"The spec is marked {spec.status}, not approved. Read it and approve it."
        )

    if not spec.requirements:
        result.failures.append("The spec has no numbered requirements to check against.")

    if spec.assumptions:
        result.warnings.append(
            f"{len(spec.assumptions)} assumptions stand unanswered in the spec."
        )

    if review is None:
        result.failures.append("No review has been run against this spec.")
        return result

    absent = missing_verdicts(spec, review)
    if absent:
        result.failures.append(
            f"The reviewer gave no verdict on {', '.join(absent)}."
        )

    failed = unmet(review)
    if failed:
        result.failures.append(
            f"Requirements not met: {', '.join(v.id for v in failed)}."
        )

    blocking = blocking_objections(review)
    if blocking:
        result.failures.append(
            f"{len(blocking)} high severity objections are unresolved."
        )

    lesser = [o for o in review.objections if o.severity != "high"]
    if lesser:
        result.warnings.append(f"{len(lesser)} lower severity objections stand.")

    if tests_passed is False:
        result.failures.append("The test command failed.")
    elif tests_passed is None:
        result.warnings.append("No test command is configured, so nothing was run.")

    return result
