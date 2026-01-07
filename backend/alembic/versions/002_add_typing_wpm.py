"""Add typing_wpm column for time saved calculation

Revision ID: 002
Revises: 001
Create Date: 2025-01-07

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = "002"
down_revision: Union[str, None] = "001"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # Add typing_wpm column if it doesn't exist
    conn = op.get_bind()

    result = conn.execute(sa.text(
        "SELECT EXISTS (SELECT FROM information_schema.columns "
        "WHERE table_name = 'users' AND column_name = 'typing_wpm')"
    ))
    if not result.scalar():
        op.add_column('users', sa.Column('typing_wpm', sa.Integer(), nullable=False, server_default='40'))


def downgrade() -> None:
    op.drop_column('users', 'typing_wpm')
