"""Add missing user columns for stats and settings

Revision ID: 003
Revises: 002
Create Date: 2025-01-08

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = "003"
down_revision: Union[str, None] = "002"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # Add missing columns if they don't exist
    conn = op.get_bind()

    # Check and add is_admin
    result = conn.execute(sa.text(
        "SELECT EXISTS (SELECT FROM information_schema.columns "
        "WHERE table_name = 'users' AND column_name = 'is_admin')"
    ))
    if not result.scalar():
        op.add_column('users', sa.Column('is_admin', sa.Boolean(), nullable=False, server_default='false'))

    # Check and add total_transcriptions
    result = conn.execute(sa.text(
        "SELECT EXISTS (SELECT FROM information_schema.columns "
        "WHERE table_name = 'users' AND column_name = 'total_transcriptions')"
    ))
    if not result.scalar():
        op.add_column('users', sa.Column('total_transcriptions', sa.Integer(), nullable=False, server_default='0'))

    # Check and add total_words
    result = conn.execute(sa.text(
        "SELECT EXISTS (SELECT FROM information_schema.columns "
        "WHERE table_name = 'users' AND column_name = 'total_words')"
    ))
    if not result.scalar():
        op.add_column('users', sa.Column('total_words', sa.Integer(), nullable=False, server_default='0'))

    # Check and add daily_transcription_limit
    result = conn.execute(sa.text(
        "SELECT EXISTS (SELECT FROM information_schema.columns "
        "WHERE table_name = 'users' AND column_name = 'daily_transcription_limit')"
    ))
    if not result.scalar():
        op.add_column('users', sa.Column('daily_transcription_limit', sa.Integer(), nullable=False, server_default='100'))


def downgrade() -> None:
    op.drop_column('users', 'daily_transcription_limit')
    op.drop_column('users', 'total_words')
    op.drop_column('users', 'total_transcriptions')
    op.drop_column('users', 'is_admin')
