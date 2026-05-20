"""Tests for the agent runtime — message polling, context building, response parsing."""

import pytest

from app.services.agent_runtime import (
    AgentState,
    _build_context,
    _parse_message_segment,
)


# --- Message parsing tests ---


def test_parse_structured_message():
    """Full TO/TYPE/SUBJECT headers are extracted correctly."""
    segment = "TO: manager\nTYPE: handoff\nSUBJECT: Feature complete\n\nDone with implementation."
    result = _parse_message_segment(segment, "developer:0")
    assert result["to"] == "manager"
    assert result["type"] == "handoff"
    assert result["subject"] == "Feature complete"
    assert result["body"] == "Done with implementation."


def test_parse_partial_headers_to_only():
    """Only TO header present."""
    segment = "TO: architect\nHere is the analysis."
    result = _parse_message_segment(segment, "developer:0")
    assert result["to"] == "architect"
    assert result["type"] == "message"
    assert result["subject"] == ""
    assert "analysis" in result["body"]


def test_parse_unstructured_message():
    """No headers at all — falls back to broadcast."""
    segment = "Just a plain text response from the agent."
    result = _parse_message_segment(segment, "developer:0")
    assert result["to"] == "all"
    assert result["type"] == "message"
    assert result["subject"] == ""
    assert result["body"] == "Just a plain text response from the agent."


def test_parse_case_insensitive_headers():
    """Headers are case-insensitive."""
    segment = "to: developer\ntype: directive\nsubject: Do this thing\n\nPlease implement X."
    result = _parse_message_segment(segment, "manager:0")
    assert result["to"] == "developer"
    assert result["type"] == "directive"
    assert result["subject"] == "Do this thing"


def test_parse_subject_with_special_chars():
    """Subject can contain colons, spaces, etc."""
    segment = "TO: all\nSUBJECT: Bug fix: login page (critical)\n\nFixed the issue."
    result = _parse_message_segment(segment, "developer:0")
    assert result["subject"] == "Bug fix: login page (critical)"


# --- Context building tests ---


def test_build_context_user_messages():
    """Messages from other roles become user turns."""
    state = AgentState(project_id="1", role_slug="developer", instance=0)
    messages = [
        {"from": "manager:0", "type": "directive", "subject": "Do X", "body": "Implement feature X"},
    ]
    context = _build_context("", state, messages)
    assert len(context) == 1
    assert context[0]["role"] == "user"
    assert "[manager:0]" in context[0]["content"]
    assert "Implement feature X" in context[0]["content"]


def test_build_context_own_messages_as_assistant():
    """Messages from this agent instance become assistant turns."""
    state = AgentState(project_id="1", role_slug="developer", instance=0)
    messages = [
        {"from": "manager:0", "type": "directive", "subject": "Do X", "body": "Please do X"},
        {"from": "developer:0", "type": "status", "subject": "Working", "body": "On it"},
    ]
    context = _build_context("", state, messages)
    assert len(context) == 2
    assert context[0]["role"] == "user"
    assert context[1]["role"] == "assistant"


def test_build_context_prepends_user_if_starts_with_assistant():
    """If first message is from this agent, a synthetic user message is prepended."""
    state = AgentState(project_id="1", role_slug="developer", instance=0)
    messages = [
        {"from": "developer:0", "type": "status", "subject": "Starting", "body": "Beginning work"},
    ]
    context = _build_context("", state, messages)
    assert len(context) == 2
    assert context[0]["role"] == "user"
    assert "resuming" in context[0]["content"].lower()
    assert context[1]["role"] == "assistant"


def test_build_context_empty_messages():
    """Empty message list returns empty context."""
    state = AgentState(project_id="1", role_slug="developer", instance=0)
    context = _build_context("", state, [])
    assert context == []


def test_build_context_broadcast_is_user():
    """Messages to 'all' from other roles are user turns."""
    state = AgentState(project_id="1", role_slug="developer", instance=0)
    messages = [
        {"from": "architect:0", "type": "broadcast", "subject": "Announcement", "body": "New convention"},
    ]
    context = _build_context("", state, messages)
    assert context[0]["role"] == "user"


def test_build_context_different_instance_is_user():
    """Messages from same role but different instance are user turns."""
    state = AgentState(project_id="1", role_slug="developer", instance=0)
    messages = [
        {"from": "developer:1", "type": "status", "subject": "Done", "body": "I finished my part"},
    ]
    context = _build_context("", state, messages)
    assert context[0]["role"] == "user"
