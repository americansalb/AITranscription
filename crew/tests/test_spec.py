import pytest

from crew import spec as spec_mod


SAMPLE = """# Spec: read the final message aloud

Status: approved

## Intent

When Claude Code finishes a turn, I want to hear what it did without looking.

## Requirements

R1. The Stop hook speaks the final assistant message.
R2. Pressing the talk hotkey stops playback within 200 milliseconds.

## Out of scope

- Choosing a voice from a settings window

## Assumptions

A1. Which voice provider? Assumed the system voice, because it needs no key.
"""


def test_parses_every_section():
    spec = spec_mod.parse(SAMPLE)
    assert spec.title == "read the final message aloud"
    assert spec.status == "approved"
    assert spec.is_approved
    assert spec.intent.startswith("When Claude Code finishes")
    assert spec.requirement_ids == ["R1", "R2"]
    assert spec.requirements[1].text.endswith("200 milliseconds.")
    assert spec.out_of_scope == ["Choosing a voice from a settings window"]
    assert [a.id for a in spec.assumptions] == ["A1"]


def test_render_then_parse_is_stable():
    once = spec_mod.parse(SAMPLE)
    twice = spec_mod.parse(spec_mod.render(once))
    assert twice == once


def test_draft_is_not_approved():
    text = SAMPLE.replace("Status: approved", "Status: draft")
    assert not spec_mod.parse(text).is_approved


def test_missing_status_defaults_to_draft():
    text = SAMPLE.replace("Status: approved\n", "")
    spec = spec_mod.parse(text)
    assert spec.status == spec_mod.DRAFT
    assert not spec.is_approved


def test_empty_spec_is_rejected():
    with pytest.raises(spec_mod.SpecError):
        spec_mod.parse("   \n  ")


def test_spec_without_title_is_rejected():
    with pytest.raises(spec_mod.SpecError):
        spec_mod.parse("## Requirements\n\nR1. Something.\n")


def test_duplicate_requirement_ids_are_rejected():
    text = SAMPLE.replace(
        "R2. Pressing the talk hotkey stops playback within 200 milliseconds.",
        "R1. Pressing the talk hotkey stops playback within 200 milliseconds.",
    )
    with pytest.raises(spec_mod.SpecError):
        spec_mod.parse(text)


def test_numbering_skips_blank_entries():
    numbered = spec_mod.number_requirements(["first", "  ", "second"])
    assert [(r.id, r.text) for r in numbered] == [("R1", "first"), ("R2", "second")]


def test_spoken_summary_counts_everything():
    summary = spec_mod.parse(SAMPLE).spoken_summary()
    assert "2 requirements" in summary
    assert "1 assumptions" in summary


def test_assumption_numbering_is_contiguous():
    numbered = spec_mod.number_assumptions(["", "only one"])
    assert [(a.id, a.text) for a in numbered] == [("A1", "only one")]
