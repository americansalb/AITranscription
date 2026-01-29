"""add_gamification_tables

Revision ID: 003_gamification
Revises: bd3eeb8595a4
Create Date: 2026-01-28

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = '003_gamification'
down_revision: Union[str, None] = 'bd3eeb8595a4'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # Create achievement_definitions table
    op.create_table(
        'achievement_definitions',
        sa.Column('id', sa.String(length=100), primary_key=True),
        sa.Column('name', sa.String(length=255), nullable=False),
        sa.Column('description', sa.Text(), nullable=False),
        sa.Column('category', sa.Enum(
            'volume', 'streak', 'speed', 'context', 'formality',
            'learning', 'temporal', 'records', 'combo', 'special',
            name='achievementcategory'
        ), nullable=False),
        sa.Column('rarity', sa.Enum(
            'common', 'rare', 'epic', 'legendary',
            name='achievementrarity'
        ), nullable=False),
        sa.Column('xp_reward', sa.Integer(), nullable=False),
        sa.Column('icon', sa.String(length=50), nullable=False),
        sa.Column('tier', sa.Integer(), nullable=False),
        sa.Column('threshold', sa.Float(), nullable=False),
        sa.Column('metric_type', sa.String(length=100), nullable=False),
        sa.Column('is_hidden', sa.Boolean(), default=False, nullable=False),
        sa.Column('parent_id', sa.String(length=100), nullable=True),
    )
    op.create_index('ix_achievement_category', 'achievement_definitions', ['category'])
    op.create_index('ix_achievement_rarity', 'achievement_definitions', ['rarity'])
    op.create_index('ix_achievement_metric', 'achievement_definitions', ['metric_type'])

    # Create user_gamification table
    op.create_table(
        'user_gamification',
        sa.Column('id', sa.Integer(), primary_key=True),
        sa.Column('user_id', sa.Integer(), sa.ForeignKey('users.id', ondelete='CASCADE'), unique=True, nullable=False),
        sa.Column('current_xp', sa.Integer(), default=0, nullable=False),
        sa.Column('current_level', sa.Integer(), default=1, nullable=False),
        sa.Column('prestige_tier', sa.Enum(
            'bronze', 'silver', 'gold', 'platinum', 'diamond', 'master', 'legend',
            name='prestigetier'
        ), default='bronze', nullable=False),
        sa.Column('lifetime_xp', sa.Integer(), default=0, nullable=False),
        sa.Column('achievements_unlocked', sa.Integer(), default=0, nullable=False),
        sa.Column('xp_multiplier', sa.Float(), default=1.0, nullable=False),
        sa.Column('streak_bonus_active', sa.Boolean(), default=False, nullable=False),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False),
        sa.Column('updated_at', sa.DateTime(timezone=True), server_default=sa.func.now(), onupdate=sa.func.now(), nullable=False),
        sa.Column('last_xp_earned_at', sa.DateTime(timezone=True), nullable=True),
    )
    op.create_index('ix_user_gamification_user_id', 'user_gamification', ['user_id'])

    # Create user_achievements table
    op.create_table(
        'user_achievements',
        sa.Column('id', sa.Integer(), primary_key=True),
        sa.Column('user_id', sa.Integer(), sa.ForeignKey('users.id', ondelete='CASCADE'), nullable=False),
        sa.Column('achievement_id', sa.String(length=100), sa.ForeignKey('achievement_definitions.id', ondelete='CASCADE'), nullable=False),
        sa.Column('current_value', sa.Float(), default=0.0, nullable=False),
        sa.Column('is_unlocked', sa.Boolean(), default=False, nullable=False),
        sa.Column('unlocked_at', sa.DateTime(timezone=True), nullable=True),
        sa.Column('notified', sa.Boolean(), default=False, nullable=False),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False),
        sa.Column('updated_at', sa.DateTime(timezone=True), server_default=sa.func.now(), onupdate=sa.func.now(), nullable=False),
    )
    op.create_index('ix_user_achievements_user_id', 'user_achievements', ['user_id'])
    op.create_index('ix_user_achievements_achievement_id', 'user_achievements', ['achievement_id'])
    op.create_index('ix_user_achievement_unique', 'user_achievements', ['user_id', 'achievement_id'], unique=True)
    op.create_index('ix_user_achievement_unlocked', 'user_achievements', ['user_id', 'is_unlocked'])

    # Create xp_transactions table
    op.create_table(
        'xp_transactions',
        sa.Column('id', sa.Integer(), primary_key=True),
        sa.Column('user_gamification_id', sa.Integer(), sa.ForeignKey('user_gamification.id', ondelete='CASCADE'), nullable=False),
        sa.Column('amount', sa.Integer(), nullable=False),
        sa.Column('multiplier', sa.Float(), default=1.0, nullable=False),
        sa.Column('final_amount', sa.Integer(), nullable=False),
        sa.Column('source', sa.String(length=100), nullable=False),
        sa.Column('source_id', sa.String(length=100), nullable=True),
        sa.Column('description', sa.Text(), nullable=True),
        sa.Column('level_before', sa.Integer(), nullable=False),
        sa.Column('level_after', sa.Integer(), nullable=False),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False),
    )
    op.create_index('ix_xp_transaction_user_gamification_id', 'xp_transactions', ['user_gamification_id'])
    op.create_index('ix_xp_transaction_source', 'xp_transactions', ['source'])
    op.create_index('ix_xp_transaction_user_date', 'xp_transactions', ['user_gamification_id', 'created_at'])
    op.create_index('ix_xp_transaction_created_at', 'xp_transactions', ['created_at'])


def downgrade() -> None:
    # Drop tables in reverse order of creation
    op.drop_index('ix_xp_transaction_created_at', 'xp_transactions')
    op.drop_index('ix_xp_transaction_user_date', 'xp_transactions')
    op.drop_index('ix_xp_transaction_source', 'xp_transactions')
    op.drop_index('ix_xp_transaction_user_gamification_id', 'xp_transactions')
    op.drop_table('xp_transactions')

    op.drop_index('ix_user_achievement_unlocked', 'user_achievements')
    op.drop_index('ix_user_achievement_unique', 'user_achievements')
    op.drop_index('ix_user_achievements_achievement_id', 'user_achievements')
    op.drop_index('ix_user_achievements_user_id', 'user_achievements')
    op.drop_table('user_achievements')

    op.drop_index('ix_user_gamification_user_id', 'user_gamification')
    op.drop_table('user_gamification')

    op.drop_index('ix_achievement_metric', 'achievement_definitions')
    op.drop_index('ix_achievement_rarity', 'achievement_definitions')
    op.drop_index('ix_achievement_category', 'achievement_definitions')
    op.drop_table('achievement_definitions')

    # Drop enums
    op.execute('DROP TYPE IF EXISTS achievementcategory')
    op.execute('DROP TYPE IF EXISTS achievementrarity')
    op.execute('DROP TYPE IF EXISTS prestigetier')
