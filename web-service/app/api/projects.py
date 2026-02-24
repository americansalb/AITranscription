"""Project management — CRUD, role configuration, provider assignment, agent control."""

import logging

from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.database import get_db
from app.models import Project, ProjectRole, WebUser
from app.services.agent_runtime import get_active_agents, start_agent, stop_agent

logger = logging.getLogger(__name__)
router = APIRouter()


# --- Default role templates ---

DEFAULT_ROLES = [
    {"slug": "manager", "title": "Project Manager", "provider": "anthropic", "model": "claude-sonnet-4-6"},
    {"slug": "architect", "title": "Architect", "provider": "anthropic", "model": "claude-sonnet-4-6"},
    {"slug": "developer", "title": "Developer", "provider": "anthropic", "model": "claude-sonnet-4-6", "max_instances": 3},
    {"slug": "tester", "title": "Tester", "provider": "anthropic", "model": "claude-haiku-4-5-20251001"},
]


# --- Schemas ---

class CreateProjectRequest(BaseModel):
    name: str = Field(min_length=1, max_length=100)


class UpdateRoleProviderRequest(BaseModel):
    provider: str
    model: str


class CreateRoleRequest(BaseModel):
    slug: str = Field(min_length=1, max_length=50, pattern=r"^[a-z][a-z0-9_-]*$")
    title: str = Field(min_length=1, max_length=100)
    description: str = Field(default="", max_length=5000)
    tags: list[str] = Field(default_factory=list)
    permissions: list[str] = Field(default_factory=list)
    maxInstances: int = Field(default=1, ge=1, le=10)
    provider: dict | None = Field(default=None, description='{"provider": "anthropic", "model": "..."}')


class UpdateBriefingRequest(BaseModel):
    briefing: str = Field(max_length=50000)


class BuzzAgentRequest(BaseModel):
    instance: int = Field(default=0, ge=0)


class InterruptAgentRequest(BaseModel):
    reason: str = Field(min_length=1, max_length=2000)
    instance: int = Field(default=0, ge=0)


# --- Endpoints ---

@router.post("/", status_code=status.HTTP_201_CREATED)
async def create_project(
    request: CreateProjectRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Create a new collaboration project with default roles."""
    project = Project(name=request.name, owner_id=user.id)
    db.add(project)
    await db.flush()

    # Create default roles
    for role_def in DEFAULT_ROLES:
        role = ProjectRole(
            project_id=project.id,
            slug=role_def["slug"],
            title=role_def["title"],
            provider=role_def.get("provider", "anthropic"),
            model=role_def.get("model", "claude-sonnet-4-6"),
            max_instances=role_def.get("max_instances", 1),
        )
        db.add(role)

    await db.commit()
    await db.refresh(project)

    logger.info("Project created: %d '%s' by user %d", project.id, project.name, user.id)
    return _project_response(project)


@router.get("/")
async def list_projects(
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """List all projects owned by the current user."""
    result = await db.execute(
        select(Project)
        .where(Project.owner_id == user.id, Project.is_active == True)
        .order_by(Project.created_at.desc())
    )
    projects = result.scalars().all()
    return [_project_response(p) for p in projects]


@router.get("/{project_id}")
async def get_project(
    project_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Get a specific project."""
    project = await _get_user_project(db, project_id, user.id)
    return _project_response(project)


@router.delete("/{project_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_project(
    project_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Soft-delete a project."""
    project = await _get_user_project(db, project_id, user.id)
    project.is_active = False
    await db.commit()
    logger.info("Project deleted: %d by user %d", project_id, user.id)


@router.put("/{project_id}/roles/{role_slug}/provider")
async def update_role_provider(
    project_id: int,
    role_slug: str,
    request: UpdateRoleProviderRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Assign a specific LLM provider and model to a role."""
    project = await _get_user_project(db, project_id, user.id)
    role = _find_role(project, role_slug)
    role.provider = request.provider
    role.model = request.model
    await db.commit()
    return {"slug": role.slug, "provider": role.provider, "model": role.model}


@router.post("/{project_id}/roles/{role_slug}/start")
async def start_role_agent(
    project_id: int,
    role_slug: str,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Start a server-side agent for a role."""
    project = await _get_user_project(db, project_id, user.id)
    role = _find_role(project, role_slug)

    if role.is_agent_running:
        raise HTTPException(status_code=409, detail=f"Agent for {role_slug} is already running")

    state = await start_agent(
        project_id=str(project.id),
        role_slug=role_slug,
        model=role.model,
        briefing=role.briefing,
        user_id=user.id,
    )
    role.is_agent_running = True
    await db.commit()

    logger.info("Agent started: project=%d role=%s model=%s", project_id, role_slug, role.model)
    return {"status": "started", "role": role_slug, "model": role.model}


@router.post("/{project_id}/roles/{role_slug}/stop")
async def stop_role_agent(
    project_id: int,
    role_slug: str,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Stop a running agent."""
    project = await _get_user_project(db, project_id, user.id)
    role = _find_role(project, role_slug)

    if not role.is_agent_running:
        raise HTTPException(status_code=409, detail=f"Agent for {role_slug} is not running")

    await stop_agent(project_id=str(project.id), role_slug=role_slug)
    role.is_agent_running = False
    await db.commit()

    logger.info("Agent stopped: project=%d role=%s", project_id, role_slug)
    return {"status": "stopped", "role": role_slug}


@router.get("/{project_id}/agents")
async def list_agents(
    project_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """List active agents for a project."""
    await _get_user_project(db, project_id, user.id)  # auth check
    agents = get_active_agents(str(project_id))
    return [
        {
            "role": a.role_slug,
            "instance": a.instance,
            "model": a.model,
            "is_running": a.is_running,
            "context_tokens": a.context_tokens,
        }
        for a in agents
    ]


# --- Role CRUD ---

@router.get("/{project_id}/roles/{role_slug}/briefing")
async def get_role_briefing(
    project_id: int,
    role_slug: str,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Get a role's briefing text."""
    project = await _get_user_project(db, project_id, user.id)
    role = _find_role(project, role_slug)
    return {"briefing": role.briefing or ""}


@router.put("/{project_id}/roles/{role_slug}/briefing")
async def update_role_briefing(
    project_id: int,
    role_slug: str,
    request: UpdateBriefingRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Update a role's briefing text."""
    project = await _get_user_project(db, project_id, user.id)
    role = _find_role(project, role_slug)
    role.briefing = request.briefing
    await db.commit()
    return {"status": "updated", "slug": role_slug}


@router.post("/{project_id}/roles", status_code=status.HTTP_201_CREATED)
async def create_role(
    project_id: int,
    request: CreateRoleRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Create a new role in a project."""
    project = await _get_user_project(db, project_id, user.id)

    # Check for duplicate slug
    for r in project.roles:
        if r.slug == request.slug:
            raise HTTPException(status_code=409, detail=f"Role '{request.slug}' already exists")

    prov = request.provider or {}
    role = ProjectRole(
        project_id=project.id,
        slug=request.slug,
        title=request.title,
        briefing=request.description,
        provider=prov.get("provider", "anthropic"),
        model=prov.get("model", "claude-sonnet-4-6"),
        max_instances=request.maxInstances,
    )
    db.add(role)
    await db.commit()
    await db.refresh(role)

    logger.info("Role created: %s in project %d", request.slug, project_id)
    return {
        "slug": role.slug,
        "title": role.title,
        "description": role.briefing,
        "tags": request.tags,
        "permissions": request.permissions,
        "maxInstances": role.max_instances,
        "provider": {"provider": role.provider, "model": role.model},
    }


@router.delete("/{project_id}/roles/{role_slug}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_role(
    project_id: int,
    role_slug: str,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Delete a role from a project."""
    project = await _get_user_project(db, project_id, user.id)
    role = _find_role(project, role_slug)

    if role.is_agent_running:
        raise HTTPException(status_code=409, detail="Stop the agent before deleting the role")

    await db.delete(role)
    await db.commit()
    logger.info("Role deleted: %s from project %d", role_slug, project_id)


# --- Agent actions ---

@router.post("/{project_id}/roles/{role_slug}/buzz")
async def buzz_role_agent(
    project_id: int,
    role_slug: str,
    request: BuzzAgentRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Send a wake-up buzz to an agent instance."""
    project = await _get_user_project(db, project_id, user.id)
    _find_role(project, role_slug)  # validates role exists

    # Post a buzz message to the board
    from app.models import Message as MsgModel
    msg = MsgModel(
        project_id=project_id,
        from_role=f"human:{user.id}",
        to_role=f"{role_slug}:{request.instance}",
        msg_type="buzz",
        subject="Wake up",
        body=f"Buzz signal sent to {role_slug}:{request.instance}",
    )
    db.add(msg)
    await db.commit()

    logger.info("Buzzed agent %s:%d in project %d", role_slug, request.instance, project_id)
    return {"status": "buzzed", "role": role_slug, "instance": request.instance}


@router.post("/{project_id}/roles/{role_slug}/interrupt")
async def interrupt_role_agent(
    project_id: int,
    role_slug: str,
    request: InterruptAgentRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Send a priority interrupt to an agent instance."""
    project = await _get_user_project(db, project_id, user.id)
    _find_role(project, role_slug)  # validates role exists

    from app.models import Message as MsgModel
    msg = MsgModel(
        project_id=project_id,
        from_role=f"human:{user.id}",
        to_role=f"{role_slug}:{request.instance}",
        msg_type="interrupt",
        subject="Priority Interrupt",
        body=request.reason,
    )
    db.add(msg)
    await db.commit()

    logger.info("Interrupted agent %s:%d in project %d: %s", role_slug, request.instance, project_id, request.reason[:100])
    return {"status": "interrupted", "role": role_slug, "instance": request.instance}


# --- File Claims (stub — no desktop-style file lock system yet) ---

@router.get("/{project_id}/claims")
async def get_file_claims(
    project_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Get active file claims for a project. Stub — returns empty list until claim system is built."""
    await _get_user_project(db, project_id, user.id)
    return []


# --- Sections (lightweight — no ORM, stored as project metadata) ---

@router.get("/{project_id}/sections")
async def list_sections(
    project_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """List sections. Stub — returns default section only."""
    await _get_user_project(db, project_id, user.id)
    return [
        {"slug": "default", "name": "Default", "message_count": 0, "last_activity": None},
    ]


@router.post("/{project_id}/sections", status_code=status.HTTP_201_CREATED)
async def create_section(
    project_id: int,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Create a section. Stub — returns 501 until section system is built."""
    raise HTTPException(status_code=501, detail="Sections not yet implemented in web service")


@router.post("/{project_id}/sections/{slug}/switch")
async def switch_section(
    project_id: int,
    slug: str,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Switch active section. Stub — no-op for default section."""
    await _get_user_project(db, project_id, user.id)
    return {"status": "switched", "slug": slug}


# --- Helpers ---

async def _get_user_project(db: AsyncSession, project_id: int, user_id: int) -> Project:
    """Fetch a project, ensuring it belongs to the user."""
    result = await db.execute(
        select(Project).where(Project.id == project_id, Project.owner_id == user_id)
    )
    project = result.scalar_one_or_none()
    if not project:
        raise HTTPException(status_code=404, detail="Project not found")
    return project


def _find_role(project: Project, role_slug: str) -> ProjectRole:
    for role in project.roles:
        if role.slug == role_slug:
            return role
    raise HTTPException(status_code=404, detail=f"Role '{role_slug}' not found in project")


def _project_response(project: Project) -> dict:
    """Convert Project ORM object to dict matching frontend ProjectResponse interface."""
    roles_dict = {}
    for r in project.roles:
        roles_dict[r.slug] = {
            "title": r.title,
            "description": r.briefing or "",
            "tags": [],
            "permissions": [],
            "maxInstances": r.max_instances,
            "provider": {"provider": r.provider, "model": r.model},
            "is_agent_running": r.is_agent_running,
        }
    return {
        "id": str(project.id),
        "name": project.name,
        "owner_id": project.owner_id,
        "roles": roles_dict,
        "created_at": project.created_at.isoformat(),
    }
