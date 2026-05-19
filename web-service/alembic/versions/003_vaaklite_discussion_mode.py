"""Vaaklite v1 — discussion-mode + document drafting schema.

Revision ID: 003_vaaklite
Revises: 002_role_tags
Create Date: 2026-05-19

Adds the schema additions needed for Vaaklite (per architect msg 5738 spec
lock, human msg 5730 directive). Purely additive — existing rows in
`web_projects` get `mode=coding` via server_default and `template=NULL`,
preserving all current coding-collab behavior.

New columns on existing tables:
* `web_projects.mode` enum(`coding`,`discussion`) default `coding`
* `web_projects.template` nullable string(64)

New tables:
* `web_documents` — markdown documents being drafted in discussion mode
* `web_document_sections` — ordered sections per document with phase status
* `web_drafting_turns` — audit log of which role drafted which section when

Down-revision drops the new tables + columns. No data destruction on
existing coding-mode projects since they don't use any of the new state.
"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


revision: str = "003_vaaklite"
down_revision: Union[str, None] = "002_role_tags"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


# Enum value lists kept in lockstep with app.models — keep them static here
# so the migration is self-contained and doesn't import app.* (Alembic
# isolation discipline; same pattern as 001_initial / 002_role_tags).
PROJECT_MODE_VALUES = ("coding", "discussion")
DOCUMENT_PHASE_VALUES = ("drafting", "review", "revision", "final")
DOCUMENT_SECTION_STATUS_VALUES = ("pending", "drafting", "review_pending", "accepted")


def upgrade() -> None:
    # web_projects: mode + template
    project_mode = sa.Enum(*PROJECT_MODE_VALUES, name="projectmode")
    project_mode.create(op.get_bind(), checkfirst=True)
    op.add_column(
        "web_projects",
        sa.Column(
            "mode",
            project_mode,
            nullable=False,
            server_default="coding",
        ),
    )
    op.add_column(
        "web_projects",
        sa.Column("template", sa.String(length=64), nullable=True),
    )

    # web_documents
    op.create_table(
        "web_documents",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column(
            "project_id",
            sa.Integer(),
            sa.ForeignKey("web_projects.id"),
            nullable=False,
            index=True,
        ),
        sa.Column("title", sa.String(length=200), nullable=False),
        sa.Column("topic", sa.Text(), nullable=False, server_default=""),
        sa.Column(
            "phase",
            sa.Enum(*DOCUMENT_PHASE_VALUES, name="documentphase"),
            nullable=False,
            server_default="drafting",
        ),
        sa.Column("current_section_idx", sa.Integer(), nullable=True),
        sa.Column("current_role", sa.String(length=100), nullable=True),
        sa.Column("rendered_markdown", sa.Text(), nullable=False, server_default=""),
        sa.Column("final_markdown", sa.Text(), nullable=True),
        sa.Column("finalized_at", sa.DateTime(timezone=True), nullable=True),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.func.now(),
        ),
        sa.Column(
            "updated_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.func.now(),
        ),
    )

    # web_document_sections
    op.create_table(
        "web_document_sections",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column(
            "document_id",
            sa.Integer(),
            sa.ForeignKey("web_documents.id"),
            nullable=False,
            index=True,
        ),
        sa.Column("idx", sa.Integer(), nullable=False),
        sa.Column("title", sa.String(length=200), nullable=False),
        sa.Column("assigned_role", sa.String(length=100), nullable=True),
        sa.Column("body", sa.Text(), nullable=False, server_default=""),
        sa.Column(
            "status",
            sa.Enum(*DOCUMENT_SECTION_STATUS_VALUES, name="documentsectionstatus"),
            nullable=False,
            server_default="pending",
        ),
        sa.Column("review_notes", sa.Text(), nullable=True),
        sa.Column(
            "updated_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.func.now(),
        ),
        sa.UniqueConstraint("document_id", "idx", name="uq_document_section_idx"),
    )

    # web_drafting_turns
    op.create_table(
        "web_drafting_turns",
        sa.Column("id", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column(
            "document_id",
            sa.Integer(),
            sa.ForeignKey("web_documents.id"),
            nullable=False,
            index=True,
        ),
        sa.Column("section_idx", sa.Integer(), nullable=False),
        sa.Column("role_seat", sa.String(length=100), nullable=False),
        sa.Column("output_body", sa.Text(), nullable=False, server_default=""),
        sa.Column(
            "started_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.func.now(),
        ),
        sa.Column("completed_at", sa.DateTime(timezone=True), nullable=True),
    )


def downgrade() -> None:
    op.drop_table("web_drafting_turns")
    op.drop_table("web_document_sections")
    op.drop_table("web_documents")
    op.drop_column("web_projects", "template")
    op.drop_column("web_projects", "mode")
    # Drop enum types
    sa.Enum(name="documentsectionstatus").drop(op.get_bind(), checkfirst=True)
    sa.Enum(name="documentphase").drop(op.get_bind(), checkfirst=True)
    sa.Enum(name="projectmode").drop(op.get_bind(), checkfirst=True)
