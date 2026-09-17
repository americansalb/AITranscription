"""The interpreter.

Its whole job is turning what a person said into something another reader, with
no access to the conversation, can build from and check against. It is the one
artifact that predates every later contamination, which is why the reviewer is
given it word for word rather than a restatement.

It does not do the work and it does not judge the work.
"""

from __future__ import annotations

from pydantic import BaseModel, Field

from .spec import Spec, number_assumptions, number_requirements

INTERPRETER_SYSTEM = """You turn a person's request into a specification that a \
different person, with no access to this conversation, could build from and \
check against.

Write the intent as one paragraph, in the requester's own framing. Do not add \
goals they did not state and do not make their request more ambitious than it is.

Write the requirements as single testable statements. Someone reading the \
finished work must be able to say met or unmet without asking you what you \
meant. Split compound requirements into separate ones. Do not invent \
requirements in order to look thorough.

List what is out of scope wherever the request implies a boundary, so that later \
work has something to be judged against when it drifts.

List the ambiguities that actually change what gets built. For each, state the \
question and the assumption you would make if nobody answers it. Leave out any \
question whose answer would not change the work."""


class DraftSpec(BaseModel):
    title: str = Field(description="A short name for this piece of work.")
    intent: str = Field(description="One paragraph, in the requester's own framing.")
    requirements: list[str] = Field(
        default_factory=list,
        description="Single testable statements, each judgeable as met or unmet.",
    )
    out_of_scope: list[str] = Field(default_factory=list)
    assumptions: list[str] = Field(
        default_factory=list,
        description="Each one states the open question and the assumption made in its absence.",
    )


def build_request(raw_request: str, context: str = "") -> str:
    """The interpreter sees the request as the person wrote it, not a summary."""
    if not raw_request.strip():
        raise ValueError("Refusing to write a spec from an empty request.")

    parts = ["REQUEST, in the requester's own words:", "---", raw_request.strip(), "---"]
    if context.strip():
        parts += [
            "",
            "PROJECT CONTEXT, for vocabulary only. It is not part of the request:",
            "---",
            context.strip(),
            "---",
        ]
    return "\n".join(parts)


def to_spec(draft: DraftSpec) -> Spec:
    return Spec(
        title=draft.title.strip() or "untitled",
        intent=draft.intent.strip(),
        requirements=number_requirements(draft.requirements),
        out_of_scope=[item.strip() for item in draft.out_of_scope if item.strip()],
        assumptions=number_assumptions(draft.assumptions),
    )
