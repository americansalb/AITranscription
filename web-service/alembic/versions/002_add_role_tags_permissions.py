"""Add tags and permissions columns to web_project_roles.

Revision ID: 002_role_tags
Revises: 001_initial
Create Date: 2026-02-24

Fixes silent data loss: CreateRoleRequest accepted tags/permissions but
ProjectRole had no columns — data was silently dropped on every role creation.
"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa

revision: str = "002_role_tags"
down_revision: Union[str, None] = "001_initial"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    op.add_column(
        "web_project_roles",
        sa.Column("tags", sa.JSON(), nullable=False, server_default="[]"),
    )
    op.add_column(
        "web_project_roles",
        sa.Column("permissions", sa.JSON(), nullable=False, server_default="[]"),
    )


def downgrade() -> None:
    op.drop_column("web_project_roles", "permissions")
    op.drop_column("web_project_roles", "tags")
