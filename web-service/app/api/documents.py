"""Vaaklite v1 document drafting API endpoints.

Per architect msg 5738 spec lock. All endpoints scoped to a project the
caller owns; mode=discussion enforced at the service layer.
"""

from __future__ import annotations

import logging

from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.database import get_db
from app.models import (
    Document,
    DocumentPhase,
    DocumentSection,
    DocumentSectionStatus,
    Project,
    ProjectMode,
    ProjectRole,
    WebUser,
)
from app.services.provider_proxy import proxy_completion
from app.services.vaaklite_documents import (
    accept_section,
    create_document,
    draft_current_section,
    finalize_document,
    section_outline_from_template,
    submit_section_draft,
)

logger = logging.getLogger(__name__)
router = APIRouter()


# --- Schemas ---


class SectionOutlineEntry(BaseModel):
    title: str = Field(min_length=1, max_length=200)
    assigned_role: str | None = Field(default=None, max_length=100)


class CreateDocumentRequest(BaseModel):
    title: str = Field(min_length=1, max_length=200)
    topic: str = Field(default="", max_length=5000)
    # Optional explicit outline. When omitted, the service auto-derives
    # an outline from the project template + role roster.
    sections: list[SectionOutlineEntry] | None = Field(default=None)


class SubmitSectionRequest(BaseModel):
    section_idx: int = Field(ge=0)
    role_seat: str = Field(min_length=1, max_length=100)
    body: str = Field(default="", max_length=200_000)


class AcceptSectionRequest(BaseModel):
    section_idx: int = Field(ge=0)


# --- Completion dependency ---


async def get_completion_fn(user: WebUser = Depends(get_current_user)):
    """Provide the LLM completion callable used for agent drafting.

    Production wires this to the metered provider proxy (real LLM via
    LiteLLM) per architect ruling msg 5793. Tests override this FastAPI
    dependency with a deterministic fake so CI never makes a network call.
    """

    async def _completion(model: str, system: str, prompt: str) -> str:
        result = await proxy_completion(
            user_id=user.id,
            model=model,
            messages=[{"role": "user", "content": prompt}],
            system=system,
            max_tokens=2000,
            timeout=120.0,
        )
        return result.content

    return _completion


# --- Endpoints ---


@router.post("/{project_id}/documents", status_code=status.HTTP_201_CREATED)
async def create_project_document(
    project_id: int,
    request: CreateDocumentRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Create a new Vaaklite document under a discussion-mode project.

    If `request.sections` is omitted, the section outline is generated
    from the project template + role roster via
    `section_outline_from_template`.
    """
    project = await _get_owned_project(db, project_id, user.id)
    if project.mode is not ProjectMode.DISCUSSION:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Project {project_id} is in {project.mode.value} mode; "
            "Vaaklite documents require mode=discussion",
        )

    if request.sections:
        outline = [s.model_dump() for s in request.sections]
    else:
        # Auto-derive from template + roster
        roles_result = await db.execute(
            select(ProjectRole).where(ProjectRole.project_id == project_id)
        )
        roles = list(roles_result.scalars().all())
        outline = section_outline_from_template(project.template, roles)
        if not outline:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Cannot auto-derive sections — no roles in roster. Add roles or supply `sections` explicitly.",
            )

    try:
        document = await create_document(
            db,
            project_id=project_id,
            title=request.title,
            topic=request.topic,
            section_outline=outline,
        )
    except ValueError as exc:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail=str(exc))
    return await _document_response(db, document)


@router.get("/{project_id}/documents")
async def list_project_documents(
    project_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """List all documents in a project."""
    await _get_owned_project(db, project_id, user.id)
    result = await db.execute(
        select(Document)
        .where(Document.project_id == project_id)
        .order_by(Document.created_at.desc())
    )
    documents = result.scalars().all()
    return [await _document_response(db, doc) for doc in documents]


@router.get("/{project_id}/documents/{document_id}")
async def get_project_document(
    project_id: int,
    document_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Fetch a single document with its sections."""
    await _get_owned_project(db, project_id, user.id)
    document = await db.get(Document, document_id)
    if document is None or document.project_id != project_id:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Document not found")
    return await _document_response(db, document)


@router.post("/{project_id}/documents/{document_id}/submit")
async def submit_section(
    project_id: int,
    document_id: int,
    request: SubmitSectionRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Submit a section draft on behalf of a role.

    The web client (or an agent runtime) calls this when a role has
    finished its drafting turn. Flips the section to review_pending and
    appends a DraftingTurn row.
    """
    await _get_owned_project(db, project_id, user.id)
    document = await db.get(Document, document_id)
    if document is None or document.project_id != project_id:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Document not found")
    try:
        updated = await submit_section_draft(
            db,
            document_id=document_id,
            section_idx=request.section_idx,
            role_seat=request.role_seat,
            body=request.body,
        )
    except ValueError as exc:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail=str(exc))
    return await _document_response(db, updated)


@router.post("/{project_id}/documents/{document_id}/draft-current")
async def draft_current_section_endpoint(
    project_id: int,
    document_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
    completion_fn=Depends(get_completion_fn),
):
    """Have the current section's assigned role draft it via a real LLM call.

    This is the "agents take turns drafting sections" capability (spec
    smoke item 6). The mic's current section is drafted by its assigned
    role's configured model, then submitted — flipping it to
    review_pending. The caller advances the rotation with /accept.
    """
    project = await _get_owned_project(db, project_id, user.id)
    if project.mode is not ProjectMode.DISCUSSION:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Project {project_id} is in {project.mode.value} mode; "
            "Vaaklite documents require mode=discussion",
        )
    document = await db.get(Document, document_id)
    if document is None or document.project_id != project_id:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Document not found")
    try:
        updated = await draft_current_section(
            db, document_id=document_id, completion_fn=completion_fn
        )
    except ValueError as exc:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail=str(exc))
    return await _document_response(db, updated)


@router.post("/{project_id}/documents/{document_id}/accept")
async def accept_section_endpoint(
    project_id: int,
    document_id: int,
    request: AcceptSectionRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Accept a reviewed section + advance the mic to the next pending section."""
    await _get_owned_project(db, project_id, user.id)
    document = await db.get(Document, document_id)
    if document is None or document.project_id != project_id:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Document not found")
    try:
        updated = await accept_section(
            db, document_id=document_id, section_idx=request.section_idx
        )
    except ValueError as exc:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail=str(exc))
    return await _document_response(db, updated)


@router.post("/{project_id}/documents/{document_id}/finalize")
async def finalize_document_endpoint(
    project_id: int,
    document_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Lock the document into FINAL phase + freeze the markdown."""
    await _get_owned_project(db, project_id, user.id)
    document = await db.get(Document, document_id)
    if document is None or document.project_id != project_id:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Document not found")
    updated = await finalize_document(db, document_id=document_id)
    return await _document_response(db, updated)


@router.get("/{project_id}/documents/{document_id}/markdown")
async def download_document_markdown(
    project_id: int,
    document_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Return the rendered (or final) markdown body for download."""
    await _get_owned_project(db, project_id, user.id)
    document = await db.get(Document, document_id)
    if document is None or document.project_id != project_id:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Document not found")
    body = document.final_markdown if document.phase is DocumentPhase.FINAL else document.rendered_markdown
    return {
        "document_id": document_id,
        "title": document.title,
        "phase": document.phase.value,
        "markdown": body or "",
    }


# --- Helpers ---


async def _get_owned_project(db: AsyncSession, project_id: int, user_id: int) -> Project:
    project = await db.get(Project, project_id)
    if project is None or project.owner_id != user_id:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND, detail=f"Project {project_id} not found"
        )
    return project


async def _document_response(db: AsyncSession, document: Document) -> dict:
    sections_result = await db.execute(
        select(DocumentSection)
        .where(DocumentSection.document_id == document.id)
        .order_by(DocumentSection.idx)
    )
    sections = list(sections_result.scalars().all())
    return {
        "id": document.id,
        "project_id": document.project_id,
        "title": document.title,
        "topic": document.topic,
        "phase": document.phase.value,
        "current_section_idx": document.current_section_idx,
        "current_role": document.current_role,
        "rendered_markdown": document.rendered_markdown,
        "final_markdown": document.final_markdown,
        "finalized_at": document.finalized_at.isoformat() if document.finalized_at else None,
        "created_at": document.created_at.isoformat(),
        "updated_at": document.updated_at.isoformat(),
        "sections": [
            {
                "id": s.id,
                "idx": s.idx,
                "title": s.title,
                "assigned_role": s.assigned_role,
                "body": s.body,
                "status": s.status.value,
                "review_notes": s.review_notes,
                "updated_at": s.updated_at.isoformat(),
            }
            for s in sections
        ],
    }
