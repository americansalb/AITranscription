"""Project management — CRUD, role configuration, provider assignment."""

from fastapi import APIRouter, HTTPException, status
from pydantic import BaseModel, Field

from shared.schemas.collab import RoleConfig, ProviderAssignment, LLMProvider

router = APIRouter()


# --- Request/Response schemas ---

class CreateProjectRequest(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    template: str | None = Field(default=None, description="Role template to start from")


class ProjectResponse(BaseModel):
    id: str
    name: str
    roles: dict[str, RoleConfig]
    owner_id: int
    created_at: str


class UpdateRoleProviderRequest(BaseModel):
    provider: LLMProvider
    model: str


# --- Endpoints ---

@router.post("/", response_model=ProjectResponse, status_code=status.HTTP_201_CREATED)
async def create_project(request: CreateProjectRequest):
    """Create a new collaboration project."""
    # TODO: create project in DB, apply template if specified
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.get("/", response_model=list[ProjectResponse])
async def list_projects():
    """List all projects for the current user."""
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.get("/{project_id}", response_model=ProjectResponse)
async def get_project(project_id: str):
    """Get a specific project by ID."""
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.delete("/{project_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_project(project_id: str):
    """Delete a project and all its data."""
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.put("/{project_id}/roles/{role_slug}/provider")
async def update_role_provider(project_id: str, role_slug: str, request: UpdateRoleProviderRequest):
    """Assign a specific LLM provider and model to a role."""
    # TODO: update role's provider config in DB
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.post("/{project_id}/roles/{role_slug}/start")
async def start_agent(project_id: str, role_slug: str):
    """Start an agent for a role — begins the server-side agent loop."""
    # TODO: spawn agent runtime task for this role
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.post("/{project_id}/roles/{role_slug}/stop")
async def stop_agent(project_id: str, role_slug: str):
    """Stop a running agent."""
    raise HTTPException(status_code=501, detail="Not implemented yet")
