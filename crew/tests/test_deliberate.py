import random
import re
from pathlib import Path

import pytest

from crew import deliberate, truth
from crew.deliberate import Answer, ObjectionItem, Objections, Position, Revision, Seat


class Scripted:
    """Answers by role and output type, and records every brief it was sent."""

    def __init__(self, script):
        self.script = script
        self.calls = []

    def __call__(self, provider, system, user, output_model, model=None):
        role = system.split(".", 1)[0].removeprefix("Role: ")
        self.calls.append({"provider": provider, "role": role, "system": system, "user": user,
                           "kind": output_model.__name__, "model": model})
        value = self.script[(role, output_model.__name__)]
        return value(user) if callable(value) else value


def labels_holding(user: str, option: str) -> list[str]:
    """The letters whose shown position is this option."""
    return [m.group(1) for m in re.finditer(r"Position ([A-Z]): (.*?) \(", user) if m.group(2).strip() == option]


SEATS = [Seat("analyst", "anthropic"), Seat("skeptic", "groq"), Seat("advocate", "anthropic")]
ROLE_NAMES = ["analyst", "skeptic", "advocate"]


def groq_vs_openai_script(advocate_accepts=True):
    """Analyst and advocate say Groq; the skeptic says OpenAI and objects to every Groq position."""
    return {
        ("analyst", "Position"): Position(position="Groq", confidence=0.8, reasons=["Nine times cheaper."], assumptions=["Batch latency is fine."]),
        ("skeptic", "Position"): Position(position="OpenAI", confidence=0.6, reasons=["Streaming."]),
        ("advocate", "Position"): Position(position="Groq", confidence=0.7),
        ("analyst", "Objections"): Objections(),
        ("advocate", "Objections"): Objections(),
        ("skeptic", "Objections"): lambda user: Objections(objections=[
            ObjectionItem(to=label, claim="Ten second minimum billing makes short clips cost more.", evidence="Groq docs.", severity="high")
            for label in labels_holding(user, "Groq")
        ]),
        ("analyst", "Revision"): Revision(position="Groq", confidence=0.85, answers=[Answer(objection="O1", accept=False, reason="Still cheaper even at the minimum.")]),
        ("advocate", "Revision"): Revision(position="Groq", confidence=0.65, answers=[Answer(objection="O1", accept=advocate_accepts, reason="Fair point about short commands.")]),
        ("skeptic", "Revision"): Revision(position="OpenAI", confidence=0.6),
    }


def test_round_one_is_blind_and_round_two_is_anonymized():
    asker = Scripted(groq_vs_openai_script())
    deliberate.run("Which recognizer goes first?", ["Groq", "OpenAI"], SEATS, asker=asker, seed=7)
    round1 = [c for c in asker.calls if c["kind"] == "Position"]
    round2 = [c for c in asker.calls if c["kind"] == "Objections"]
    round3 = [c for c in asker.calls if c["kind"] == "Revision"]
    assert len(round1) == len(round2) == len(round3) == 3
    for call in round1:
        assert "Position A" not in call["user"] and "Nine times cheaper" not in call["user"]
        assert "will not see their answers" in call["system"]
    order = list(SEATS)
    random.Random(7).shuffle(order)
    label_of_role = {seat.role: "ABC"[i] for i, seat in enumerate(order)}
    for call in round2:
        own = label_of_role[call["role"]]
        others = sorted(set("ABC") - {own})
        assert f"Position {own}" not in call["user"], "a seat never sees its own position in round two"
        assert all(f"Position {other}" in call["user"] for other in others)
        for role in ROLE_NAMES:
            assert role not in call["user"], f"round two leaked the role name {role}"
    for call in round3:
        own = label_of_role[call["role"]]
        assert f"Your position is {own}." in call["user"]
        assert all(f"Position {label}" in call["user"] for label in "ABC"), "round three shows every position including your own"
    # Providers and models are routed per seat.
    assert {c["provider"] for c in round1} == {"anthropic", "groq"}


def test_a_two_thirds_majority_decides_and_objections_are_answered():
    asker = Scripted(groq_vs_openai_script())
    record = deliberate.run("Which recognizer goes first?", ["Groq", "OpenAI"], SEATS, asker=asker, seed=7)
    assert record.decided == "Groq"
    assert record.agreement == pytest.approx(2 / 3)
    assert len(record.objections) == 2
    accepted = {o.to_role: o.accepted for o in record.objections}
    assert accepted == {"analyst": False, "advocate": True}
    assert all(o.by_role == "skeptic" and o.severity == "high" for o in record.objections)
    assert any("rejected by its holder" in w for w in record.warnings)
    assert "Decided: Groq. Weighted agreement 67% across 3 seats. 2 objections, 1 upheld." == record.spoken_summary()
    assert not record.single_family
    labels = sorted(s.label for s in record.seats)
    assert labels == ["A", "B", "C"]


def test_the_seed_fixes_the_shuffle_and_is_on_the_record():
    a = deliberate.run("Q?", ["Groq", "OpenAI"], SEATS, asker=Scripted(groq_vs_openai_script()), seed=7)
    b = deliberate.run("Q?", ["Groq", "OpenAI"], SEATS, asker=Scripted(groq_vs_openai_script()), seed=7)
    assert [s.label for s in a.seats] == [s.label for s in b.seats]
    assert a.seed == 7
    order = list(SEATS)
    random.Random(7).shuffle(order)
    expected = {seat.key: "ABC"[i] for i, seat in enumerate(order)}
    assert {s.key: s.label for s in a.seats} == expected


def test_a_split_council_makes_no_decision_and_names_the_crux():
    two = SEATS[:2]
    asker = Scripted(groq_vs_openai_script())
    record = deliberate.run("Which recognizer goes first?", ["Groq", "OpenAI"], two, asker=asker, seed=1)
    assert record.decided is None
    assert record.agreement == pytest.approx(0.5)
    assert record.crux == ["Ten second minimum billing makes short clips cost more."]
    summary = record.spoken_summary()
    assert summary.startswith("No decision.") and "The crux:" in summary


def test_without_options_the_proposals_become_the_ballot():
    script = groq_vs_openai_script()
    script[("analyst", "Position")] = Position(position="Use Groq", confidence=0.8)
    script[("advocate", "Position")] = Position(position="use groq", confidence=0.7)
    script[("skeptic", "Position")] = Position(position="Use OpenAI", confidence=0.6)
    script[("skeptic", "Objections")] = Objections()
    script[("analyst", "Revision")] = Revision(position="Use Groq", confidence=0.8)
    script[("advocate", "Revision")] = Revision(position="use groq", confidence=0.7)
    script[("skeptic", "Revision")] = Revision(position="Use OpenAI", confidence=0.6)
    record = deliberate.run("Which recognizer goes first?", [], SEATS, asker=Scripted(script), seed=3)
    assert record.options == ["Use Groq", "Use OpenAI"]
    assert record.decided == "Use Groq"


def test_write_ins_snap_to_an_option_when_they_name_one():
    script = groq_vs_openai_script()
    script[("analyst", "Position")] = Position(position="I would go with Groq first.", confidence=0.8)
    record = deliberate.run("Q?", ["Groq", "OpenAI"], SEATS, asker=Scripted(script), seed=7)
    analyst = next(s for s in record.seats if s.role == "analyst")
    assert analyst.round1["position"] == "Groq"


def test_one_round_is_a_blind_vote_with_no_debate():
    asker = Scripted(groq_vs_openai_script())
    record = deliberate.run("Q?", ["Groq", "OpenAI"], SEATS, asker=asker, rounds=1, seed=7)
    assert {c["kind"] for c in asker.calls} == {"Position"}
    assert record.decided == "Groq" and record.objections == []
    assert all(s.round3 is None for s in record.seats)


def test_calibration_weights_can_outvote_a_head_count():
    weights = {"analyst@anthropic:claude-opus-5": 0.25, "advocate@anthropic:claude-opus-5": 0.25}
    record = deliberate.run("Q?", ["Groq", "OpenAI"], SEATS, asker=Scripted(groq_vs_openai_script()), weights=weights, seed=7)
    assert record.decided == "OpenAI"
    assert record.agreement == pytest.approx(1.0 / 1.5)


def test_weights_come_from_calibration_with_a_floor_and_a_minimum_history():
    state = truth.State()

    def add(kind, text, who, **fields):
        entry = truth.Entry(id=truth.new_id(), kind=kind, text=text, author=truth.Author.parse(who), time=truth.now(), **fields)
        event = truth.Event(op="add", time=truth.now(), author=entry.author, entry=entry)
        state.check(event)
        state.apply(event)
        return entry

    for i in range(3):
        d = add("decision", f"D{i}?", "founder@human", options=["A", "B"])
        add("position", "A", "sharp@anthropic:claude-opus-5", about=d.id, round=1, confidence=0.9)
        add("position", "A", "blunt@groq:openai/gpt-oss-120b", about=d.id, round=1, confidence=0.9)
        add("position", "A", "newbie@openai:gpt-5", about=d.id, round=1, confidence=0.9) if i == 0 else None
        add("outcome", "A" if i < 2 else "B", "founder@human", about=d.id, evidence=["later"])
    weights = deliberate.weights_from_scoreboard(state)
    # sharp and blunt are identical here: right twice, wrong once at 90% confidence.
    brier = (2 * (0.9 - 1) ** 2 + (0.9 - 0) ** 2) / 3
    assert weights["sharp@anthropic:claude-opus-5"] == pytest.approx(max(0.25, 1 - brier))
    assert "newbie@openai:gpt-5" not in weights, "one decision is not enough history"
    assert deliberate.weights_from_scoreboard(state, floor=0.9)["blunt@groq:openai/gpt-oss-120b"] == 0.9


def test_default_seats_span_the_families_with_keys(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "x")
    monkeypatch.setenv("GROQ_API_KEY", "y")
    monkeypatch.delenv("OPENAI_API_KEY", raising=False)
    seats = deliberate.default_seats()
    assert [(s.role, s.provider) for s in seats] == [("analyst", "anthropic"), ("skeptic", "groq"), ("advocate", "anthropic")]
    for key in ("ANTHROPIC_API_KEY", "GROQ_API_KEY"):
        monkeypatch.delenv(key)
    assert {s.provider for s in deliberate.default_seats()} == {"anthropic"}


def test_seats_parse_and_refuse_nonsense():
    seat = Seat.parse("skeptic@groq:llama-3.3-70b-versatile")
    assert (seat.role, seat.provider, seat.model) == ("skeptic", "groq", "llama-3.3-70b-versatile")
    assert seat.key == "skeptic@groq:llama-3.3-70b-versatile"
    assert Seat.parse("analyst@anthropic").key == "analyst@anthropic:claude-opus-5"
    for bad in ("analyst", "@groq", "analyst@", "analyst@nowhere"):
        with pytest.raises(deliberate.DeliberationError):
            Seat.parse(bad)
    with pytest.raises(deliberate.DeliberationError, match="at least one seat"):
        deliberate.run("Q?", [], [], asker=Scripted({}))
    with pytest.raises(deliberate.DeliberationError, match="Rounds"):
        deliberate.run("Q?", [], SEATS, asker=Scripted({}), rounds=4)


def test_the_whole_run_is_written_into_the_truth(tmp_path: Path):
    log = tmp_path / "truth.jsonl"
    record = deliberate.run("Which recognizer goes first?", ["Groq", "OpenAI"], SEATS, asker=Scripted(groq_vs_openai_script()), seed=7)
    decision_id = deliberate.record_to_truth(log, record, truth.Author.parse("council@crew"), tmp_path / "run.json")
    assert record.decision_id == decision_id
    state, problems = truth.load(log)
    assert problems == []
    decision = state.get(decision_id)
    assert decision.chosen == "Groq" and decision.entry.options == ["Groq", "OpenAI"]
    positions = [r for r in state.entries("position") if r.entry.about == decision_id]
    assert len(positions) == 6, "round one and round three for each of three seats"
    objections = [r for r in state.entries("objection") if r.entry.about == decision_id]
    assert len(objections) == 2 and all(r.resolved for r in objections)
    assert sorted(r.upheld for r in objections) == [False, True]
    assert state.status(decision_id) == "proposed", "a decision is not verified by being made"
    view = truth.render_view(state)
    assert "decided: Groq" in view and "Which recognizer goes first?" in view
    # Once a human records the outcome, the scoreboard learns from the run.
    outcome = truth.Entry(id=truth.new_id(), kind="outcome", text="Groq", author=truth.Author.parse("founder@human"), time=truth.now(), about=decision_id, evidence=["bake-off"])
    truth.append(log, truth.Event(op="add", time=outcome.time, author=outcome.author, entry=outcome))
    state, _ = truth.load(log)
    scores = truth.scoreboard(state)
    assert scores["analyst@anthropic:claude-opus-5"].accuracy == 1.0
    assert scores["skeptic@groq:openai/gpt-oss-120b"].accuracy == 0.0
    assert scores["skeptic@groq:openai/gpt-oss-120b"].objection_yield == 0.5


def test_decide_is_only_for_decisions_and_only_among_the_options():
    state = truth.State()
    who = truth.Author.parse("council@crew")
    claim = truth.Entry(id="c1", kind="claim", text="A fact.", author=who, time=truth.now())
    decision = truth.Entry(id="d1", kind="decision", text="Pick one?", author=who, time=truth.now(), options=["Groq", "OpenAI"])
    for entry in (claim, decision):
        event = truth.Event(op="add", time=truth.now(), author=who, entry=entry)
        state.check(event)
        state.apply(event)
    with pytest.raises(truth.TruthError, match="Only a decision"):
        state.check(truth.Event(op="decide", time=truth.now(), author=who, target="c1", chosen="x"))
    with pytest.raises(truth.TruthError, match="not one of the options"):
        state.check(truth.Event(op="decide", time=truth.now(), author=who, target="d1", chosen="Deepgram"))
    with pytest.raises(truth.TruthError, match="chosen option"):
        state.check(truth.Event(op="decide", time=truth.now(), author=who, target="d1", chosen="  "))
    event = truth.Event(op="decide", time=truth.now(), author=who, target="d1", chosen="groq")
    state.check(event)
    state.apply(event)
    assert state.get("d1").chosen == "groq"
