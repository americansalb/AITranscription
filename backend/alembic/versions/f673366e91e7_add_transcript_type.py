"""add_transcript_type

Revision ID: f673366e91e7
Revises: 0b161e9c498a
Create Date: 2026-01-21 19:28:40.902630

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = 'f673366e91e7'
down_revision: Union[str, None] = '0b161e9c498a'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # Add transcript_type column to transcripts table
    op.add_column('transcripts', sa.Column('transcript_type', sa.String(length=20), nullable=True))

    # Set existing transcripts to 'input' type
    op.execute("UPDATE transcripts SET transcript_type = 'input' WHERE transcript_type IS NULL")

    # Make the column non-nullable after setting defaults
    op.alter_column('transcripts', 'transcript_type', nullable=False)


def downgrade() -> None:
    op.drop_column('transcripts', 'transcript_type')
