from pathlib import Path

from crew import cli


def run(tmp_path: Path, *argv: str, capsys=None) -> tuple[int, str]:
    code = cli.main(["--root", str(tmp_path), *argv])
    out = capsys.readouterr().out if capsys else ""
    return code, out


def test_a_full_round_trip_through_the_command_line(tmp_path: Path, capsys, monkeypatch):
    monkeypatch.setenv("CREW_AS", "tester@claude-opus-5")
    code, out = run(tmp_path, "truth", "add", "claim", "The capture takes 40 ms.", capsys=capsys)
    assert code == 0 and out.startswith("Added claim e")
    claim_id = out.split()[2].rstrip(".")

    code, out = run(tmp_path, "truth", "add", "objection", "Only on a small window.", "--about", claim_id,
                    "--as", "skeptic@gpt-5", "--severity", "medium", capsys=capsys)
    assert code == 0
    objection_id = out.split()[2].rstrip(".")

    code, out = run(tmp_path, "truth", "check", capsys=capsys)
    assert code == 0 and "1 contested" in out

    code, out = run(tmp_path, "truth", "resolve", objection_id, "--upheld", "--reason", "measured on Chrome: 900 ms", capsys=capsys)
    assert code == 0 and f"{objection_id} is now resolved." in out

    code, out = run(tmp_path, "truth", "verify", claim_id, "--evidence", "timing in CI", capsys=capsys)
    assert code == 0 and f"{claim_id} is now verified." in out

    code, out = run(tmp_path, "truth", "view", capsys=capsys)
    assert code == 0 and "## Verified" in out and "The capture takes 40 ms." in out

    # The view file is regenerated on every write, so git always holds a current one.
    view_file = tmp_path / ".crew" / "truth.md"
    assert view_file.exists() and "The capture takes 40 ms." in view_file.read_text()

    code, out = run(tmp_path, "truth", "score", capsys=capsys)
    assert code == 0 and "| skeptic@gpt-5 |" in out and "1/1" in out


def test_writes_need_an_author(tmp_path: Path, capsys, monkeypatch):
    monkeypatch.delenv("CREW_AS", raising=False)
    code = cli.main(["--root", str(tmp_path), "truth", "add", "claim", "No author."])
    assert code == 2
    assert "who is writing" in capsys.readouterr().err.lower()
    assert not (tmp_path / ".crew" / "truth.jsonl").exists()


def test_a_bad_write_is_refused_with_a_sentence(tmp_path: Path, capsys):
    code = cli.main(["--root", str(tmp_path), "truth", "verify", "e-nope", "--evidence", "x", "--as", "a@b"])
    assert code == 2
    assert "No entry e-nope." in capsys.readouterr().err


def test_check_reports_a_corrupt_log(tmp_path: Path, capsys):
    (tmp_path / ".crew").mkdir()
    (tmp_path / ".crew" / "truth.jsonl").write_text("not json\n")
    code = cli.main(["--root", str(tmp_path), "truth", "check"])
    out = capsys.readouterr().out
    assert code == 1 and "1 problems" in out and "line 1" in out
