import json
from pathlib import Path

import pytest

from crew import cli, providers
from tests.test_deliberate import Scripted, groq_vs_openai_script

SEAT_ARGS = ["--seat", "analyst@anthropic", "--seat", "skeptic@groq", "--seat", "advocate@anthropic"]


def test_dry_run_asks_nothing_and_writes_nothing(tmp_path: Path, capsys, monkeypatch):
    def explode(*args, **kwargs):
        raise AssertionError("a dry run must not call a provider")

    monkeypatch.setattr(providers, "ask", explode)
    code = cli.main(["--root", str(tmp_path), "deliberate", "Which recognizer goes first?", "--option", "Groq", "--option", "OpenAI", *SEAT_ARGS, "--dry-run"])
    out = capsys.readouterr().out
    assert code == 0
    assert "Seats: analyst on anthropic (claude-opus-5), skeptic on groq (openai/gpt-oss-120b), advocate on anthropic (claude-opus-5)" in out
    assert "Role: analyst." in out and "Question: Which recognizer goes first?" in out
    assert "Dry run." in out
    assert not (tmp_path / ".crew").exists()


def test_a_full_run_decides_and_records(tmp_path: Path, capsys, monkeypatch):
    monkeypatch.setattr(providers, "ask", Scripted(groq_vs_openai_script()))
    code = cli.main(["--root", str(tmp_path), "deliberate", "Which recognizer goes first?", "--option", "Groq", "--option", "OpenAI", *SEAT_ARGS])
    out = capsys.readouterr().out
    assert code == 0, out
    assert "Decided: Groq. Weighted agreement 67% across 3 seats. 2 objections, 1 upheld." in out
    assert "skeptic on groq: OpenAI, confidence 60%" in out
    assert "objection by skeptic against" in out and "(high, upheld)" in out and "(high, rejected)" in out
    assert "warning: A high objection against the chosen option was rejected by its holder" in out
    assert "Recorded as e" in out

    view = (tmp_path / ".crew" / "truth.md").read_text()
    assert "## Decisions" in view and "decided: Groq" in view
    records = list((tmp_path / ".crew" / "deliberations").glob("*.json"))
    assert len(records) == 1
    record = json.loads(records[0].read_text())
    assert record["decided"] == "Groq" and record["seed"] >= 0 and len(record["seats"]) == 3
    assert record["decision_id"].startswith("e")


def test_no_decision_exits_one_but_still_records(tmp_path: Path, capsys, monkeypatch):
    monkeypatch.setattr(providers, "ask", Scripted(groq_vs_openai_script()))
    code = cli.main(["--root", str(tmp_path), "deliberate", "Which recognizer goes first?", "--option", "Groq", "--option", "OpenAI", "--seat", "analyst@anthropic", "--seat", "skeptic@groq"])
    out = capsys.readouterr().out
    assert code == 1
    assert out.count("No decision.") == 1 and "The crux:" in out
    assert "Which recognizer goes first?" in (tmp_path / ".crew" / "truth.md").read_text()


def test_the_council_file_sets_seats_and_roles(tmp_path: Path, capsys, monkeypatch):
    (tmp_path / ".crew").mkdir()
    (tmp_path / ".crew" / "council.json").write_text(json.dumps({
        "seats": ["analyst@anthropic", "skeptic@groq:llama-3.3-70b-versatile", "advocate@anthropic"],
        "roles": {"advocate": "You speak for interpreters working in hospitals."},
    }))
    asker = Scripted(groq_vs_openai_script())
    monkeypatch.setattr(providers, "ask", asker)
    code = cli.main(["--root", str(tmp_path), "deliberate", "Q?", "--option", "Groq", "--option", "OpenAI"])
    assert code == 0
    skeptic = next(c for c in asker.calls if c["role"] == "skeptic")
    assert skeptic["model"] == "llama-3.3-70b-versatile"
    advocate = next(c for c in asker.calls if c["role"] == "advocate")
    assert "interpreters working in hospitals" in advocate["system"]


def test_the_truth_view_is_given_to_every_seat_as_context(tmp_path: Path, monkeypatch):
    monkeypatch.setenv("CREW_AS", "founder@human")
    assert cli.main(["--root", str(tmp_path), "truth", "add", "claim", "Groq bills a ten second minimum."]) == 0
    asker = Scripted(groq_vs_openai_script())
    monkeypatch.setattr(providers, "ask", asker)
    cli.main(["--root", str(tmp_path), "deliberate", "Q?", "--option", "Groq", "--option", "OpenAI", *SEAT_ARGS])
    first_round = [c for c in asker.calls if c["kind"] == "Position"]
    assert all("Groq bills a ten second minimum." in c["user"] for c in first_round)
    assert all("The shared truth so far:" in c["user"] for c in first_round)
