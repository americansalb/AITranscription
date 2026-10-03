"""The shared truth.

Agents are transient; each lives in one context window and dies. What
persists is this log: facts an agent verified, decisions and why, questions
still open, objections that stood. Every agent reads the view before it works
and appends to the log as it works, so the next agent starts where the last
one ended, and the whole comes to hold knowledge no single agent was given.

Shared truths rot unless something keeps them true, so four rules are code:

- Entries carry provenance: who, which model, when, and what evidence.
- Nothing is overwritten. The log is append-only events; status is derived.
- A contradiction is an objection entry about another entry. While it is
  live, the target is contested. Resolving it records whether it was upheld.
- Promotion to verified needs evidence. The validator refuses every write
  that breaks a rule, so the log is always consistent.

The scoreboard measures every author, meaning a role on a model, against
outcomes rather than approval: accuracy, calibration, conformity flips,
objection yield, and cost per useful contribution.

Pure functions over a file. No network, no model.
"""

from __future__ import annotations

import json
import secrets
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field, ValidationError

Kind = Literal["claim", "decision", "question", "task", "objection", "position", "outcome"]
Op = Literal["add", "verify", "resolve", "retire"]
Severity = Literal["high", "medium", "low"]

KINDS_WITH_TARGET = ("objection", "position", "outcome")
VERIFIABLE = ("claim", "decision")
RESOLVABLE = ("objection", "question", "task")


class TruthError(ValueError):
    """A write that would make the log inconsistent."""


class Author(BaseModel):
    role: str = Field(min_length=1)
    model: str = Field(min_length=1)

    @property
    def key(self) -> str:
        return f"{self.role}@{self.model}"

    @classmethod
    def parse(cls, text: str) -> "Author":
        """'reviewer@claude-opus-5' or 'kevin@human'."""
        role, sep, model = text.partition("@")
        if not sep or not role.strip() or not model.strip():
            raise TruthError(f"An author is written role@model, not {text!r}.")
        return cls(role=role.strip(), model=model.strip())


class Entry(BaseModel):
    id: str = Field(min_length=1)
    kind: Kind
    text: str = Field(min_length=1)
    author: Author
    time: str
    confidence: float | None = Field(default=None, ge=0.0, le=1.0)
    evidence: list[str] = Field(default_factory=list)
    about: str | None = None
    round: int | None = Field(default=None, ge=1)
    severity: Severity | None = None
    options: list[str] = Field(default_factory=list)
    tokens: int | None = Field(default=None, ge=0)
    tags: list[str] = Field(default_factory=list)


class Event(BaseModel):
    op: Op
    time: str
    author: Author
    entry: Entry | None = None
    target: str | None = None
    evidence: list[str] = Field(default_factory=list)
    upheld: bool | None = None
    reason: str | None = None
    superseded_by: str | None = None


def now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def new_id() -> str:
    return "e" + secrets.token_hex(3)


@dataclass
class Record:
    """The current state of one entry, folded from the events about it."""

    entry: Entry
    verified: bool = False
    verify_evidence: list[str] = field(default_factory=list)
    resolved: bool = False
    upheld: bool | None = None
    resolution: str | None = None
    retired: bool = False
    superseded_by: str | None = None
    objections: list[str] = field(default_factory=list)


class State:
    """The fold of a log: every entry's current state, and the rules."""

    def __init__(self) -> None:
        self.records: dict[str, Record] = {}
        self.order: list[str] = []

    # -- reading ---------------------------------------------------------

    def get(self, entry_id: str) -> Record:
        try:
            return self.records[entry_id]
        except KeyError:
            raise TruthError(f"No entry {entry_id}.") from None

    def live_objections(self, entry_id: str) -> list[Record]:
        record = self.records[entry_id]
        return [
            self.records[o]
            for o in record.objections
            if not self.records[o].resolved and not self.records[o].retired
        ]

    def status(self, entry_id: str) -> str:
        record = self.records[entry_id]
        if record.retired:
            return "retired"
        if record.resolved:
            return "resolved"
        if self.live_objections(entry_id):
            return "contested"
        if record.verified:
            return "verified"
        return "proposed"

    def entries(self, kind: str | None = None, include_retired: bool = False) -> list[Record]:
        out = []
        for entry_id in self.order:
            record = self.records[entry_id]
            if kind and record.entry.kind != kind:
                continue
            if record.retired and not include_retired:
                continue
            out.append(record)
        return out

    def outcome_of(self, decision_id: str) -> Record | None:
        """The latest live outcome recorded about a decision."""
        found = None
        for record in self.entries("outcome"):
            if record.entry.about == decision_id:
                found = record
        return found

    # -- the rules, applied to one event -----------------------------------

    def check(self, event: Event) -> None:
        """Raise TruthError if applying this event would break a rule."""
        if event.op == "add":
            if event.entry is None:
                raise TruthError("An add event needs an entry.")
            entry = event.entry
            if entry.id in self.records:
                raise TruthError(f"Entry {entry.id} already exists.")
            if entry.kind in KINDS_WITH_TARGET:
                if not entry.about:
                    raise TruthError(f"A {entry.kind} must say what it is about.")
                target = self.get(entry.about)
                if target.retired:
                    raise TruthError(f"{entry.about} is retired; nothing can be about it.")
                if entry.kind == "position":
                    if target.entry.kind != "decision":
                        raise TruthError("A position must be about a decision.")
                    if entry.round is None:
                        raise TruthError("A position needs a round number.")
                if entry.kind == "outcome" and target.entry.kind != "decision":
                    raise TruthError("An outcome must be about a decision.")
                if entry.kind == "objection" and target.entry.kind in ("position", "outcome"):
                    raise TruthError("Object to the decision or claim, not to a position or outcome.")
            elif entry.about:
                target = self.get(entry.about)
                if target.retired:
                    raise TruthError(f"{entry.about} is retired; nothing can be about it.")
            return

        if event.target is None:
            raise TruthError(f"A {event.op} event needs a target.")
        record = self.get(event.target)
        if record.retired:
            raise TruthError(f"{event.target} is already retired.")

        if event.op == "verify":
            if record.entry.kind not in VERIFIABLE:
                raise TruthError(f"Only a claim or a decision can be verified, not a {record.entry.kind}.")
            if not event.evidence:
                raise TruthError("Verifying needs evidence. Say what proved it.")
            if record.verified:
                raise TruthError(f"{event.target} is already verified.")
        elif event.op == "resolve":
            if record.entry.kind not in RESOLVABLE:
                raise TruthError(f"Only an objection, question, or task can be resolved, not a {record.entry.kind}.")
            if record.resolved:
                raise TruthError(f"{event.target} is already resolved.")
            if record.entry.kind == "objection" and event.upheld is None:
                raise TruthError("Resolving an objection must say whether it was upheld.")
            if not event.reason:
                raise TruthError("Resolving needs a reason.")
        elif event.op == "retire":
            if event.superseded_by is not None:
                successor = self.get(event.superseded_by)
                if successor.retired:
                    raise TruthError(f"{event.superseded_by} is itself retired.")
            if not event.reason and not event.superseded_by:
                raise TruthError("Retiring needs a reason or a successor.")

    def apply(self, event: Event) -> None:
        """Apply an event that has passed check()."""
        if event.op == "add":
            entry = event.entry
            assert entry is not None
            self.records[entry.id] = Record(entry=entry)
            self.order.append(entry.id)
            if entry.kind == "objection" and entry.about:
                self.records[entry.about].objections.append(entry.id)
            return
        record = self.records[event.target]  # type: ignore[index]
        if event.op == "verify":
            record.verified = True
            record.verify_evidence = list(event.evidence)
        elif event.op == "resolve":
            record.resolved = True
            record.upheld = event.upheld
            record.resolution = event.reason
        elif event.op == "retire":
            record.retired = True
            record.superseded_by = event.superseded_by


# -- the log on disk -----------------------------------------------------------


def parse_events(text: str) -> tuple[list[Event], list[str]]:
    """Every parseable event, and a problem line for each unparseable one."""
    events: list[Event] = []
    problems: list[str] = []
    for number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            events.append(Event.model_validate_json(line))
        except ValidationError as exc:
            problems.append(f"line {number}: {exc.errors()[0]['msg']}")
        except ValueError as exc:
            problems.append(f"line {number}: {exc}")
    return events, problems


def fold(events: list[Event]) -> tuple[State, list[str]]:
    """Replay events in order. Events that break a rule are reported and skipped."""
    state = State()
    problems: list[str] = []
    for number, event in enumerate(events, start=1):
        try:
            state.check(event)
        except TruthError as exc:
            problems.append(f"event {number} ({event.op}): {exc}")
            continue
        state.apply(event)
    return state, problems


def load(path: Path) -> tuple[State, list[str]]:
    if not path.exists():
        return State(), []
    events, problems = parse_events(path.read_text(encoding="utf-8"))
    state, more = fold(events)
    return state, problems + more


def append(path: Path, event: Event) -> Event:
    """Validate against the current log, then append. The log stays consistent."""
    state, problems = load(path)
    if problems:
        raise TruthError(
            f"The log already has {len(problems)} problems. Run check and fix them before writing."
        )
    state.check(event)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(event.model_dump_json(exclude_none=True) + "\n")
    return event


# -- views ---------------------------------------------------------------------


def _line(state: State, record: Record) -> str:
    entry = record.entry
    bits = [f"- [{entry.id}] {entry.text}"]
    details = [entry.author.key, entry.time[:10]]
    if entry.confidence is not None:
        details.append(f"confidence {entry.confidence:.0%}")
    status = state.status(entry.id)
    if status not in ("proposed", "resolved"):
        details.append(status)
    bits.append(f" ({', '.join(details)})")
    return "".join(bits)


def render_view(state: State) -> str:
    """The readable truth, generated. Retired entries are history, not view."""
    lines: list[str] = ["# Truth", ""]
    live = [r for r in state.entries()]
    counts = {
        "verified": sum(1 for r in live if state.status(r.entry.id) == "verified"),
        "contested": sum(1 for r in live if state.status(r.entry.id) == "contested"),
        "questions": sum(1 for r in state.entries("question") if not r.resolved),
        "tasks": sum(1 for r in state.entries("task") if not r.resolved),
    }
    lines.append(
        f"{len(live)} live entries. {counts['verified']} verified, {counts['contested']} contested, "
        f"{counts['questions']} open questions, {counts['tasks']} open tasks."
    )
    lines.append("")

    decisions = state.entries("decision")
    if decisions:
        lines.append("## Decisions")
        for record in decisions:
            lines.append(_line(state, record))
            outcome = state.outcome_of(record.entry.id)
            if outcome:
                lines.append(f"  outcome: {outcome.entry.text} ({outcome.entry.author.key}, {outcome.entry.time[:10]})")
            for objection in state.live_objections(record.entry.id):
                lines.append(f"  objection [{objection.entry.id}]: {objection.entry.text} ({objection.entry.author.key})")
        lines.append("")

    claims = state.entries("claim")
    verified = [r for r in claims if state.status(r.entry.id) == "verified"]
    contested = [r for r in claims if state.status(r.entry.id) == "contested"]
    proposed = [r for r in claims if state.status(r.entry.id) == "proposed"]
    for title, group in (("Verified", verified), ("Contested", contested), ("Proposed", proposed)):
        if group:
            lines.append(f"## {title}")
            for record in group:
                lines.append(_line(state, record))
                if title == "Verified" and record.verify_evidence:
                    lines.append(f"  evidence: {'; '.join(record.verify_evidence)}")
                if title == "Contested":
                    for objection in state.live_objections(record.entry.id):
                        lines.append(f"  objection [{objection.entry.id}]: {objection.entry.text} ({objection.entry.author.key})")
            lines.append("")

    questions = [r for r in state.entries("question") if not r.resolved]
    if questions:
        lines.append("## Open questions")
        lines.extend(_line(state, r) for r in questions)
        lines.append("")

    tasks = [r for r in state.entries("task") if not r.resolved]
    if tasks:
        lines.append("## Open tasks")
        lines.extend(_line(state, r) for r in tasks)
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


# -- the scoreboard ------------------------------------------------------------


@dataclass
class Score:
    author: str
    positions: int = 0
    correct: int = 0
    brier_sum: float = 0.0
    brier_n: int = 0
    conformity_flips: int = 0
    productive_updates: int = 0
    objections_resolved: int = 0
    objections_upheld: int = 0
    claims_verified: int = 0
    tokens: int = 0

    @property
    def accuracy(self) -> float | None:
        return self.correct / self.positions if self.positions else None

    @property
    def brier(self) -> float | None:
        return self.brier_sum / self.brier_n if self.brier_n else None

    @property
    def objection_yield(self) -> float | None:
        return self.objections_upheld / self.objections_resolved if self.objections_resolved else None

    @property
    def useful(self) -> int:
        return self.correct + self.objections_upheld + self.claims_verified

    @property
    def tokens_per_useful(self) -> float | None:
        return self.tokens / self.useful if self.useful and self.tokens else None


def _same(a: str, b: str) -> bool:
    return a.strip().lower() == b.strip().lower()


def scoreboard(state: State) -> dict[str, Score]:
    scores: dict[str, Score] = {}

    def score_for(key: str) -> Score:
        if key not in scores:
            scores[key] = Score(author=key)
        return scores[key]

    # Tokens: every entry an author wrote.
    for record in state.entries(include_retired=True):
        if record.entry.tokens:
            score_for(record.entry.author.key).tokens += record.entry.tokens

    # Claims that became verified.
    for record in state.entries("claim", include_retired=True):
        if record.verified:
            score_for(record.entry.author.key).claims_verified += 1

    # Objections that were resolved, and whether they stood.
    for record in state.entries("objection", include_retired=True):
        if record.resolved and record.upheld is not None:
            score = score_for(record.entry.author.key)
            score.objections_resolved += 1
            if record.upheld:
                score.objections_upheld += 1

    # Positions on decisions that have an outcome: first and final round per author.
    for decision in state.entries("decision", include_retired=True):
        outcome = state.outcome_of(decision.entry.id)
        if outcome is None:
            continue
        by_author: dict[str, list[Entry]] = {}
        for record in state.entries("position"):
            if record.entry.about == decision.entry.id:
                by_author.setdefault(record.entry.author.key, []).append(record.entry)
        for key, positions in by_author.items():
            positions.sort(key=lambda p: p.round or 0)
            first, final = positions[0], positions[-1]
            score = score_for(key)
            final_correct = _same(final.text, outcome.entry.text)
            first_correct = _same(first.text, outcome.entry.text)
            score.positions += 1
            if final_correct:
                score.correct += 1
            if final.confidence is not None:
                score.brier_sum += (final.confidence - (1.0 if final_correct else 0.0)) ** 2
                score.brier_n += 1
            if first is not final:
                if first_correct and not final_correct:
                    score.conformity_flips += 1
                if not first_correct and final_correct:
                    score.productive_updates += 1
    return scores


def _pct(value: float | None) -> str:
    return "-" if value is None else f"{value:.0%}"


def render_scoreboard(scores: dict[str, Score]) -> str:
    if not scores:
        return "No one has been measured yet. Record decisions, positions, and outcomes first.\n"
    lines = [
        "| author | positions | accuracy | calibration (Brier, lower is better) | conformity flips | productive updates | objections upheld | claims verified | tokens per useful |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    for key in sorted(scores):
        s = scores[key]
        brier = "-" if s.brier is None else f"{s.brier:.2f}"
        upheld = f"{s.objections_upheld}/{s.objections_resolved}" if s.objections_resolved else "-"
        tpu = "-" if s.tokens_per_useful is None else f"{s.tokens_per_useful:,.0f}"
        lines.append(
            f"| {key} | {s.positions} | {_pct(s.accuracy)} | {brier} | {s.conformity_flips} | "
            f"{s.productive_updates} | {upheld} | {s.claims_verified} | {tpu} |"
        )
    return "\n".join(lines) + "\n"


def spoken_summary(state: State, problems: list[str]) -> str:
    if problems:
        return f"The truth has {len(problems)} problems. Fix them before anyone writes."
    live = state.entries()
    verified = sum(1 for r in live if state.status(r.entry.id) == "verified")
    contested = sum(1 for r in live if state.status(r.entry.id) == "contested")
    questions = sum(1 for r in state.entries("question") if not r.resolved)
    return (
        f"The truth is consistent: {len(live)} live entries, {verified} verified, "
        f"{contested} contested, {questions} open questions."
    )
