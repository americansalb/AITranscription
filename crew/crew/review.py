"""The reviewer.

The reviewer is given exactly two things: the spec, written before the work
began, and the diff. It never sees the conversation that produced the work, and
it never sees the author's reasoning. Hand a reviewer your hypothesis and it
tends to hand the hypothesis back confirmed, so the brief is built here, by a
function that takes two strings and cannot be passed anything else.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

from pydantic import BaseModel, Field

from .spec import Spec

Verdict = Literal["met", "unmet", "unclear"]
Severity = Literal["high", "medium", "low"]


class RequirementVerdict(BaseModel):
    id: str = Field(description="The requirement id from the spec, such as R1.")
    verdict: Verdict = Field(
        description="met if the diff satisfies it, unmet if it does not, "
        "unclear if the diff does not contain enough to tell."
    )
    evidence: str = Field(
        description="The file and the change that supports this verdict, or what is missing."
    )


class Objection(BaseModel):
    severity: Severity
    claim: str = Field(description="One sentence stating what is wrong.")
    evidence: str = Field(description="The file, line, or condition that shows it.")


class Review(BaseModel):
    requirements: list[RequirementVerdict] = Field(default_factory=list)
    objections: list[Objection] = Field(default_factory=list)


REVIEWER_SYSTEM = """You are reviewing a change you did not write.

You have two things and nothing else: a specification written before the work \
began, and the diff. You do not have the author's reasoning, and you must not \
assume it was sound.

For every numbered requirement in the spec, decide whether the diff meets it. \
Cite the file and the change as evidence, or say what is missing. If the diff \
does not contain enough to tell, answer unclear rather than guessing. Return a \
verdict for every requirement, including ones the diff appears not to touch.

Then list objections. An objection is a concrete way this change is wrong or \
will fail, with evidence from the diff. Write no praise. Do not raise style \
preferences unless the spec names them. If the change is sound, return an empty \
list of objections rather than inventing one.

Severity is high when something is broken or will break, medium when it is \
likely wrong or will cause a problem later, and low when it is minor."""


@dataclass(frozen=True)
class Brief:
    """Everything the reviewer is allowed to see."""

    system: str
    user: str


def build_brief(spec_text: str, diff_text: str) -> Brief:
    """Build the reviewer's entire context from two strings.

    Nothing else can reach the reviewer. That is the point of the signature.
    """
    if not spec_text.strip():
        raise ValueError("Refusing to review without a spec.")
    if not diff_text.strip():
        raise ValueError("Refusing to review an empty diff.")

    user = (
        "SPECIFICATION, written before the work began:\n"
        "---\n"
        f"{spec_text.strip()}\n"
        "---\n\n"
        "DIFF of the work as it stands:\n"
        "---\n"
        f"{diff_text.strip()}\n"
        "---"
    )
    return Brief(system=REVIEWER_SYSTEM, user=user)


def missing_verdicts(spec: Spec, review: Review) -> list[str]:
    """Requirement ids the reviewer did not rule on."""
    ruled = {v.id for v in review.requirements}
    return [rid for rid in spec.requirement_ids if rid not in ruled]


def unmet(review: Review) -> list[RequirementVerdict]:
    return [v for v in review.requirements if v.verdict == "unmet"]


def unclear(review: Review) -> list[RequirementVerdict]:
    return [v for v in review.requirements if v.verdict == "unclear"]


def blocking_objections(review: Review) -> list[Objection]:
    return [o for o in review.objections if o.severity == "high"]


def spoken_summary(spec: Spec, review: Review) -> str:
    """One line a person can hear instead of reading the review."""
    total = len(spec.requirements)
    met = sum(1 for v in review.requirements if v.verdict == "met")
    high = len(blocking_objections(review))
    parts = [f"{met} of {total} requirements met"]
    if unclear(review):
        parts.append(f"{len(unclear(review))} unclear")
    if review.objections:
        parts.append(f"{len(review.objections)} objections, {high} high severity")
    else:
        parts.append("no objections")
    return ". ".join(parts) + "."
