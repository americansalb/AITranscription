"""Deliberation.

A council decides a question in three rounds, built so that the published
failure modes of multi-agent debate cannot happen by construction:

1. Positions, blind. Each seat answers alone. It does not know what the
   others said; it does not know who the others are.
2. Objections, anonymized. Each seat sees the others' positions under
   shuffled letters and may only object: a concrete flaw, with evidence and a
   severity. No praise, no agreement, no revising yet.
3. Revision, private. Each seat sees the objections raised against its own
   position, must accept or reject each with a reason, and gives a final
   position. Changing to match the majority is explicitly not a reason.

Then code tallies. Votes are weighted by each seat's calibration on past
decisions when the scoreboard knows it, a decision is adopted only when
agreement clears the threshold, and when it does not the output is the two
live options and the crux rather than a manufactured consensus. Every run is
written into the shared truth: the decision, every position in rounds one and
three, every objection and how its target answered it, and the adoption. The
scoreboard learns from it once a human records the outcome.

Each round's brief is built by a function that can only be handed what that
round is allowed to see. That is the information barrier, and it is the
signature, not a promise.
"""

from __future__ import annotations

import json
import random
import secrets
import string
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Callable, Literal

from pydantic import BaseModel, Field

from . import providers, truth

Severity = Literal["high", "medium", "low"]

ROLES: dict[str, str] = {
    "analyst": "You weigh evidence and quantify. State assumptions explicitly and prefer numbers to adjectives.",
    "skeptic": "You look for what breaks: worst cases, hidden costs, second-order effects, and the ways the question itself is wrong.",
    "advocate": "You speak for the end user with the least power in the room, especially one who cannot see the screen. Judge options by their first minute of use.",
    "builder": "You care about what can be built and kept simple. Prefer the option with fewer moving parts and a shorter path to something real.",
    "historian": "You ask what has been tried before and why it failed. Use the context for precedents and name the lesson, not the anecdote.",
}
DEFAULT_ROLE_ORDER = ["analyst", "skeptic", "advocate", "builder", "historian"]
PROVIDER_ORDER = ["anthropic", "groq", "openai"]
DEFAULT_THRESHOLD = 2 / 3

COUNCIL_RULES = (
    "You are one seat on a council deciding a question. Other seats exist, but you will "
    "not see their answers in this round and they will not see yours. Answer from your "
    "own perspective only. Reply only with JSON matching the schema you were given."
)


class DeliberationError(ValueError):
    pass


@dataclass(frozen=True)
class Seat:
    role: str
    provider: str
    model: str | None = None

    @property
    def model_name(self) -> str:
        return self.model or providers.default_model(self.provider)

    @property
    def author(self) -> truth.Author:
        return truth.Author(role=self.role, model=f"{self.provider}:{self.model_name}")

    @property
    def key(self) -> str:
        return self.author.key

    @classmethod
    def parse(cls, text: str) -> "Seat":
        role, sep, rest = text.partition("@")
        if not sep or not role.strip() or not rest.strip():
            raise DeliberationError(
                f"A seat is written role@provider or role@provider:model, not {text!r}."
            )
        provider, _, model = rest.partition(":")
        if provider not in providers.available():
            raise DeliberationError(
                f"Unknown provider {provider!r} in seat {text!r}. Available: {', '.join(providers.available())}."
            )
        return cls(role=role.strip(), provider=provider, model=model.strip() or None)


def default_seats() -> list[Seat]:
    """Three seats across every model family the environment has a key for."""
    families = [p for p in PROVIDER_ORDER if providers.has_key(p)] or [providers.DEFAULT_PROVIDER]
    return [
        Seat(role=role, provider=families[i % len(families)])
        for i, role in enumerate(DEFAULT_ROLE_ORDER[:3])
    ]


def role_prompt(role: str, custom: dict[str, str] | None = None) -> str:
    text = (custom or {}).get(role) or ROLES.get(role) or f"You take the perspective of a {role}."
    return f"Role: {role}. {text}"


# -- what a seat says -----------------------------------------------------------


class Position(BaseModel):
    position: str = Field(description="Your answer. When options are given, exactly one of them, word for word.")
    confidence: float = Field(ge=0.0, le=1.0, description="How likely you are to be right, 0 to 1. You are scored on calibration, so be honest.")
    reasons: list[str] = Field(default_factory=list, description="Up to three reasons, one sentence each.")
    assumptions: list[str] = Field(default_factory=list, description="What you are assuming that could be false.")
    what_would_change_my_mind: str = Field(default="", description="The evidence that would move you.")


class ObjectionItem(BaseModel):
    to: str = Field(description="The letter of the position you object to.")
    claim: str = Field(description="One sentence: what is wrong with it.")
    evidence: str = Field(description="The fact, number, or case that shows it.")
    severity: Severity


class Objections(BaseModel):
    objections: list[ObjectionItem] = Field(default_factory=list)


class Answer(BaseModel):
    objection: str = Field(description="The objection's label, such as O1.")
    accept: bool = Field(description="True if the objection stands.")
    reason: str = Field(description="Why it stands or why it does not.")


class Revision(BaseModel):
    position: str = Field(description="Your final answer. When options are given, exactly one of them, word for word.")
    confidence: float = Field(ge=0.0, le=1.0)
    answers: list[Answer] = Field(default_factory=list)


# -- briefs: each function can only be handed what its round may see --------------


def round1_brief(seat: Seat, question: str, options: list[str], context: str, custom: dict[str, str] | None) -> tuple[str, str]:
    system = f"{role_prompt(seat.role, custom)}\n\n{COUNCIL_RULES}"
    user = f"Question: {question}\n"
    if options:
        user += "Options. Choose exactly one, word for word:\n" + "".join(f"- {o}\n" for o in options)
    else:
        user += "There are no fixed options. Propose the answer you believe is right, in one sentence.\n"
    if context.strip():
        user += f"\nContext, identical for every seat:\n{context.strip()}\n"
    return system, user


def _show(label: str, position: Position) -> str:
    text = f"Position {label}: {position.position} (confidence {position.confidence:.0%})\n"
    if position.reasons:
        text += "  Reasons: " + " ".join(position.reasons) + "\n"
    if position.assumptions:
        text += "  Assumptions: " + " ".join(position.assumptions) + "\n"
    return text


def round2_brief(seat: Seat, question: str, shown: list[tuple[str, Position]], custom: dict[str, str] | None) -> tuple[str, str]:
    system = (
        f"{role_prompt(seat.role, custom)}\n\n"
        "You are shown the other seats' positions, anonymized and shuffled. You do not know "
        "who wrote them. Raise objections only: a concrete way a position is wrong, with "
        "evidence and a severity. No praise, no agreement, and do not revise your own position "
        "yet. If a position has no real flaw, raise nothing against it. Reply only with JSON."
    )
    user = f"Question: {question}\n\nPositions:\n" + "".join(_show(label, p) for label, p in shown)
    return system, user


def round3_brief(
    seat: Seat,
    question: str,
    options: list[str],
    own_label: str,
    shown: list[tuple[str, Position]],
    against: list[tuple[str, ObjectionItem]],
    custom: dict[str, str] | None,
) -> tuple[str, str]:
    system = (
        f"{role_prompt(seat.role, custom)}\n\n"
        "You are shown every position including your own, and the objections raised against "
        "yours. Answer each objection: accept it if it stands, reject it with a reason if it "
        "does not. Then give your final position and confidence. Changing your mind is right "
        "only when an objection or a position gave you a reason. Changing to match the "
        "majority is not a reason. Reply only with JSON."
    )
    user = f"Question: {question}\n"
    if options:
        user += "Options. Choose exactly one, word for word:\n" + "".join(f"- {o}\n" for o in options)
    user += f"\nYour position is {own_label}.\n\nAll positions:\n" + "".join(_show(label, p) for label, p in shown)
    if against:
        user += "\nObjections to your position:\n" + "".join(
            f"{label} (severity {o.severity}): {o.claim} Evidence: {o.evidence}\n" for label, o in against
        )
    else:
        user += "\nNo objections were raised against your position.\n"
    return system, user


# -- the record ----------------------------------------------------------------


@dataclass
class ObjectionOutcome:
    by: str          # objector seat key
    by_role: str
    to: str          # holder seat key
    to_role: str
    to_label: str
    label: str       # O1, O2 as shown to the holder
    claim: str
    evidence: str
    severity: str
    accepted: bool | None
    reason: str


@dataclass
class SeatResult:
    key: str
    role: str
    provider: str
    model: str
    label: str
    round1: dict
    round3: dict | None
    final_position: str
    final_confidence: float
    weight: float


@dataclass
class Record:
    question: str
    options: list[str]
    seed: int
    rounds: int
    threshold: float
    single_family: bool
    seats: list[SeatResult] = field(default_factory=list)
    objections: list[ObjectionOutcome] = field(default_factory=list)
    votes: dict[str, float] = field(default_factory=dict)
    agreement: float = 0.0
    decided: str | None = None
    crux: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    decision_id: str | None = None

    def ranked(self) -> list[tuple[str, float]]:
        total = sum(self.votes.values()) or 1.0
        return sorted(((o, v / total) for o, v in self.votes.items()), key=lambda x: -x[1])

    def spoken_summary(self) -> str:
        upheld = sum(1 for o in self.objections if o.accepted)
        tail = f" {len(self.objections)} objections, {upheld} upheld."
        if self.single_family:
            tail += " All seats share one model family, so their errors may be correlated."
        if self.decided:
            return (
                f"Decided: {self.decided}. Weighted agreement {self.agreement:.0%} across "
                f"{len(self.seats)} seats.{tail}"
            )
        top = ", ".join(f"{o} {share:.0%}" for o, share in self.ranked()[:2])
        crux = f" The crux: {self.crux[0]}" if self.crux else ""
        return f"No decision. {top} across {len(self.seats)} seats.{tail}{crux}"

    def to_json(self) -> str:
        return json.dumps(asdict(self), indent=2)


# -- the run ---------------------------------------------------------------------

Asker = Callable[..., BaseModel]


def _match(text: str, options: list[str]) -> str:
    """Snap a position to an option when it names one; otherwise keep it as a write-in."""
    for option in options:
        if truth._same(text, option):
            return option
    lowered = text.strip().lower()
    for option in options:
        if option.strip().lower() in lowered:
            return option
    return text.strip()


def weights_from_scoreboard(state: truth.State, minimum_positions: int = 3, floor: float = 0.25) -> dict[str, float]:
    """A seat's vote weighs its calibration: one minus its Brier score, never below the floor.
    Seats without enough history weigh one."""
    out: dict[str, float] = {}
    for key, score in truth.scoreboard(state).items():
        if score.brier is not None and score.positions >= minimum_positions:
            out[key] = max(floor, 1.0 - score.brier)
    return out


def run(
    question: str,
    options: list[str],
    seats: list[Seat],
    *,
    context: str = "",
    rounds: int = 3,
    threshold: float = DEFAULT_THRESHOLD,
    weights: dict[str, float] | None = None,
    custom_roles: dict[str, str] | None = None,
    asker: Asker | None = None,
    seed: int | None = None,
) -> Record:
    if not question.strip():
        raise DeliberationError("A deliberation needs a question.")
    if not seats:
        raise DeliberationError("A council needs at least one seat.")
    if rounds not in (1, 2, 3):
        raise DeliberationError("Rounds must be 1, 2, or 3.")
    if not 0.0 < threshold <= 1.0:
        raise DeliberationError("The threshold must be between 0 and 1.")
    ask = asker or providers.ask
    weights = weights or {}
    seed = secrets.randbelow(2**31) if seed is None else seed
    options = [o.strip() for o in options if o.strip()]
    families = {s.provider for s in seats}
    record = Record(
        question=question.strip(),
        options=options,
        seed=seed,
        rounds=rounds,
        threshold=threshold,
        single_family=len(families) == 1 and len(seats) > 1,
    )

    # Round one: blind.
    positions: dict[Seat, Position] = {}
    for seat in seats:
        system, user = round1_brief(seat, question, options, context, custom_roles)
        positions[seat] = ask(seat.provider, system, user, Position, seat.model)
    if options:
        for seat, p in positions.items():
            p.position = _match(p.position, options)
    else:
        # The seats' own proposals become the ballot.
        ballot: list[str] = []
        for p in positions.values():
            if not any(truth._same(p.position, b) for b in ballot):
                ballot.append(p.position.strip())
        options = ballot
        record.options = options

    # Anonymize and shuffle, with the seed on the record.
    order = list(seats)
    random.Random(seed).shuffle(order)
    labels = {seat: string.ascii_uppercase[i] for i, seat in enumerate(order)}
    by_label = {label: seat for seat, label in labels.items()}

    # Round two: objections only.
    raised: dict[Seat, list[ObjectionItem]] = {seat: [] for seat in seats}
    if rounds >= 2:
        for seat in seats:
            shown = [(labels[s], positions[s]) for s in order if s is not seat]
            system, user = round2_brief(seat, question, shown, custom_roles)
            result = ask(seat.provider, system, user, Objections, seat.model)
            allowed = {label for label, _ in shown}
            raised[seat] = [o for o in result.objections if o.to in allowed]

    # Objections against each seat, numbered as the holder will see them.
    against: dict[Seat, list[tuple[str, Seat, ObjectionItem]]] = {seat: [] for seat in seats}
    for objector, items in raised.items():
        for item in items:
            holder = by_label[item.to]
            number = len(against[holder]) + 1
            against[holder].append((f"O{number}", objector, item))

    # Round three: private revision.
    revisions: dict[Seat, Revision] = {}
    if rounds >= 3:
        shown_all = [(labels[s], positions[s]) for s in order]
        for seat in seats:
            system, user = round3_brief(
                seat, question, options, labels[seat], shown_all,
                [(label, item) for label, _, item in against[seat]], custom_roles,
            )
            revisions[seat] = ask(seat.provider, system, user, Revision, seat.model)

    # Objection outcomes: how each holder answered.
    for holder, items in against.items():
        answers = {a.objection.strip().upper(): a for a in revisions[holder].answers} if holder in revisions else {}
        for label, objector, item in items:
            answer = answers.get(label)
            record.objections.append(ObjectionOutcome(
                by=objector.key, by_role=objector.role, to=holder.key, to_role=holder.role,
                to_label=labels[holder], label=label, claim=item.claim, evidence=item.evidence,
                severity=item.severity, accepted=None if answer is None else answer.accept,
                reason=answer.reason if answer else "",
            ))

    # Tally, weighted by calibration.
    for seat in seats:
        if seat in revisions:
            final, confidence = _match(revisions[seat].position, options), revisions[seat].confidence
        else:
            final, confidence = positions[seat].position, positions[seat].confidence
        weight = weights.get(seat.key, 1.0)
        record.votes[final] = record.votes.get(final, 0.0) + weight
        record.seats.append(SeatResult(
            key=seat.key, role=seat.role, provider=seat.provider, model=seat.model_name,
            label=labels[seat], round1=positions[seat].model_dump(),
            round3=revisions[seat].model_dump() if seat in revisions else None,
            final_position=final, final_confidence=confidence, weight=weight,
        ))
    ranked = record.ranked()
    top, share = ranked[0]
    record.agreement = share
    if share + 1e-9 >= threshold:
        record.decided = top
        holders = {s.key for s in record.seats if s.final_position == top}
        for o in record.objections:
            if o.severity == "high" and o.to in holders and o.accepted is False:
                record.warnings.append(f"A high objection against the chosen option was rejected by its holder: {o.claim}")
            if o.severity == "high" and o.to in holders and o.accepted is None:
                record.warnings.append(f"A high objection against the chosen option was never answered: {o.claim}")
    else:
        contenders = {o for o, _ in ranked[:2]}
        holders = {s.label for s in record.seats if s.final_position in contenders}
        record.crux = [o.claim for o in record.objections if o.to_label in holders and o.accepted is not False]
        if not record.crux:
            record.crux = [o.claim for o in record.objections if o.to_label in holders]
    return record


# -- writing the run into the truth ----------------------------------------------


def record_to_truth(log: Path, record: Record, author: truth.Author, record_path: Path | None) -> str:
    """Append the whole run: decision, positions, objections with their answers, adoption."""
    evidence = [str(record_path)] if record_path else []
    decision = truth.Entry(
        id=truth.new_id(), kind="decision", text=record.question, author=author, time=truth.now(),
        options=record.options, evidence=evidence, tags=["deliberation"],
    )
    truth.append(log, truth.Event(op="add", time=decision.time, author=author, entry=decision))

    seat_by_key = {s.key: s for s in record.seats}

    def seat_author(key: str) -> truth.Author:
        return truth.Author.parse(key)

    for s in record.seats:
        who = seat_author(s.key)
        first = truth.Entry(id=truth.new_id(), kind="position", text=s.round1["position"], author=who, time=truth.now(),
                            confidence=s.round1["confidence"], about=decision.id, round=1)
        truth.append(log, truth.Event(op="add", time=first.time, author=who, entry=first))
        if s.round3 is not None:
            final = truth.Entry(id=truth.new_id(), kind="position", text=s.final_position, author=who, time=truth.now(),
                                confidence=s.final_confidence, about=decision.id, round=3)
            truth.append(log, truth.Event(op="add", time=final.time, author=who, entry=final))

    for o in record.objections:
        objector = seat_author(o.by)
        holder_role = seat_by_key[o.to].role
        entry = truth.Entry(
            id=truth.new_id(), kind="objection", author=objector, time=truth.now(), about=decision.id,
            severity=o.severity,  # type: ignore[arg-type]
            text=f"Against position {o.to_label} ({holder_role}): {o.claim} Evidence: {o.evidence}",
        )
        truth.append(log, truth.Event(op="add", time=entry.time, author=objector, entry=entry))
        if o.accepted is not None:
            holder = seat_author(o.to)
            truth.append(log, truth.Event(op="resolve", time=truth.now(), author=holder, target=entry.id,
                                          upheld=o.accepted, reason=o.reason or ("accepted" if o.accepted else "rejected")))

    if record.decided:
        truth.append(log, truth.Event(op="decide", time=truth.now(), author=author, target=decision.id,
                                      chosen=record.decided, evidence=evidence))
    record.decision_id = decision.id
    return decision.id
