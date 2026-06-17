"""Add Transcription Studio tables (bulk media items + transcript segments).

Revision ID: 005_media_library
Revises: 004_fix_enums
Create Date: 2026-06-17
"""
from alembic import op
import sqlalchemy as sa

# revision identifiers
revision = "005_media_library"
down_revision = "004_fix_enums"
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.create_table(
        "studio_media_items",
        sa.Column("id", sa.Integer(), nullable=False),
        sa.Column("filename", sa.String(512), nullable=False),
        sa.Column("content_type", sa.String(128), nullable=True),
        sa.Column("size_bytes", sa.Integer(), nullable=True),
        sa.Column("status", sa.String(20), nullable=False, server_default="queued"),
        sa.Column("error", sa.Text(), nullable=True),
        sa.Column("language", sa.String(32), nullable=True),
        sa.Column("duration_seconds", sa.Float(), nullable=True),
        sa.Column("transcript", sa.Text(), nullable=True),
        sa.Column("word_count", sa.Integer(), nullable=False, server_default="0"),
        sa.Column("source_path", sa.String(1024), nullable=True),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
            nullable=False,
        ),
        sa.Column(
            "updated_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
            nullable=False,
        ),
        sa.Column("completed_at", sa.DateTime(timezone=True), nullable=True),
        sa.PrimaryKeyConstraint("id"),
    )
    op.create_index(
        "idx_studio_media_status", "studio_media_items", ["status"]
    )

    op.create_table(
        "studio_transcript_segments",
        sa.Column("id", sa.Integer(), nullable=False),
        sa.Column("media_id", sa.Integer(), nullable=False),
        sa.Column("idx", sa.Integer(), nullable=False),
        sa.Column("text", sa.Text(), nullable=False),
        sa.Column("start_seconds", sa.Float(), nullable=True),
        sa.Column("end_seconds", sa.Float(), nullable=True),
        sa.PrimaryKeyConstraint("id"),
        sa.ForeignKeyConstraint(
            ["media_id"], ["studio_media_items.id"], ondelete="CASCADE"
        ),
    )
    op.create_index(
        "idx_studio_segments_media",
        "studio_transcript_segments",
        ["media_id"],
    )


def downgrade() -> None:
    op.drop_index("idx_studio_segments_media", table_name="studio_transcript_segments")
    op.drop_table("studio_transcript_segments")
    op.drop_index("idx_studio_media_status", table_name="studio_media_items")
    op.drop_table("studio_media_items")
