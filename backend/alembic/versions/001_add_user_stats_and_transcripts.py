"""Add user stats columns and transcripts table

Revision ID: 001
Revises:
Create Date: 2024-12-31

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = "001"
down_revision: Union[str, None] = None
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # Add new columns to users table if they don't exist
    # Using raw SQL to check if columns exist first
    conn = op.get_bind()

    # Check if users table exists
    result = conn.execute(sa.text(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'users')"
    ))
    users_exists = result.scalar()

    if users_exists:
        # Add is_admin column if missing
        result = conn.execute(sa.text(
            "SELECT EXISTS (SELECT FROM information_schema.columns "
            "WHERE table_name = 'users' AND column_name = 'is_admin')"
        ))
        if not result.scalar():
            op.add_column('users', sa.Column('is_admin', sa.Boolean(), nullable=False, server_default='false'))

        # Add total_audio_seconds column if missing
        result = conn.execute(sa.text(
            "SELECT EXISTS (SELECT FROM information_schema.columns "
            "WHERE table_name = 'users' AND column_name = 'total_audio_seconds')"
        ))
        if not result.scalar():
            op.add_column('users', sa.Column('total_audio_seconds', sa.Integer(), nullable=False, server_default='0'))

        # Add total_polish_tokens column if missing
        result = conn.execute(sa.text(
            "SELECT EXISTS (SELECT FROM information_schema.columns "
            "WHERE table_name = 'users' AND column_name = 'total_polish_tokens')"
        ))
        if not result.scalar():
            op.add_column('users', sa.Column('total_polish_tokens', sa.Integer(), nullable=False, server_default='0'))

        # Add total_transcriptions column if missing
        result = conn.execute(sa.text(
            "SELECT EXISTS (SELECT FROM information_schema.columns "
            "WHERE table_name = 'users' AND column_name = 'total_transcriptions')"
        ))
        if not result.scalar():
            op.add_column('users', sa.Column('total_transcriptions', sa.Integer(), nullable=False, server_default='0'))

        # Add total_words column if missing
        result = conn.execute(sa.text(
            "SELECT EXISTS (SELECT FROM information_schema.columns "
            "WHERE table_name = 'users' AND column_name = 'total_words')"
        ))
        if not result.scalar():
            op.add_column('users', sa.Column('total_words', sa.Integer(), nullable=False, server_default='0'))

    # Create transcripts table if it doesn't exist
    result = conn.execute(sa.text(
        "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'transcripts')"
    ))
    if not result.scalar():
        op.create_table(
            'transcripts',
            sa.Column('id', sa.Integer(), nullable=False),
            sa.Column('user_id', sa.Integer(), nullable=True),
            sa.Column('raw_text', sa.Text(), nullable=False),
            sa.Column('polished_text', sa.Text(), nullable=False),
            sa.Column('audio_duration_seconds', sa.Float(), nullable=False, server_default='0.0'),
            sa.Column('language', sa.String(10), nullable=True),
            sa.Column('word_count', sa.Integer(), nullable=False, server_default='0'),
            sa.Column('character_count', sa.Integer(), nullable=False, server_default='0'),
            sa.Column('words_per_minute', sa.Float(), nullable=False, server_default='0.0'),
            sa.Column('context', sa.String(50), nullable=True),
            sa.Column('formality', sa.String(20), nullable=True),
            sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False),
            sa.PrimaryKeyConstraint('id'),
            sa.ForeignKeyConstraint(['user_id'], ['users.id'], ondelete='CASCADE'),
        )
        op.create_index('ix_transcripts_user_id', 'transcripts', ['user_id'])
        op.create_index('ix_transcripts_created_at', 'transcripts', ['created_at'])


def downgrade() -> None:
    # Drop transcripts table
    op.drop_index('ix_transcripts_created_at', table_name='transcripts')
    op.drop_index('ix_transcripts_user_id', table_name='transcripts')
    op.drop_table('transcripts')

    # Remove new columns from users
    op.drop_column('users', 'total_words')
    op.drop_column('users', 'total_transcriptions')
    op.drop_column('users', 'total_polish_tokens')
    op.drop_column('users', 'total_audio_seconds')
    op.drop_column('users', 'is_admin')
