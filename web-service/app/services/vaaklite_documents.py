"""Vaaklite v1 document drafting service.

Per architect msg 5738 spec lock (human msg 5730 directive). This module
owns the document lifecycle inside a discussion-mode project: creating a
document, applying a section outline, advancing the rotation, accepting
sections, and finalizing the markdown artifact.

All mutations go through the helpers here so the API layer stays a thin
wrapper. The helpers update both the `DocumentSection.body/status` and
the `Document.rendered_markdown` aggregate, keeping reads cheap.
"""

from __future__ import annotations

import logging
from datetime import datetime, timezone

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.models import (
    Document,
    DocumentPhase,
    DocumentSection,
    DocumentSectionStatus,
    DraftingTurn,
    Project,
    ProjectMode,
    ProjectRole,
)

logger = logging.getLogger(__name__)


def _utcnow() -> datetime:
    return datetime.now(timezone.utc)


def render_markdown(document: Document, sections: list[DocumentSection]) -> str:
    """Materialize the document body from its sections.

    Accepted + drafting sections are included; pending sections render as
    "_(pending: <title>)_" placeholders so the reader sees the full
    outline even before drafting completes.
    """
    lines = [f"# {document.title}", ""]
    if document.topic:
        lines.extend([f"_{document.topic}_", ""])
    for section in sorted(sections, key=lambda s: s.idx):
        lines.append(f"## {section.title}")
        lines.append("")
        if section.status is DocumentSectionStatus.PENDING:
            lines.append(f"_(pending — assigned to {section.assigned_role or 'TBD'})_")
        elif section.body:
            lines.append(section.body)
        else:
            lines.append("_(empty)_")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


async def create_document(
    db: AsyncSession,
    *,
    project_id: int,
    title: str,
    topic: str,
    section_outline: list[dict],
) -> Document:
    """Create a new Vaaklite document with its section outline.

    `section_outline` is a list of {"title": str, "assigned_role": str|None}
    dicts in order. The first section becomes `current_section_idx=0` with
    its assigned_role copied to `Document.current_role`.

    Raises ValueError if the project doesn't exist, isn't owned by the
    caller, or isn't in discussion mode.
    """
    project = await db.get(Project, project_id)
    if project is None:
        raise ValueError(f"Project {project_id} not found")
    if project.mode is not ProjectMode.DISCUSSION:
        raise ValueError(
            f"Project {project_id} is in {project.mode.value} mode; "
            "Vaaklite documents require mode=discussion"
        )
    if not section_outline:
        raise ValueError("section_outline must contain at least one section")

    first = section_outline[0]
    document = Document(
        project_id=project_id,
        title=title,
        topic=topic,
        phase=DocumentPhase.DRAFTING,
        current_section_idx=0,
        current_role=first.get("assigned_role"),
    )
    db.add(document)
    await db.flush()

    sections: list[DocumentSection] = []
    for idx, entry in enumerate(section_outline):
        section = DocumentSection(
            document_id=document.id,
            idx=idx,
            title=entry["title"],
            assigned_role=entry.get("assigned_role"),
            status=DocumentSectionStatus.DRAFTING if idx == 0 else DocumentSectionStatus.PENDING,
        )
        db.add(section)
        sections.append(section)

    await db.flush()
    document.rendered_markdown = render_markdown(document, sections)
    await db.commit()
    await db.refresh(document)
    logger.info(
        "Vaaklite document created: id=%d project=%d title=%r sections=%d",
        document.id,
        project_id,
        title,
        len(section_outline),
    )
    return document


async def submit_section_draft(
    db: AsyncSession,
    *,
    document_id: int,
    section_idx: int,
    role_seat: str,
    body: str,
) -> Document:
    """Save a role's draft for a section + log the DraftingTurn.

    Status flips drafting → review_pending. The next mic rotation happens
    via `advance_section` after the review phase (or immediately, depending
    on the project template).
    """
    document = await db.get(Document, document_id)
    if document is None:
        raise ValueError(f"Document {document_id} not found")

    section_result = await db.execute(
        select(DocumentSection)
        .where(DocumentSection.document_id == document_id)
        .where(DocumentSection.idx == section_idx)
    )
    section = section_result.scalar_one_or_none()
    if section is None:
        raise ValueError(f"Section idx={section_idx} not found in document {document_id}")
    if section.status is DocumentSectionStatus.ACCEPTED:
        raise ValueError(f"Section idx={section_idx} already accepted; cannot redraft")

    section.body = body
    section.status = DocumentSectionStatus.REVIEW_PENDING
    section.updated_at = _utcnow()

    turn = DraftingTurn(
        document_id=document_id,
        section_idx=section_idx,
        role_seat=role_seat,
        output_body=body,
        completed_at=_utcnow(),
    )
    db.add(turn)

    # Refresh the rendered markdown aggregate
    all_sections_result = await db.execute(
        select(DocumentSection).where(DocumentSection.document_id == document_id)
    )
    all_sections = list(all_sections_result.scalars().all())
    document.rendered_markdown = render_markdown(document, all_sections)
    document.updated_at = _utcnow()

    await db.commit()
    await db.refresh(document)
    return document


async def accept_section(
    db: AsyncSession,
    *,
    document_id: int,
    section_idx: int,
) -> Document:
    """Accept a reviewed section and advance the mic to the next pending one.

    Flips status review_pending → accepted, picks the next PENDING section
    (in order), sets it DRAFTING + updates Document.current_section_idx/
    current_role. If no pending sections remain, the document phase moves
    to REVIEW (giving the team a chance for cross-section revisions before
    finalize).
    """
    document = await db.get(Document, document_id)
    if document is None:
        raise ValueError(f"Document {document_id} not found")

    section_result = await db.execute(
        select(DocumentSection)
        .where(DocumentSection.document_id == document_id)
        .where(DocumentSection.idx == section_idx)
    )
    section = section_result.scalar_one_or_none()
    if section is None:
        raise ValueError(f"Section idx={section_idx} not found in document {document_id}")
    if section.status is not DocumentSectionStatus.REVIEW_PENDING:
        raise ValueError(
            f"Section idx={section_idx} status={section.status.value}; "
            "only review_pending sections can be accepted"
        )

    section.status = DocumentSectionStatus.ACCEPTED
    section.review_notes = None
    section.updated_at = _utcnow()

    # Find next pending section
    all_sections_result = await db.execute(
        select(DocumentSection)
        .where(DocumentSection.document_id == document_id)
        .order_by(DocumentSection.idx)
    )
    all_sections = list(all_sections_result.scalars().all())
    next_pending = next(
        (s for s in all_sections if s.status is DocumentSectionStatus.PENDING),
        None,
    )
    if next_pending is not None:
        next_pending.status = DocumentSectionStatus.DRAFTING
        next_pending.updated_at = _utcnow()
        document.current_section_idx = next_pending.idx
        document.current_role = next_pending.assigned_role
    else:
        # All sections accepted — move to REVIEW phase. The team can
        # still hit a section's review path via revise_section.
        document.phase = DocumentPhase.REVIEW
        document.current_section_idx = None
        document.current_role = None

    document.rendered_markdown = render_markdown(document, all_sections)
    document.updated_at = _utcnow()

    await db.commit()
    await db.refresh(document)
    return document


async def finalize_document(db: AsyncSession, *, document_id: int) -> Document:
    """Lock the document into FINAL phase + freeze the markdown."""
    document = await db.get(Document, document_id)
    if document is None:
        raise ValueError(f"Document {document_id} not found")
    if document.phase is DocumentPhase.FINAL:
        return document
    document.phase = DocumentPhase.FINAL
    document.final_markdown = document.rendered_markdown
    document.finalized_at = _utcnow()
    document.updated_at = _utcnow()
    await db.commit()
    await db.refresh(document)
    return document


def _build_drafting_prompt(
    document: Document,
    section: DocumentSection,
    all_sections: list[DocumentSection],
    role_title: str,
) -> tuple[str, str]:
    """Build the (system, user) prompt pair for an LLM drafting turn.

    The system prompt establishes the role identity; the user prompt
    carries the document context — title, topic, the full outline (so the
    agent knows where its section sits), and the text of already-accepted
    sections so the new section reads coherently with what came before.
    """
    system = (
        f"You are the {role_title} on an AI team collaboratively drafting a "
        "document, one section at a time. Write focused, clear, well-structured "
        "markdown prose. Stay strictly within the scope of the section you are "
        "assigned — other team members own the other sections."
    )

    lines: list[str] = [f"Document title: {document.title}"]
    if document.topic:
        lines.append(f"Topic / brief: {document.topic}")
    lines.append("")
    lines.append("Full outline:")
    for s in sorted(all_sections, key=lambda x: x.idx):
        if s.idx == section.idx:
            marker = "  <-- YOUR SECTION"
        elif s.status is DocumentSectionStatus.ACCEPTED:
            marker = " (done)"
        else:
            marker = ""
        lines.append(f"  {s.idx + 1}. {s.title}{marker}")
    lines.append("")

    accepted = [
        s
        for s in sorted(all_sections, key=lambda x: x.idx)
        if s.status is DocumentSectionStatus.ACCEPTED and s.body
    ]
    if accepted:
        lines.append("Sections already written (for continuity — do not repeat them):")
        lines.append("")
        for s in accepted:
            lines.append(f"### {s.title}")
            lines.append(s.body)
            lines.append("")

    lines.append(f'Your task: draft the section titled "{section.title}".')
    lines.append(
        "Write 2-4 paragraphs of clear markdown prose. Output ONLY the section "
        "body — do not repeat the section heading, and do not write any other "
        "section."
    )
    return system, "\n".join(lines)


async def draft_current_section(
    db: AsyncSession,
    *,
    document_id: int,
    completion_fn,
) -> Document:
    """Have the current section's assigned role draft it via an LLM call.

    `completion_fn` is an async callable `(model, system, prompt) -> str`.
    Production passes a wrapper around the metered provider proxy; tests
    inject a deterministic fake. This keeps the service layer pure and
    fully testable while the v1 ship path uses a real LLM (architect
    ruling msg 5793 — no mock LLM as the v1 ship target).

    Drafting only applies in the DRAFTING phase to the section the mic
    currently points at. The generated body is persisted via
    `submit_section_draft`, which flips the section to review_pending and
    logs a DraftingTurn.
    """
    document = await db.get(Document, document_id)
    if document is None:
        raise ValueError(f"Document {document_id} not found")
    if document.phase is not DocumentPhase.DRAFTING:
        raise ValueError(
            f"Document {document_id} phase={document.phase.value}; "
            "agent drafting requires the DRAFTING phase"
        )
    if document.current_section_idx is None:
        raise ValueError(f"Document {document_id} has no current section to draft")

    section_result = await db.execute(
        select(DocumentSection)
        .where(DocumentSection.document_id == document_id)
        .where(DocumentSection.idx == document.current_section_idx)
    )
    section = section_result.scalar_one_or_none()
    if section is None:
        raise ValueError(
            f"Current section idx={document.current_section_idx} missing from "
            f"document {document_id}"
        )
    if section.status is not DocumentSectionStatus.DRAFTING:
        raise ValueError(
            f"Section idx={section.idx} status={section.status.value}; "
            "only a section in DRAFTING status can be agent-drafted"
        )

    role_slug = section.assigned_role
    role_title = role_slug or "Writer"
    model = "claude-sonnet-4-6"
    if role_slug:
        role_result = await db.execute(
            select(ProjectRole)
            .where(ProjectRole.project_id == document.project_id)
            .where(ProjectRole.slug == role_slug)
        )
        role = role_result.scalar_one_or_none()
        if role is not None:
            model = role.model or model
            role_title = role.title or role_slug

    all_sections_result = await db.execute(
        select(DocumentSection).where(DocumentSection.document_id == document_id)
    )
    all_sections = list(all_sections_result.scalars().all())

    system, prompt = _build_drafting_prompt(document, section, all_sections, role_title)
    body = await completion_fn(model, system, prompt)
    if not isinstance(body, str) or not body.strip():
        raise ValueError("LLM returned an empty draft")

    logger.info(
        "Vaaklite agent draft: doc=%d section=%d role=%s model=%s chars=%d",
        document_id,
        section.idx,
        role_slug,
        model,
        len(body),
    )
    return await submit_section_draft(
        db,
        document_id=document_id,
        section_idx=section.idx,
        role_seat=role_slug or "writer",
        body=body.strip(),
    )


def section_outline_from_template(template_slug: str | None, roles: list[ProjectRole]) -> list[dict]:
    """Generate a default section outline based on the template + roster.

    For `simple-rotation`: one section per non-moderator role.
    For `delphi-debate`: Intro/Statement/Round 1/Synthesis sections.
    For `oxford-review`: Motion/Case For/Case Against/Verdict sections.
    Unknown templates fall through to simple-rotation.
    """
    non_moderator = [r for r in roles if r.slug != "moderator"]
    if template_slug == "delphi-debate":
        return [
            {"title": "Introduction", "assigned_role": "moderator"},
            {"title": "Problem Statement", "assigned_role": next((r.slug for r in non_moderator), None)},
            {"title": "Expert Round 1", "assigned_role": next((r.slug for r in non_moderator if r.slug == "expert"), None)},
            {"title": "Synthesis", "assigned_role": next((r.slug for r in non_moderator if r.slug == "synthesizer"), None)},
        ]
    if template_slug == "oxford-review":
        return [
            {"title": "Motion", "assigned_role": "moderator"},
            {"title": "Case For", "assigned_role": "proponent"},
            {"title": "Case Against", "assigned_role": "opponent"},
            {"title": "Verdict", "assigned_role": "judge"},
        ]
    # simple-rotation default
    return [
        {"title": f"Section {idx + 1}", "assigned_role": role.slug}
        for idx, role in enumerate(non_moderator or roles)
    ]
