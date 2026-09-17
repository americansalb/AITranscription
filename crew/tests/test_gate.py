from crew import gate
from crew import review as review_mod
from crew import spec as spec_mod

APPROVED = """# Spec: example

Status: approved

## Requirements

R1. First thing.
"""

DRAFT = APPROVED.replace("Status: approved", "Status: draft")


def clean_review() -> review_mod.Review:
    return review_mod.Review(
        requirements=[{"id": "R1", "verdict": "met", "evidence": "added in x.py"}]
    )


def test_everything_clean_passes():
    result = gate.evaluate(spec_mod.parse(APPROVED), clean_review(), True)
    assert result.passed
    assert result.failures == []
    assert result.warnings == []


def test_no_spec_fails_immediately():
    result = gate.evaluate(None, clean_review(), True)
    assert not result.passed
    assert "No spec" in result.failures[0]


def test_unapproved_spec_fails():
    result = gate.evaluate(spec_mod.parse(DRAFT), clean_review(), True)
    assert not result.passed
    assert any("not approved" in f for f in result.failures)


def test_no_review_fails():
    result = gate.evaluate(spec_mod.parse(APPROVED), None, True)
    assert not result.passed
    assert any("No review" in f for f in result.failures)


def test_missing_verdict_fails():
    review = review_mod.Review(requirements=[])
    result = gate.evaluate(spec_mod.parse(APPROVED), review, True)
    assert not result.passed
    assert any("no verdict on R1" in f for f in result.failures)


def test_unmet_requirement_fails():
    review = review_mod.Review(
        requirements=[{"id": "R1", "verdict": "unmet", "evidence": "absent"}]
    )
    result = gate.evaluate(spec_mod.parse(APPROVED), review, True)
    assert not result.passed
    assert any("not met: R1" in f for f in result.failures)


def test_high_severity_objection_fails():
    review = clean_review()
    review.objections = [
        review_mod.Objection(severity="high", claim="races", evidence="line 4")
    ]
    result = gate.evaluate(spec_mod.parse(APPROVED), review, True)
    assert not result.passed
    assert any("high severity" in f for f in result.failures)


def test_low_severity_objection_only_warns():
    review = clean_review()
    review.objections = [
        review_mod.Objection(severity="low", claim="naming", evidence="line 4")
    ]
    result = gate.evaluate(spec_mod.parse(APPROVED), review, True)
    assert result.passed
    assert any("lower severity" in w for w in result.warnings)


def test_failing_tests_fail_the_gate():
    result = gate.evaluate(spec_mod.parse(APPROVED), clean_review(), False)
    assert not result.passed
    assert any("test command failed" in f for f in result.failures)


def test_no_test_command_warns_but_passes():
    result = gate.evaluate(spec_mod.parse(APPROVED), clean_review(), None)
    assert result.passed
    assert any("No test command" in w for w in result.warnings)


def test_unanswered_assumptions_warn():
    text = APPROVED + "\n## Assumptions\n\nA1. Which provider? Assumed the system voice.\n"
    result = gate.evaluate(spec_mod.parse(text), clean_review(), True)
    assert result.passed
    assert any("assumptions stand unanswered" in w for w in result.warnings)


def test_every_failure_is_reported_not_just_the_first():
    review = review_mod.Review(
        requirements=[{"id": "R1", "verdict": "unmet", "evidence": "absent"}],
        objections=[review_mod.Objection(severity="high", claim="breaks", evidence="line 1")],
    )
    result = gate.evaluate(spec_mod.parse(DRAFT), review, False)
    assert len(result.failures) == 4
