"""Project management — CRUD, role configuration, provider assignment, agent control."""

import logging

from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.database import get_db
from app.models import Message, Project, ProjectRole, WebUser
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


class RoleResponse(BaseModel):
    slug: str
    title: str
    provider: str
    model: str
    max_instances: int
    is_agent_running: bool


class ProjectResponse(BaseModel):
    id: int
    name: str
    owner_id: int
    is_active: bool
    roles: list[RoleResponse]
    created_at: str


class UpdateRoleProviderRequest(BaseModel):
    provider: str
    model: str


# --- Endpoints ---

@router.post("/", response_model=ProjectResponse, status_code=status.HTTP_201_CREATED)
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


@router.get("/", response_model=list[ProjectResponse])
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


@router.get("/{project_id}", response_model=ProjectResponse)
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


def _project_response(project: Project) -> ProjectResponse:
    return ProjectResponse(
        id=project.id,
        name=project.name,
        owner_id=project.owner_id,
        is_active=project.is_active,
        roles=[
            RoleResponse(
                slug=r.slug,
                title=r.title,
                provider=r.provider,
                model=r.model,
                max_instances=r.max_instances,
                is_agent_running=r.is_agent_running,
            )
            for r in project.roles
        ],
        created_at=project.created_at.isoformat(),
    )
