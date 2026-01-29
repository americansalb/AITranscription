"""add_session_id_to_transcripts

Revision ID: bd3eeb8595a4
Revises: f673366e91e7
Create Date: 2026-01-22 10:44:56.652541

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'bd3eeb8595a4'
down_revision: Union[str, None] = 'f673366e91e7'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # Add session_id column to transcripts table (nullable, for Claude Code session tracking)
    op.add_column('transcripts', sa.Column('session_id', sa.String(length=255), nullable=True))

    # Add index for faster querying by session_id
    op.create_index('ix_transcripts_session_id', 'transcripts', ['session_id'])


def downgrade() -> None:
    op.drop_index('ix_transcripts_session_id', 'transcripts')
    op.drop_column('transcripts', 'session_id')
