"""The spec file format.

A spec is the artifact that everything else reads. It is written before the
work starts, from what the human said, and it is the only thing the reviewer
sees besides the diff.

This module is pure: it parses and renders text and makes no network calls, so
it can be tested without an API key.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

_TITLE_RE = re.compile(r"^#\s+Spec:\s*(.+?)\s*$")
_STATUS_RE = re.compile(r"^Status:\s*(\S+)\s*$", re.IGNORECASE)
_HEADING_RE = re.compile(r"^##\s+(.+?)\s*$")
_REQUIREMENT_RE = re.compile(r"^(R\d+)\.\s+(.+?)\s*$")
_ASSUMPTION_RE = re.compile(r"^(A\d+)\.\s+(.+?)\s*$")
_BULLET_RE = re.compile(r"^-\s+(.+?)\s*$")

APPROVED = "approved"
DRAFT = "draft"


class SpecError(ValueError):
    """The spec text could not be parsed, or is not usable as written."""


@dataclass(frozen=True)
class Requirement:
    id: str
    text: str


@dataclass(frozen=True)
class Assumption:
    id: str
    text: str


@dataclass
class Spec:
    title: str
    intent: str = ""
    requirements: list[Requirement] = field(default_factory=list)
    out_of_scope: list[str] = field(default_factory=list)
    assumptions: list[Assumption] = field(default_factory=list)
    status: str = DRAFT

    @property
    def is_approved(self) -> bool:
        return self.status.strip().lower() == APPROVED

    @property
    def requirement_ids(self) -> list[str]:
        return [r.id for r in self.requirements]

    def spoken_summary(self) -> str:
        """One line a person can hear instead of reading the file."""
        return (
            f"Spec {self.title}. Status {self.status}. "
            f"{len(self.requirements)} requirements, "
            f"{len(self.assumptions)} assumptions, "
            f"{len(self.out_of_scope)} items out of scope."
        )


def _split_sections(text: str) -> tuple[str, str, dict[str, list[str]]]:
    """Return (title, status, {heading_lower: [lines]})."""
    title = ""
    status = DRAFT
    sections: dict[str, list[str]] = {}
    current: str | None = None

    for raw in text.splitlines():
        line = raw.rstrip()

        title_match = _TITLE_RE.match(line)
        if title_match and not title:
            title = title_match.group(1)
            continue

        if current is None:
            status_match = _STATUS_RE.match(line)
            if status_match:
                status = status_match.group(1)
                continue

        heading_match = _HEADING_RE.match(line)
        if heading_match:
            current = heading_match.group(1).strip().lower()
            sections.setdefault(current, [])
            continue

        if current is not None:
            sections[current].append(line)

    return title, status, sections


def parse(text: str) -> Spec:
    """Parse spec markdown. Raises SpecError when the shape is wrong."""
    if not text.strip():
        raise SpecError("Spec is empty.")

    title, status, sections = _split_sections(text)
    if not title:
        raise SpecError("Spec has no '# Spec: <title>' heading.")

    intent = "\n".join(sections.get("intent", [])).strip()

    requirements: list[Requirement] = []
    seen_ids: set[str] = set()
    for line in sections.get("requirements", []):
        match = _REQUIREMENT_RE.match(line.strip())
        if not match:
            continue
        req_id, req_text = match.group(1), match.group(2)
        if req_id in seen_ids:
            raise SpecError(f"Duplicate requirement id {req_id}.")
        seen_ids.add(req_id)
        requirements.append(Requirement(req_id, req_text))

    out_of_scope = [
        match.group(1)
        for line in sections.get("out of scope", [])
        if (match := _BULLET_RE.match(line.strip()))
    ]

    assumptions: list[Assumption] = []
    seen_assumptions: set[str] = set()
    for line in sections.get("assumptions", []):
        match = _ASSUMPTION_RE.match(line.strip())
        if not match:
            continue
        a_id, a_text = match.group(1), match.group(2)
        if a_id in seen_assumptions:
            raise SpecError(f"Duplicate assumption id {a_id}.")
        seen_assumptions.add(a_id)
        assumptions.append(Assumption(a_id, a_text))

    return Spec(
        title=title,
        intent=intent,
        requirements=requirements,
        out_of_scope=out_of_scope,
        assumptions=assumptions,
        status=status,
    )


def render(spec: Spec) -> str:
    """Render a spec back to markdown. render(parse(x)) is stable."""
    lines = [f"# Spec: {spec.title}", "", f"Status: {spec.status}", ""]

    lines += ["## Intent", ""]
    lines += [spec.intent.strip() or "(not stated)", ""]

    lines += ["## Requirements", ""]
    if spec.requirements:
        lines += [f"{r.id}. {r.text}" for r in spec.requirements]
    else:
        lines.append("(none)")
    lines.append("")

    lines += ["## Out of scope", ""]
    if spec.out_of_scope:
        lines += [f"- {item}" for item in spec.out_of_scope]
    else:
        lines.append("- (nothing stated)")
    lines.append("")

    lines += ["## Assumptions", ""]
    if spec.assumptions:
        lines += [f"{a.id}. {a.text}" for a in spec.assumptions]
    else:
        lines.append("(none)")
    lines.append("")

    return "\n".join(lines)


def number_requirements(texts: list[str]) -> list[Requirement]:
    """Number non-empty entries contiguously, so R2 always follows R1."""
    kept = [t.strip() for t in texts if t.strip()]
    return [Requirement(f"R{i}", t) for i, t in enumerate(kept, start=1)]


def number_assumptions(texts: list[str]) -> list[Assumption]:
    """Number non-empty entries contiguously, so A2 always follows A1."""
    kept = [t.strip() for t in texts if t.strip()]
    return [Assumption(f"A{i}", t) for i, t in enumerate(kept, start=1)]
