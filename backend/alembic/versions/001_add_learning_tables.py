"""Add learning system tables with pgvector support.

Revision ID: 001_learning
Revises:
Create Date: 2024-01-15
"""
from alembic import op
import sqlalchemy as sa
from pgvector.sqlalchemy import Vector

# revision identifiers
revision = "001_learning"
down_revision = None
branch_labels = None
depends_on = None


def upgrade() -> None:
    # Enable pgvector extension
    op.execute("CREATE EXTENSION IF NOT EXISTS vector")

    # Create audio_samples table first (referenced by correction_embeddings)
    op.create_table(
        "audio_samples",
        sa.Column("id", sa.Integer(), nullable=False),
        sa.Column("user_id", sa.Integer(), nullable=False),
        sa.Column("audio_path", sa.String(500), nullable=False),
        sa.Column("duration_seconds", sa.Float(), nullable=True),
        sa.Column("raw_transcription", sa.Text(), nullable=True),
        sa.Column("corrected_transcription", sa.Text(), nullable=True),
        sa.Column("error_rate", sa.Float(), nullable=True),
        sa.Column("used_for_training", sa.Boolean(), default=False),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
        ),
        sa.PrimaryKeyConstraint("id"),
        sa.ForeignKeyConstraint(
            ["user_id"],
            ["users.id"],
            ondelete="CASCADE",
        ),
    )
    op.create_index(
        "idx_audio_user_training",
        "audio_samples",
        ["user_id", "used_for_training"],
    )

    # Create correction_embeddings table
    op.create_table(
        "correction_embeddings",
        sa.Column("id", sa.Integer(), nullable=False),
        sa.Column("user_id", sa.Integer(), nullable=False),
        sa.Column("original_text", sa.Text(), nullable=False),
        sa.Column("corrected_text", sa.Text(), nullable=False),
        sa.Column("embedding", Vector(384), nullable=True),
        sa.Column("correction_type", sa.String(50), nullable=True),
        sa.Column("correction_count", sa.Integer(), default=1),
        sa.Column("audio_sample_id", sa.Integer(), nullable=True),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
        ),
        sa.PrimaryKeyConstraint("id"),
        sa.ForeignKeyConstraint(
            ["user_id"],
            ["users.id"],
            ondelete="CASCADE",
        ),
        sa.ForeignKeyConstraint(
            ["audio_sample_id"],
            ["audio_samples.id"],
            ondelete="SET NULL",
        ),
    )
    op.create_index(
        "idx_corrections_user",
        "correction_embeddings",
        ["user_id"],
    )
    # Create HNSW index for fast vector similarity search
    op.execute("""
        CREATE INDEX idx_corrections_embedding
        ON correction_embeddings
        USING hnsw (embedding vector_cosine_ops)
        WITH (m = 16, ef_construction = 64)
    """)

    # Create learning_metrics table
    op.create_table(
        "learning_metrics",
        sa.Column("id", sa.Integer(), nullable=False),
        sa.Column("user_id", sa.Integer(), nullable=False),
        sa.Column("date", sa.Date(), nullable=False),
        sa.Column("transcriptions_count", sa.Integer(), default=0),
        sa.Column("corrections_count", sa.Integer(), default=0),
        sa.Column("auto_accepted", sa.Integer(), default=0),
        sa.Column("avg_confidence", sa.Float(), nullable=True),
        sa.Column("model_version", sa.String(50), nullable=True),
        sa.PrimaryKeyConstraint("id"),
        sa.ForeignKeyConstraint(
            ["user_id"],
            ["users.id"],
            ondelete="CASCADE",
        ),
        sa.UniqueConstraint("user_id", "date", name="uq_metrics_user_date"),
    )

    # Create model_versions table
    op.create_table(
        "model_versions",
        sa.Column("id", sa.Integer(), nullable=False),
        sa.Column("user_id", sa.Integer(), nullable=False),
        sa.Column("model_type", sa.String(50), nullable=False),
        sa.Column("version", sa.Integer(), nullable=False),
        sa.Column("model_path", sa.String(500), nullable=True),
        sa.Column("training_samples", sa.Integer(), nullable=True),
        sa.Column("training_loss", sa.Float(), nullable=True),
        sa.Column("validation_wer", sa.Float(), nullable=True),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
        ),
        sa.PrimaryKeyConstraint("id"),
        sa.ForeignKeyConstraint(
            ["user_id"],
            ["users.id"],
            ondelete="CASCADE",
        ),
    )
    op.create_index(
        "idx_model_versions_user",
        "model_versions",
        ["user_id"],
    )

    # Create correction_rules table
    op.create_table(
        "correction_rules",
        sa.Column("id", sa.Integer(), nullable=False),
        sa.Column("user_id", sa.Integer(), nullable=False),
        sa.Column("pattern", sa.String(500), nullable=False),
        sa.Column("replacement", sa.String(500), nullable=False),
        sa.Column("is_regex", sa.Boolean(), default=False),
        sa.Column("priority", sa.Integer(), default=0),
        sa.Column("hit_count", sa.Integer(), default=0),
        sa.Column("is_active", sa.Boolean(), default=True),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
        ),
        sa.PrimaryKeyConstraint("id"),
        sa.ForeignKeyConstraint(
            ["user_id"],
            ["users.id"],
            ondelete="CASCADE",
        ),
    )
    op.create_index(
        "idx_correction_rules_user",
        "correction_rules",
        ["user_id"],
    )


def downgrade() -> None:
    op.drop_table("correction_rules")
    op.drop_table("model_versions")
    op.drop_table("learning_metrics")
    op.drop_table("correction_embeddings")
    op.drop_table("audio_samples")
    op.execute("DROP EXTENSION IF EXISTS vector")
