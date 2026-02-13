"""LLM-driven conversational role designer.

Uses Anthropic (Claude Sonnet 4.5) to interview the user about their team role needs,
then generates a complete role configuration including briefing.
"""

import json
import logging
import re

from anthropic import AsyncAnthropic

from app.core.config import settings

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# System prompt for the role designer LLM
# ---------------------------------------------------------------------------

SYSTEM_PROMPT = """You are a Role Designer — an expert at creating team roles for AI agent collaboration systems. Your job is to interview the user about the role they want to create, then produce a complete role configuration.

## How You Work

1. **Interview phase**: Ask the user 3-5 focused questions to understand the role they need. Ask one question at a time. Be conversational but efficient.
2. **Design phase**: When you have enough information, generate the complete role configuration.

## Interview Guidelines

- Start by asking what kind of work they need help with
- Ask about boundaries — what should this role NOT do?
- Ask about team fit — how does this relate to their existing roles?
- Ask about authority — should this role direct others, or be directed?
- Don't ask more than 5 questions total. If you have enough after 3, proceed to design.

## Available Capabilities (tags)

Each tag shapes the agent's behavior, anti-patterns, and peer relationships:
- implementation: Writes and modifies code
- code-review: Reviews code quality and correctness
- testing: Validates implementations, writes tests
- architecture: Designs system structure and patterns
- moderation: Runs structured discussions and debates
- security: Security analysis and auditing
- compliance: Regulatory and policy compliance
- analysis: Research, investigation, and analysis
- coordination: Task management and team coordination
- red-team: Adversarial testing and attack simulation
- documentation: Writes docs, specs, and technical writing
- debugging: Diagnoses and resolves bugs

## Available Permissions

- broadcast: Send messages to all team members simultaneously
- review: Review and approve/reject others' work
- assign_tasks: Assign tasks to other team members
- status: Post status updates about work
- question: Ask questions to other team members
- handoff: Hand off completed work to other roles
- moderation: Moderate structured discussions and debates

## When You're Ready to Generate

When you have enough information, output a friendly summary of the role you'll create, followed by the configuration in this exact format:

|||ROLE_CONFIG|||
{
  "title": "Role Title",
  "slug": "role-slug",
  "description": "One-sentence description of this role",
  "tags": ["tag1", "tag2"],
  "permissions": ["perm1", "perm2"],
  "max_instances": 1,
  "briefing": "Full markdown briefing content..."
}
|||END_CONFIG|||

The briefing should be a complete markdown document with these sections:
1. Identity — who this role is
2. Primary Function — what it does (derived from tags)
3. Anti-patterns — what it should NEVER do
4. Peer Relationships — how it relates to other team roles
5. Action Boundary — what permissions it has
6. Onboarding — first steps when joining

## Important Rules

- The slug must be lowercase alphanumeric with hyphens only
- Always include "status" in permissions unless there's a specific reason not to
- max_instances should be 1 for specialized roles, 2-3 for implementation roles
- The briefing should reference the specific team context the user described
- Be opinionated — recommend what you think is best, don't just ask the user to pick
- If the user's request closely matches an existing role on their team, point that out
"""


# ---------------------------------------------------------------------------
# Anthropic API call
# ---------------------------------------------------------------------------

async def _call_anthropic(messages: list[dict], team_context: str) -> dict:
    """Call Anthropic API (Claude Sonnet 4.5) with the role designer system prompt."""
    if not settings.anthropic_api_key:
        raise RuntimeError("Anthropic API key not configured")

    client = AsyncAnthropic(api_key=settings.anthropic_api_key)
    resp = await client.messages.create(
        model="claude-sonnet-4-5-20250929",
        max_tokens=4096,
        system=SYSTEM_PROMPT + "\n\n" + team_context,
        messages=messages,
    )
    content = resp.content[0].text if resp.content else ""
    return {
        "content": content,
        "model": resp.model,
        "usage": {
            "input_tokens": resp.usage.input_tokens,
            "output_tokens": resp.usage.output_tokens,
            "total_tokens": resp.usage.input_tokens + resp.usage.output_tokens,
        },
    }


# ---------------------------------------------------------------------------
# Config parsing
# ---------------------------------------------------------------------------

def _parse_role_config(text: str) -> dict | None:
    """Extract role config JSON from LLM response delimiters."""
    pattern = r"\|\|\|ROLE_CONFIG\|\|\|\s*(.*?)\s*\|\|\|END_CONFIG\|\|\|"
    match = re.search(pattern, text, re.DOTALL)
    if not match:
        return None

    json_str = match.group(1).strip()
    try:
        config = json.loads(json_str)
    except json.JSONDecodeError:
        logger.warning("Failed to parse role config JSON: %s", json_str[:200])
        return None

    # Validate required fields
    required = {"title", "slug", "description", "tags", "permissions", "max_instances", "briefing"}
    if not required.issubset(config.keys()):
        missing = required - set(config.keys())
        logger.warning("Role config missing fields: %s", missing)
        return None

    # Sanitize slug
    config["slug"] = re.sub(r"[^a-z0-9-]", "", config["slug"].lower().replace(" ", "-"))
    if not config["slug"]:
        config["slug"] = config["title"].lower().replace(" ", "-")
        config["slug"] = re.sub(r"[^a-z0-9-]", "", config["slug"])

    # Ensure types
    if not isinstance(config["tags"], list):
        config["tags"] = []
    if not isinstance(config["permissions"], list):
        config["permissions"] = ["status"]
    if not isinstance(config["max_instances"], int):
        config["max_instances"] = 1

    return config


def _extract_reply(text: str) -> str:
    """Extract the conversational reply, stripping the config block if present."""
    # Remove the config block from the reply text
    pattern = r"\|\|\|ROLE_CONFIG\|\|\|.*?\|\|\|END_CONFIG\|\|\|"
    clean = re.sub(pattern, "", text, flags=re.DOTALL).strip()
    return clean if clean else text


# ---------------------------------------------------------------------------
# Main entry point
# ---------------------------------------------------------------------------

def _build_team_context(project_context: dict) -> str:
    """Build a team context string from project data."""
    lines = ["## Current Team Context"]
    roles = project_context.get("roles", {})
    if roles:
        lines.append("\nExisting roles on this team:")
        for slug, role in roles.items():
            tags = role.get("tags", [])
            perms = role.get("permissions", [])
            max_inst = role.get("max_instances", 1)
            lines.append(
                f"- **{role.get('title', slug)}** ({slug}): "
                f"{role.get('description', 'No description')}. "
                f"Tags: {', '.join(tags) if tags else 'none'}. "
                f"Permissions: {', '.join(perms) if perms else 'none'}. "
                f"Max instances: {max_inst}."
            )
    else:
        lines.append("\nNo roles defined yet — this will be the first role.")

    return "\n".join(lines)


async def design_role(messages: list[dict], project_context: dict) -> dict:
    """Run one turn of the role design conversation.

    Args:
        messages: Conversation history [{role: "user"|"assistant", content: str}, ...]
        project_context: {roles: {slug: {title, description, tags, permissions, max_instances}}}

    Returns:
        {reply: str, role_config: dict|None}
    """
    team_context = _build_team_context(project_context)

    result = await _call_anthropic(messages, team_context)
    content = result["content"]

    role_config = _parse_role_config(content)
    reply = _extract_reply(content)

    logger.info(
        "Role designer turn: %d messages, config=%s, model=%s, tokens=%s",
        len(messages),
        "generated" if role_config else "none",
        result.get("model"),
        result.get("usage", {}).get("total_tokens", "?"),
    )

    return {
        "reply": reply,
        "role_config": role_config,
    }
