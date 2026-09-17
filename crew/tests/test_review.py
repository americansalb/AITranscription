import pytest

from crew import review as review_mod
from crew import spec as spec_mod

SPEC_TEXT = """# Spec: example

Status: approved

## Requirements

R1. First thing.
R2. Second thing.
"""


def make_review(**kwargs) -> review_mod.Review:
    return review_mod.Review(**kwargs)


def test_brief_contains_only_the_spec_and_the_diff():
    brief = review_mod.build_brief(SPEC_TEXT, "diff --git a/x b/x")
    assert SPEC_TEXT.strip() in brief.user
    assert "diff --git a/x b/x" in brief.user
    assert "no praise" in brief.system.lower()


def test_brief_refuses_without_a_spec():
    with pytest.raises(ValueError):
        review_mod.build_brief("  ", "diff")


def test_brief_refuses_an_empty_diff():
    with pytest.raises(ValueError):
        review_mod.build_brief(SPEC_TEXT, "")


def test_missing_verdicts_are_named():
    spec = spec_mod.parse(SPEC_TEXT)
    review = make_review(
        requirements=[{"id": "R1", "verdict": "met", "evidence": "x"}]
    )
    assert review_mod.missing_verdicts(spec, review) == ["R2"]


def test_unmet_and_unclear_are_separated():
    review = make_review(
        requirements=[
            {"id": "R1", "verdict": "unmet", "evidence": "missing"},
            {"id": "R2", "verdict": "unclear", "evidence": "cannot tell"},
        ]
    )
    assert [v.id for v in review_mod.unmet(review)] == ["R1"]
    assert [v.id for v in review_mod.unclear(review)] == ["R2"]


def test_only_high_severity_objections_block():
    review = make_review(
        objections=[
            {"severity": "high", "claim": "breaks", "evidence": "line 3"},
            {"severity": "low", "claim": "naming", "evidence": "line 9"},
        ]
    )
    assert len(review_mod.blocking_objections(review)) == 1


def test_spoken_summary_reports_counts():
    spec = spec_mod.parse(SPEC_TEXT)
    review = make_review(
        requirements=[
            {"id": "R1", "verdict": "met", "evidence": "done"},
            {"id": "R2", "verdict": "met", "evidence": "done"},
        ]
    )
    assert review_mod.spoken_summary(spec, review) == "2 of 2 requirements met. no objections."
