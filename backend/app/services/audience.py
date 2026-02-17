"""Audience voting service for Oxford-style debates.

Manages multiple audience pools stored as JSON files in ~/.vaak/audiences/.
Each pool contains 27 personas across 3 LLM providers (9 each):
- Groq (Llama 4 Scout)
- OpenAI (GPT-5 mini)
- Anthropic (Claude Haiku 4.5)

Pools:
- "general" — 27 diverse personas. Broad societal perspective.
- "software-dev" — 27 software engineering experts. Technical depth.
- "ai-ml" — 27 AI/ML experts. Research and industry perspectives.
- "law" — 27 legal experts across specializations.
- Custom pools created by users.
"""

import asyncio
import json
import logging
import os
import re
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

import httpx
from anthropic import AsyncAnthropic

from app.core.config import settings

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Audiences directory — cross-project, in user home
# ---------------------------------------------------------------------------

def _audiences_dir() -> Path:
    """Get the audiences directory path (~/.vaak/audiences/)."""
    return Path.home() / ".vaak" / "audiences"


def _ensure_audiences_dir() -> Path:
    """Ensure the audiences directory exists and return its path."""
    d = _audiences_dir()
    d.mkdir(parents=True, exist_ok=True)
    return d


# ---------------------------------------------------------------------------
# Pool data model
# ---------------------------------------------------------------------------

@dataclass
class Persona:
    name: str
    background: str
    values: str
    style: str
    provider: str  # "groq" | "openai" | "anthropic"


@dataclass
class AudiencePool:
    id: str
    name: str
    description: str
    builtin: bool
    personas: list[Persona]

    @property
    def member_count(self) -> int:
        return len(self.personas)

    @property
    def providers(self) -> list[str]:
        return list({p.provider for p in self.personas})


# ---------------------------------------------------------------------------
# Pool ID validation — prevent path traversal
# ---------------------------------------------------------------------------

_POOL_ID_RE = re.compile(r'^[a-z0-9][a-z0-9-]*$')


def _validate_pool_id(pool_id: str) -> None:
    """Validate pool_id is a safe slug. Raises ValueError on bad input."""
    if not pool_id or not _POOL_ID_RE.match(pool_id):
        raise ValueError(
            f"Invalid pool ID '{pool_id}': must be lowercase alphanumeric with hyphens"
        )


# ---------------------------------------------------------------------------
# Pool I/O — load, save, list, delete
# ---------------------------------------------------------------------------

def load_pool(pool_id: str) -> Optional[AudiencePool]:
    """Load a pool from its JSON file. Returns None if not found."""
    _validate_pool_id(pool_id)
    path = _audiences_dir() / f"{pool_id}.json"
    if not path.exists():
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        personas = [
            Persona(
                name=p["name"],
                background=p["background"],
                values=p["values"],
                style=p["style"],
                provider=p["provider"],
            )
            for p in data.get("personas", [])
        ]
        return AudiencePool(
            id=data["id"],
            name=data["name"],
            description=data.get("description", ""),
            builtin=data.get("builtin", False),
            personas=personas,
        )
    except Exception as e:
        logger.error(f"Failed to load pool '{pool_id}': {e}")
        return None


def list_pools() -> list[dict]:
    """List all available pools with metadata (without loading full persona lists)."""
    d = _audiences_dir()
    if not d.exists():
        return []
    pools = []
    for f in sorted(d.glob("*.json")):
        try:
            data = json.loads(f.read_text(encoding="utf-8"))
            personas = data.get("personas", [])
            pools.append({
                "id": data["id"],
                "name": data["name"],
                "description": data.get("description", ""),
                "builtin": data.get("builtin", False),
                "member_count": len(personas),
                "providers": list({p["provider"] for p in personas}),
            })
        except Exception as e:
            logger.warning(f"Skipping malformed pool file {f.name}: {e}")
    return pools


def save_pool(pool_data: dict) -> str:
    """Save a pool to disk. Returns the pool ID."""
    d = _ensure_audiences_dir()
    pool_id = pool_data["id"]
    _validate_pool_id(pool_id)
    path = d / f"{pool_id}.json"
    path.write_text(json.dumps(pool_data, indent=2, ensure_ascii=False), encoding="utf-8")
    return pool_id


def delete_pool(pool_id: str) -> bool:
    """Delete a pool file. Returns True if deleted, False if not found or builtin."""
    _validate_pool_id(pool_id)
    path = _audiences_dir() / f"{pool_id}.json"
    if not path.exists():
        return False
    # Check if builtin — refuse to delete predefined pools
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        if data.get("builtin", False):
            return False
    except Exception:
        pass
    path.unlink()
    return True


# ---------------------------------------------------------------------------
# Provider-specific API callers
# ---------------------------------------------------------------------------

async def _call_groq(persona: Persona, system_prompt: str, user_prompt: str) -> dict:
    """Call Groq API (Llama 4 Scout)."""
    async with httpx.AsyncClient(timeout=30.0) as client:
        resp = await client.post(
            "https://api.groq.com/openai/v1/chat/completions",
            headers={
                "Authorization": f"Bearer {settings.groq_api_key}",
                "Content-Type": "application/json",
            },
            json={
                "model": "meta-llama/llama-4-scout-17b-16e-instruct",
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_prompt},
                ],
                "max_tokens": 512,
                "temperature": 0.7,
            },
        )
        if resp.status_code != 200:
            error_body = resp.text
            raise RuntimeError(f"Groq {resp.status_code}: {error_body[:300]}")
        data = resp.json()
        return {
            "content": data["choices"][0]["message"]["content"],
            "model": "llama-4-scout",
            "provider": "groq",
            "usage": data.get("usage", {}),
        }


async def _call_openai(persona: Persona, system_prompt: str, user_prompt: str) -> dict:
    """Call OpenAI API (GPT-5 mini)."""
    async with httpx.AsyncClient(timeout=30.0) as client:
        resp = await client.post(
            "https://api.openai.com/v1/chat/completions",
            headers={
                "Authorization": f"Bearer {settings.openai_api_key}",
                "Content-Type": "application/json",
            },
            json={
                "model": "gpt-5-mini",
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_prompt},
                ],
                "max_completion_tokens": 512,
                "temperature": 0.7,
            },
        )
        if resp.status_code != 200:
            error_body = resp.text
            raise RuntimeError(f"OpenAI {resp.status_code}: {error_body[:300]}")
        data = resp.json()
        return {
            "content": data["choices"][0]["message"]["content"],
            "model": "gpt-5-mini",
            "provider": "openai",
            "usage": data.get("usage", {}),
        }


async def _call_anthropic(persona: Persona, system_prompt: str, user_prompt: str) -> dict:
    """Call Anthropic API (Claude Haiku 4.5)."""
    client = AsyncAnthropic(api_key=settings.anthropic_api_key)
    resp = await client.messages.create(
        model="claude-haiku-4-5-20251001",
        max_tokens=512,
        temperature=0.7,
        system=system_prompt,
        messages=[{"role": "user", "content": user_prompt}],
    )
    return {
        "content": resp.content[0].text if resp.content else "",
        "model": "claude-haiku-4.5",
        "provider": "anthropic",
        "usage": {
            "input_tokens": resp.usage.input_tokens,
            "output_tokens": resp.usage.output_tokens,
        },
    }


PROVIDER_CALLERS = {
    "groq": _call_groq,
    "openai": _call_openai,
    "anthropic": _call_anthropic,
}


# ---------------------------------------------------------------------------
# Core voting logic
# ---------------------------------------------------------------------------

def _build_persona_system_prompt(persona: Persona) -> str:
    return (
        f"You are {persona.name}.\n"
        f"Background: {persona.background}\n"
        f"What you value when evaluating ideas: {persona.values}\n"
        f"Your reasoning style: {persona.style}\n\n"
        "You are an audience member in a live debate. You are a real person with real opinions — "
        "not a cautious analyst. When you hear a proposition, you react based on your years of "
        "experience and your gut instincts, informed by your expertise.\n\n"
        "RULES:\n"
        "1. Vote FOR, AGAINST, or ABSTAIN on the proposition.\n"
        "2. HAVE AN OPINION. You are an expert with a unique perspective — use it. "
        "Real audience members lean one way or another. ABSTAIN is reserved for topics "
        "genuinely outside your expertise, not for uncertainty or wanting more data. "
        "If you can see ANY angle from your background, pick a side and argue it.\n"
        "3. Your vote should reflect YOUR specific expertise and values — not generic caution. "
        "A security architect sees different risks than a game engine programmer. "
        "A mobile developer cares about different things than a compiler engineer. "
        "Let your unique lens drive your vote.\n"
        "4. Give a brief rationale (2-4 sentences) explaining your vote from YOUR unique perspective. "
        "Reference your specific domain experience.\n"
        "5. Format your response EXACTLY as:\n"
        "VOTE: FOR  (or VOTE: AGAINST  or VOTE: ABSTAIN)\n"
        "RATIONALE: [your reasoning]\n"
        "6. Nothing else — no preamble, no hedging, no 'as an AI'."
    )


def _build_vote_prompt(topic: str, arguments: str, phase: str = "post",
                       persona: Optional[Persona] = None) -> str:
    persona_reminder = ""
    if persona:
        persona_reminder = (
            f"\nRemember: You are {persona.name}, {persona.background}.\n"
            f"Apply YOUR unique lens: {persona.values}\n"
            "Different experts weigh evidence differently based on their experience. "
            "What stands out to YOU specifically? What do others likely miss?\n\n"
        )

    if phase == "pre":
        return (
            f"DEBATE TOPIC: {topic}\n\n"
            "The debate has not yet begun. No arguments have been presented yet.\n\n"
            f"{persona_reminder}"
            "Based on YOUR professional experience and instincts, what is your initial "
            "reaction to this proposition? Everyone has a first impression — what's yours?\n\n"
            "You will get to vote again after hearing the full arguments, so this is your "
            "gut reaction. Don't overthink it — lean into your expertise and pick a side. "
            "Most real audience members walk in with a leaning. What's yours?"
        )
    return (
        f"DEBATE TOPIC: {topic}\n\n"
        f"ARGUMENTS PRESENTED:\n{arguments}\n\n"
        f"{persona_reminder}"
        "You've heard the arguments. Based on everything presented and your own expertise, "
        "cast your vote. Did the arguments change your mind, reinforce your position, or "
        "leave you unconvinced? Take a clear stand."
    )


async def _get_single_vote(
    persona: Persona, topic: str, arguments: str, phase: str
) -> dict:
    """Get a single audience member's vote."""
    system_prompt = _build_persona_system_prompt(persona)
    user_prompt = _build_vote_prompt(topic, arguments, phase, persona=persona)
    caller = PROVIDER_CALLERS[persona.provider]

    start = time.monotonic()
    try:
        result = await caller(persona, system_prompt, user_prompt)
        elapsed = time.monotonic() - start

        # Parse vote from response
        content = result["content"].strip()
        vote = "ABSTAIN"
        rationale = content

        for line in content.split("\n"):
            line_upper = line.strip().upper()
            if line_upper.startswith("VOTE:"):
                vote_text = line_upper.replace("VOTE:", "").strip()
                if re.search(r'\bFOR\b', vote_text):
                    vote = "FOR"
                elif re.search(r'\bAGAINST\b', vote_text):
                    vote = "AGAINST"
            if line.strip().upper().startswith("RATIONALE:"):
                rationale = line.strip()[len("RATIONALE:"):].strip()

        return {
            "persona": persona.name,
            "background": persona.background,
            "provider": persona.provider,
            "model": result["model"],
            "vote": vote,
            "rationale": rationale,
            "latency_ms": int(elapsed * 1000),
            "usage": result.get("usage", {}),
            "error": None,
        }
    except Exception as e:
        elapsed = time.monotonic() - start
        logger.error(f"Vote failed for {persona.name} ({persona.provider}): {e}")
        return {
            "persona": persona.name,
            "background": persona.background,
            "provider": persona.provider,
            "model": "error",
            "vote": "ERROR",
            "rationale": "Vote collection failed for this persona",
            "latency_ms": int(elapsed * 1000),
            "usage": {},
            "error": "Vote failed",
        }


async def collect_audience_votes(
    topic: str,
    arguments: str = "",
    phase: str = "post",
    providers: Optional[list[str]] = None,
    pool: Optional[str] = None,
) -> dict:
    """
    Collect votes from audience members in parallel.

    Args:
        topic: The debate proposition
        arguments: Concatenated debate arguments (empty for pre-vote)
        phase: "pre" or "post"
        providers: Optional filter — e.g. ["groq", "openai"] to exclude some
        pool: Pool ID to use — "general", "software-dev", etc. Defaults to "general".

    Returns:
        dict with votes, tallies, pool info, and metadata
    """
    pool_id = pool or "general"
    audience_pool = load_pool(pool_id)
    if not audience_pool:
        return {
            "error": f"Pool '{pool_id}' not found",
            "votes": [],
            "tally": {"FOR": 0, "AGAINST": 0, "ABSTAIN": 0, "ERROR": 0},
        }

    allowed = set(providers) if providers else {"groq", "openai", "anthropic"}
    active_personas = [p for p in audience_pool.personas if p.provider in allowed]

    # Validate API keys
    if "groq" in allowed and not settings.groq_api_key:
        logger.warning("Groq API key not configured — skipping Llama votes")
        active_personas = [p for p in active_personas if p.provider != "groq"]
    if "openai" in allowed and not settings.openai_api_key:
        logger.warning("OpenAI API key not configured — skipping GPT votes")
        active_personas = [p for p in active_personas if p.provider != "openai"]
    if "anthropic" in allowed and not settings.anthropic_api_key:
        logger.warning("Anthropic API key not configured — skipping Haiku votes")
        active_personas = [p for p in active_personas if p.provider != "anthropic"]

    if not active_personas:
        return {
            "error": "No API keys configured for any provider",
            "votes": [],
            "tally": {"FOR": 0, "AGAINST": 0, "ABSTAIN": 0, "ERROR": 0},
        }

    start = time.monotonic()

    # Fire all votes in parallel
    tasks = [
        _get_single_vote(persona, topic, arguments, phase)
        for persona in active_personas
    ]
    votes = await asyncio.gather(*tasks)

    elapsed = time.monotonic() - start

    # Tag each vote with its pool
    for v in votes:
        v["pool"] = pool_id

    # Tally
    tally = {"FOR": 0, "AGAINST": 0, "ABSTAIN": 0, "ERROR": 0}
    by_provider = {}
    for v in votes:
        tally[v["vote"]] = tally.get(v["vote"], 0) + 1
        prov = v["provider"]
        if prov not in by_provider:
            by_provider[prov] = {"FOR": 0, "AGAINST": 0, "ABSTAIN": 0, "ERROR": 0}
        by_provider[prov][v["vote"]] = by_provider[prov].get(v["vote"], 0) + 1

    # Per-pool tally (single pool for now, supports multi-pool aggregation)
    tally_by_pool = {pool_id: dict(tally)}

    return {
        "topic": topic,
        "phase": phase,
        "pool": pool_id,
        "pool_name": audience_pool.name,
        "total_voters": len(votes),
        "tally": tally,
        "tally_by_provider": by_provider,
        "tally_by_pool": tally_by_pool,
        "votes": votes,
        "total_latency_ms": int(elapsed * 1000),
    }
