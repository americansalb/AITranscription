"""merge_heads

Revision ID: 0b161e9c498a
Revises: 001_learning, 002
Create Date: 2026-01-21 19:28:38.363430

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa


# revision identifiers, used by Alembic.
revision: str = '0b161e9c498a'
down_revision: Union[str, None] = ('001_learning', '002')
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    pass


def downgrade() -> None:
    pass
