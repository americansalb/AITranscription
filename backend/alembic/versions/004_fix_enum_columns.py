"""fix_enum_columns_to_varchar

Revision ID: 004_fix_enums
Revises: 003_gamification
Create Date: 2026-01-28

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = '004_fix_enums'
down_revision: Union[str, None] = '003_gamification'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # Convert enum columns to VARCHAR to avoid SQLAlchemy mapping issues
    # PostgreSQL will cast the enum values to text automatically

    # achievement_definitions.category
    op.execute("""
        ALTER TABLE achievement_definitions
        ALTER COLUMN category TYPE VARCHAR(50)
        USING category::text
    """)

    # achievement_definitions.rarity
    op.execute("""
        ALTER TABLE achievement_definitions
        ALTER COLUMN rarity TYPE VARCHAR(50)
        USING rarity::text
    """)

    # user_gamification.prestige_tier
    op.execute("""
        ALTER TABLE user_gamification
        ALTER COLUMN prestige_tier TYPE VARCHAR(50)
        USING prestige_tier::text
    """)

    # Drop the enum types (optional but keeps DB clean)
    op.execute('DROP TYPE IF EXISTS achievementcategory')
    op.execute('DROP TYPE IF EXISTS achievementrarity')
    op.execute('DROP TYPE IF EXISTS prestigetier')


def downgrade() -> None:
    # Recreate enum types
    op.execute("""
        CREATE TYPE achievementcategory AS ENUM (
            'volume', 'streak', 'speed', 'context', 'formality',
            'learning', 'temporal', 'records', 'combo', 'special'
        )
    """)
    op.execute("""
        CREATE TYPE achievementrarity AS ENUM (
            'common', 'rare', 'epic', 'legendary'
        )
    """)
    op.execute("""
        CREATE TYPE prestigetier AS ENUM (
            'bronze', 'silver', 'gold', 'platinum', 'diamond', 'master', 'legend'
        )
    """)

    # Convert back to enum
    op.execute("""
        ALTER TABLE achievement_definitions
        ALTER COLUMN category TYPE achievementcategory
        USING category::achievementcategory
    """)
    op.execute("""
        ALTER TABLE achievement_definitions
        ALTER COLUMN rarity TYPE achievementrarity
        USING rarity::achievementrarity
    """)
    op.execute("""
        ALTER TABLE user_gamification
        ALTER COLUMN prestige_tier TYPE prestigetier
        USING prestige_tier::prestigetier
    """)
