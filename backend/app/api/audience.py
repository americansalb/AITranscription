"""Audience voting API for Oxford-style debates."""

import logging
import re
from typing import Optional

from fastapi import APIRouter, HTTPException, Query
from pydantic import BaseModel, Field

from app.services.audience import (
    collect_audience_votes,
    list_pools,
    load_pool,
    save_pool,
    delete_pool,
)

logger = logging.getLogger(__name__)
router = APIRouter(tags=["audience"])


# ---------------------------------------------------------------------------
# Request / Response models
# ---------------------------------------------------------------------------

class AudienceVoteRequest(BaseModel):
    topic: str = Field(..., description="The debate proposition")
    arguments: str = Field("", description="Concatenated debate arguments (empty for pre-vote)")
    phase: str = Field("post", description="'pre' (before arguments) or 'post' (after arguments)")
    providers: Optional[list[str]] = Field(
        None, description="Filter providers: ['groq', 'openai', 'anthropic']. None = all."
    )
    pool: Optional[str] = Field(
        None, description="Audience pool ID: 'general', 'software-dev', 'ai-ml', 'law', or custom. Defaults to 'general'."
    )


class VoteResult(BaseModel):
    persona: str
    background: str
    provider: str
    pool: str = ""
    model: str
    vote: str
    rationale: str
    latency_ms: int
    error: Optional[str] = None


class AudienceVoteResponse(BaseModel):
    topic: str
    phase: str
    pool: str
    pool_name: str = ""
    total_voters: int
    tally: dict[str, int]
    tally_by_provider: dict[str, dict[str, int]]
    tally_by_pool: Optional[dict[str, dict[str, int]]] = None
    votes: list[VoteResult]
    total_latency_ms: int


class PersonaInput(BaseModel):
    name: str
    background: str
    values: str
    style: str
    provider: str


class CreatePoolRequest(BaseModel):
    id: str = Field(..., description="Pool ID (slug, e.g., 'illinois-legislature')")
    name: str = Field(..., description="Display name")
    description: str = Field("", description="Pool description")
    personas: list[PersonaInput] = Field(..., description="List of 27 personas (9 per provider)")


# ---------------------------------------------------------------------------
# Voting endpoint
# ---------------------------------------------------------------------------

@router.post("/audience/vote", response_model=AudienceVoteResponse)
async def audience_vote(req: AudienceVoteRequest):
    """Collect votes from AI audience members across 3 LLM providers."""
    logger.info(f"Audience vote requested: topic='{req.topic[:80]}', phase={req.phase}, pool={req.pool}")
    try:
        result = await collect_audience_votes(
            topic=req.topic,
            arguments=req.arguments,
            phase=req.phase,
            providers=req.providers,
            pool=req.pool,
        )
    except ValueError as e:
        logger.error("Audience vote ValueError: %s", e)
        raise HTTPException(status_code=400, detail="Invalid vote request")
    if "error" in result and not result.get("votes"):
        raise HTTPException(status_code=400, detail="Vote collection failed")
    logger.info(
        f"Audience vote complete: {result['tally']} in {result['total_latency_ms']}ms"
    )
    return result


# ---------------------------------------------------------------------------
# Pool management endpoints
# ---------------------------------------------------------------------------

@router.get("/audience/pools")
async def get_pools():
    """List all available audience pools with metadata."""
    return list_pools()


@router.get("/audience/pools/{pool_id}")
async def get_pool(pool_id: str):
    """Get a specific pool with all personas."""
    try:
        pool = load_pool(pool_id)
    except ValueError as e:
        logger.error("Pool load error for '%s': %s", pool_id, e)
        raise HTTPException(status_code=400, detail="Invalid pool request")
    if not pool:
        raise HTTPException(status_code=404, detail=f"Pool '{pool_id}' not found")
    return {
        "id": pool.id,
        "name": pool.name,
        "description": pool.description,
        "builtin": pool.builtin,
        "member_count": pool.member_count,
        "providers": pool.providers,
        "personas": [
            {
                "name": p.name,
                "background": p.background,
                "values": p.values,
                "style": p.style,
                "provider": p.provider,
            }
            for p in pool.personas
        ],
    }


@router.post("/audience/pools")
async def create_pool(req: CreatePoolRequest):
    """Create a custom audience pool."""
    # Validate ID format
    if not re.match(r'^[a-z0-9][a-z0-9-]*$', req.id):
        raise HTTPException(status_code=400, detail="Pool ID must be lowercase alphanumeric with hyphens (e.g., 'my-custom-pool')")

    # Check for duplicates
    existing = load_pool(req.id)
    if existing:
        raise HTTPException(status_code=409, detail=f"Pool '{req.id}' already exists")

    # Validate provider distribution
    providers = {p.provider for p in req.personas}
    for prov in providers:
        if prov not in ("groq", "openai", "anthropic"):
            raise HTTPException(status_code=400, detail=f"Invalid provider: {prov}")

    pool_data = {
        "id": req.id,
        "name": req.name,
        "description": req.description,
        "builtin": False,
        "personas": [
            {
                "name": p.name,
                "background": p.background,
                "values": p.values,
                "style": p.style,
                "provider": p.provider,
            }
            for p in req.personas
        ],
    }
    save_pool(pool_data)
    return {"id": req.id, "name": req.name, "member_count": len(req.personas)}


@router.delete("/audience/pools/{pool_id}")
async def remove_pool(pool_id: str):
    """Delete a custom audience pool. Built-in pools cannot be deleted."""
    try:
        deleted = delete_pool(pool_id)
    except ValueError as e:
        logger.error("Pool delete error for '%s': %s", pool_id, e)
        raise HTTPException(status_code=400, detail="Invalid pool request")
    if not deleted:
        # Check if it exists but is builtin
        pool = load_pool(pool_id)
        if pool and pool.builtin:
            raise HTTPException(status_code=403, detail="Cannot delete built-in pools")
        raise HTTPException(status_code=404, detail=f"Pool '{pool_id}' not found")
    return {"deleted": pool_id}


# ---------------------------------------------------------------------------
# Persona listing (backward-compatible)
# ---------------------------------------------------------------------------

@router.get("/audience/personas")
async def list_personas(pool: Optional[str] = Query(None, description="Filter by pool ID")):
    """List audience personas, optionally filtered by pool."""
    pool_id = pool or "general"
    try:
        audience_pool = load_pool(pool_id)
    except ValueError as e:
        logger.error("Persona list error for pool '%s': %s", pool_id, e)
        raise HTTPException(status_code=400, detail="Invalid pool request")
    if not audience_pool:
        raise HTTPException(status_code=404, detail=f"Pool '{pool_id}' not found")
    return [
        {
            "name": p.name,
            "background": p.background,
            "values": p.values,
            "style": p.style,
            "provider": p.provider,
            "pool": pool_id,
        }
        for p in audience_pool.personas
    ]
