from pathlib import Path

import pytest

from crew import truth
from crew.truth import Author, Entry, Event, State, TruthError


def author(text="tester@claude-opus-5"):
    return Author.parse(text)


def add(state_or_path, kind, text, who="tester@claude-opus-5", **fields):
    entry = Entry(id=fields.pop("id", truth.new_id()), kind=kind, text=text, author=author(who), time=truth.now(), **fields)
    event = Event(op="add", time=truth.now(), author=author(who), entry=entry)
    if isinstance(state_or_path, State):
        state_or_path.check(event)
        state_or_path.apply(event)
    else:
        truth.append(state_or_path, event)
    return entry


def op(state_or_path, name, target, who="tester@claude-opus-5", **fields):
    event = Event(op=name, time=truth.now(), author=author(who), target=target, **fields)
    if isinstance(state_or_path, State):
        state_or_path.check(event)
        state_or_path.apply(event)
    else:
        truth.append(state_or_path, event)
    return event


def test_author_is_role_at_model():
    a = Author.parse("reviewer@gpt-5")
    assert (a.role, a.model, a.key) == ("reviewer", "gpt-5", "reviewer@gpt-5")
    for bad in ("reviewer", "@gpt-5", "reviewer@", ""):
        with pytest.raises(TruthError):
            Author.parse(bad)


def test_a_claim_starts_proposed_and_verification_needs_evidence():
    state = State()
    claim = add(state, "claim", "The capture takes 40 ms on the Mail window.")
    assert state.status(claim.id) == "proposed"
    with pytest.raises(TruthError, match="evidence"):
        op(state, "verify", claim.id)
    op(state, "verify", claim.id, evidence=["timing in commit 914407b"])
    assert state.status(claim.id) == "verified"
    with pytest.raises(TruthError, match="already verified"):
        op(state, "verify", claim.id, evidence=["again"])


def test_a_live_objection_makes_its_target_contested_until_resolved():
    state = State()
    claim = add(state, "claim", "Groq is cheaper than OpenAI for transcription.")
    objection = add(state, "objection", "Only for clips longer than ten seconds.", who="skeptic@gpt-5", about=claim.id, severity="medium")
    assert state.status(claim.id) == "contested"
    with pytest.raises(TruthError, match="upheld"):
        op(state, "resolve", objection.id, reason="checked the billing page")
    op(state, "resolve", objection.id, upheld=True, reason="Groq bills a ten second minimum.")
    assert state.status(claim.id) == "proposed"
    assert state.status(objection.id) == "resolved"


def test_nothing_is_overwritten_retiring_keeps_history():
    state = State()
    old = add(state, "claim", "The talk key is right Option.")
    new = add(state, "claim", "The talk key is a user choice, default right Option.")
    with pytest.raises(TruthError, match="reason or a successor"):
        op(state, "retire", old.id)
    op(state, "retire", old.id, superseded_by=new.id)
    assert state.status(old.id) == "retired"
    assert [r.entry.id for r in state.entries("claim")] == [new.id]
    assert [r.entry.id for r in state.entries("claim", include_retired=True)] == [old.id, new.id]
    with pytest.raises(TruthError, match="retired"):
        add(state, "objection", "too late", about=old.id)


def test_positions_and_outcomes_must_be_about_decisions():
    state = State()
    claim = add(state, "claim", "A fact.")
    decision = add(state, "decision", "Which recognizer goes first?", options=["Groq", "OpenAI"])
    with pytest.raises(TruthError, match="about a decision"):
        add(state, "position", "Groq", about=claim.id, round=1)
    with pytest.raises(TruthError, match="round"):
        add(state, "position", "Groq", about=decision.id)
    add(state, "position", "Groq", about=decision.id, round=1, confidence=0.7)
    with pytest.raises(TruthError, match="about a decision"):
        add(state, "outcome", "Groq", about=claim.id)
    add(state, "outcome", "Groq", about=decision.id, evidence=["bake-off 2026-10-10"])
    assert state.outcome_of(decision.id).entry.text == "Groq"


def test_ids_are_unique_and_targets_must_exist():
    state = State()
    claim = add(state, "claim", "One.", id="e000001")
    with pytest.raises(TruthError, match="already exists"):
        add(state, "claim", "Two.", id="e000001")
    with pytest.raises(TruthError, match="No entry"):
        add(state, "objection", "About nothing.", about="e-missing")
    with pytest.raises(TruthError, match="No entry"):
        op(state, "verify", "e-missing", evidence=["x"])
    assert claim.id == "e000001"


def test_the_log_on_disk_round_trips_and_refuses_bad_writes(tmp_path: Path):
    log = tmp_path / "truth.jsonl"
    claim = add(log, "claim", "Written to disk.")
    op(log, "verify", claim.id, evidence=["a test"])
    state, problems = truth.load(log)
    assert problems == []
    assert state.status(claim.id) == "verified"
    before = log.read_text()
    with pytest.raises(TruthError):
        op(log, "verify", claim.id, evidence=["twice"])
    assert log.read_text() == before, "a refused write leaves the log untouched"


def test_a_corrupt_log_is_reported_and_blocks_writes(tmp_path: Path):
    log = tmp_path / "truth.jsonl"
    claim = add(log, "claim", "Fine.")
    with log.open("a") as handle:
        handle.write("this is not json\n")
        handle.write('{"op":"verify","time":"t","author":{"role":"x","model":"y"},"target":"nope","evidence":["e"]}\n')
    state, problems = truth.load(log)
    assert len(problems) == 2
    assert "line 2" in problems[0]
    assert "No entry nope" in problems[1]
    assert state.status(claim.id) == "proposed"
    with pytest.raises(TruthError, match="problems"):
        add(log, "claim", "Blocked.")
    assert "problems" in truth.spoken_summary(state, problems)


def test_the_view_shows_live_entries_grouped_and_hides_retired():
    state = State()
    verified = add(state, "claim", "Kokoro runs three times faster than real time on four cores.")
    op(state, "verify", verified.id, evidence=["timing run 2026-09-25"])
    contested = add(state, "claim", "ElevenLabs is the only real-sounding voice.", who="founder@human")
    add(state, "objection", "Kokoro samples sounded real.", who="builder@claude-opus-5", about=contested.id, severity="high")
    retired = add(state, "claim", "Old.")
    op(state, "retire", retired.id, reason="superseded by nothing")
    question = add(state, "question", "Can the founder see the screen?")
    decision = add(state, "decision", "First recognizer?", options=["Groq", "OpenAI"])
    add(state, "outcome", "Groq", about=decision.id, evidence=["bake-off"])
    view = truth.render_view(state)
    assert view.startswith("# Truth\n")
    assert "6 live entries. 1 verified, 1 contested, 1 open questions, 0 open tasks." in view
    assert "## Decisions\n- [" in view and "outcome: Groq" in view
    assert "## Verified\n- [" in view and "evidence: timing run 2026-09-25" in view
    assert "## Contested\n" in view and "objection [" in view
    assert "## Open questions\n" in view and "Can the founder see the screen?" in view
    assert "Old." not in view
    assert "Fine." not in view


def test_the_scoreboard_measures_accuracy_calibration_flips_and_yield():
    state = State()
    d1 = add(state, "decision", "Recognizer?", options=["Groq", "OpenAI"])
    d2 = add(state, "decision", "Voice tier?", options=["system", "cloud"])
    # steady is right alone and stays right; wobbly is right alone and flips; late is wrong then right.
    for who, first, final, conf in (
        ("steady@claude-opus-5", "Groq", "Groq", 0.9),
        ("wobbly@gpt-5", "Groq", "OpenAI", 0.8),
        ("late@gemini", "OpenAI", "Groq", 0.6),
    ):
        add(state, "position", first, who=who, about=d1.id, round=1, confidence=conf, tokens=1000)
        add(state, "position", final, who=who, about=d1.id, round=3, confidence=conf, tokens=1000)
    add(state, "outcome", "Groq", about=d1.id, evidence=["bake-off"])
    # d2 has positions but no outcome yet: it must not count.
    add(state, "position", "system", who="steady@claude-opus-5", about=d2.id, round=1, confidence=0.5)
    # objections: steady 2 upheld of 2, wobbly 0 of 1.
    claim = add(state, "claim", "Something.", who="late@gemini")
    for who, upheld in (("steady@claude-opus-5", True), ("steady@claude-opus-5", True), ("wobbly@gpt-5", False)):
        objection = add(state, "objection", "Hmm.", who=who, about=claim.id)
        op(state, "resolve", objection.id, upheld=upheld, reason="checked")
    op(state, "verify", claim.id, evidence=["proof"])

    scores = truth.scoreboard(state)
    steady, wobbly, late = scores["steady@claude-opus-5"], scores["wobbly@gpt-5"], scores["late@gemini"]
    assert (steady.positions, steady.correct, steady.accuracy) == (1, 1, 1.0)
    assert steady.brier == pytest.approx((0.9 - 1) ** 2)
    assert (steady.conformity_flips, steady.productive_updates) == (0, 0)
    assert (steady.objections_upheld, steady.objections_resolved, steady.objection_yield) == (2, 2, 1.0)
    assert steady.tokens == 2000 and steady.tokens_per_useful == pytest.approx(2000 / 3)

    assert (wobbly.correct, wobbly.conformity_flips, wobbly.productive_updates) == (0, 1, 0)
    assert wobbly.brier == pytest.approx(0.8 ** 2)
    assert wobbly.objection_yield == 0.0

    assert (late.correct, late.conformity_flips, late.productive_updates) == (1, 0, 1)
    assert late.claims_verified == 1

    table = truth.render_scoreboard(scores)
    assert "| steady@claude-opus-5 | 1 | 100% | 0.01 | 0 | 0 | 2/2 | 0 | 667 |" in table
    assert "| wobbly@gpt-5 | 1 | 0% | 0.64 | 1 | 0 | 0/1 | 0 | - |" in table
    assert truth.render_scoreboard({}).startswith("No one has been measured yet")
