"""Initial schema — 8 tables for Vaak Web Service.

Revision ID: 001_initial
Revises:
Create Date: 2026-02-24

Tables: web_users, web_projects, web_project_roles, web_messages,
        web_usage_records, web_discussions, web_discussion_rounds,
        web_discussion_submissions
"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa

revision: str = "001_initial"
down_revision: Union[str, None] = None
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # --- web_users ---
    op.create_table(
        "web_users",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column("email", sa.String(255), unique=True, nullable=False, index=True),
        sa.Column("hashed_password", sa.String(255), nullable=False),
        sa.Column("full_name", sa.String(255), nullable=True),
        sa.Column(
            "tier",
            sa.Enum("free", "pro", "byok", name="subscriptiontier"),
            nullable=False,
            server_default="free",
        ),
        sa.Column("is_active", sa.Boolean(), nullable=False, server_default="true"),
        sa.Column("stripe_customer_id", sa.String(255), nullable=True),
        sa.Column("stripe_subscription_id", sa.String(255), nullable=True),
        sa.Column("byok_anthropic_key", sa.String(512), nullable=True),
        sa.Column("byok_openai_key", sa.String(512), nullable=True),
        sa.Column("byok_google_key", sa.String(512), nullable=True),
        sa.Column("monthly_tokens_used", sa.Integer(), nullable=False, server_default="0"),
        sa.Column("monthly_cost_usd", sa.Float(), nullable=False, server_default="0"),
        sa.Column("usage_reset_at", sa.DateTime(timezone=True), nullable=True),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.text("now()"),
        ),
    )

    # --- web_projects ---
    op.create_table(
        "web_projects",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column("name", sa.String(100), nullable=False),
        sa.Column("owner_id", sa.Integer(), sa.ForeignKey("web_users.id"), nullable=False),
        sa.Column("is_active", sa.Boolean(), nullable=False, server_default="true"),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.text("now()"),
        ),
    )

    # --- web_project_roles ---
    op.create_table(
        "web_project_roles",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column(
            "project_id", sa.Integer(), sa.ForeignKey("web_projects.id"), nullable=False
        ),
        sa.Column("slug", sa.String(50), nullable=False),
        sa.Column("title", sa.String(100), nullable=False),
        sa.Column("briefing", sa.Text(), nullable=False, server_default=""),
        sa.Column("provider", sa.String(50), nullable=False, server_default="anthropic"),
        sa.Column(
            "model", sa.String(100), nullable=False, server_default="claude-sonnet-4-6"
        ),
        sa.Column("max_instances", sa.Integer(), nullable=False, server_default="1"),
        sa.Column("is_agent_running", sa.Boolean(), nullable=False, server_default="false"),
        sa.UniqueConstraint("project_id", "slug", name="uq_project_role_slug"),
    )

    # --- web_messages ---
    op.create_table(
        "web_messages",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column(
            "project_id",
            sa.Integer(),
            sa.ForeignKey("web_projects.id"),
            nullable=False,
            index=True,
        ),
        sa.Column("from_role", sa.String(100), nullable=False),
        sa.Column("to_role", sa.String(100), nullable=False),
        sa.Column("msg_type", sa.String(50), nullable=False),
        sa.Column("subject", sa.String(500), nullable=False, server_default=""),
        sa.Column("body", sa.Text(), nullable=False),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.text("now()"),
            index=True,
        ),
    )

    # --- web_usage_records ---
    op.create_table(
        "web_usage_records",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column(
            "user_id",
            sa.Integer(),
            sa.ForeignKey("web_users.id"),
            nullable=False,
            index=True,
        ),
        sa.Column(
            "project_id", sa.Integer(), sa.ForeignKey("web_projects.id"), nullable=False
        ),
        sa.Column("model", sa.String(100), nullable=False),
        sa.Column("provider", sa.String(50), nullable=False),
        sa.Column("input_tokens", sa.Integer(), nullable=False),
        sa.Column("output_tokens", sa.Integer(), nullable=False),
        sa.Column("raw_cost_usd", sa.Float(), nullable=False),
        sa.Column("marked_up_cost_usd", sa.Float(), nullable=False),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.text("now()"),
        ),
    )

    # --- web_discussions ---
    op.create_table(
        "web_discussions",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column(
            "project_id",
            sa.Integer(),
            sa.ForeignKey("web_projects.id"),
            nullable=False,
            index=True,
        ),
        sa.Column(
            "mode",
            sa.Enum("delphi", "oxford", "red_team", "continuous", name="discussionmode"),
            nullable=False,
        ),
        sa.Column("topic", sa.Text(), nullable=False),
        sa.Column("is_active", sa.Boolean(), nullable=False, server_default="true"),
        sa.Column(
            "phase",
            sa.Enum(
                "preparing",
                "submitting",
                "aggregating",
                "reviewing",
                "paused",
                "complete",
                name="discussionphase",
            ),
            nullable=False,
            server_default="preparing",
        ),
        sa.Column("moderator", sa.String(100), nullable=True),
        sa.Column("participants", sa.JSON(), nullable=False, server_default="[]"),
        sa.Column("current_round", sa.Integer(), nullable=False, server_default="0"),
        sa.Column("max_rounds", sa.Integer(), nullable=False, server_default="10"),
        sa.Column("timeout_minutes", sa.Integer(), nullable=False, server_default="15"),
        sa.Column(
            "auto_close_timeout_seconds",
            sa.Integer(),
            nullable=False,
            server_default="60",
        ),
        sa.Column("teams", sa.JSON(), nullable=True),
        sa.Column(
            "started_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.text("now()"),
        ),
        sa.Column("ended_at", sa.DateTime(timezone=True), nullable=True),
    )

    # --- web_discussion_rounds ---
    op.create_table(
        "web_discussion_rounds",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column(
            "discussion_id",
            sa.Integer(),
            sa.ForeignKey("web_discussions.id"),
            nullable=False,
        ),
        sa.Column("number", sa.Integer(), nullable=False),
        sa.Column("topic", sa.Text(), nullable=True),
        sa.Column("auto_triggered", sa.Boolean(), nullable=False, server_default="false"),
        sa.Column("trigger_from", sa.String(100), nullable=True),
        sa.Column("trigger_message_id", sa.Integer(), nullable=True),
        sa.Column(
            "opened_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.text("now()"),
        ),
        sa.Column("closed_at", sa.DateTime(timezone=True), nullable=True),
        sa.Column("aggregate", sa.JSON(), nullable=True),
        sa.Column("aggregate_message_id", sa.Integer(), nullable=True),
        sa.UniqueConstraint(
            "discussion_id", "number", name="uq_discussion_round_number"
        ),
    )

    # --- web_discussion_submissions ---
    op.create_table(
        "web_discussion_submissions",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column(
            "round_id",
            sa.Integer(),
            sa.ForeignKey("web_discussion_rounds.id"),
            nullable=False,
        ),
        sa.Column("from_role", sa.String(100), nullable=False),
        sa.Column(
            "message_id",
            sa.Integer(),
            sa.ForeignKey("web_messages.id"),
            nullable=False,
        ),
        sa.Column(
            "submitted_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.text("now()"),
        ),
    )


def downgrade() -> None:
    op.drop_table("web_discussion_submissions")
    op.drop_table("web_discussion_rounds")
    op.drop_table("web_discussions")
    op.drop_table("web_usage_records")
    op.drop_table("web_messages")
    op.drop_table("web_project_roles")
    op.drop_table("web_projects")
    op.drop_table("web_users")

    # Drop enums (PostgreSQL-specific)
    op.execute("DROP TYPE IF EXISTS subscriptiontier")
    op.execute("DROP TYPE IF EXISTS discussionmode")
    op.execute("DROP TYPE IF EXISTS discussionphase")
