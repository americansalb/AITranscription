#!/usr/bin/env python3
"""
Seed script to create the default admin user.
Run with: python -m scripts.seed_admin
"""
import asyncio
import sys
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent))

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.database import async_session_maker
from app.models.user import User, SubscriptionTier
from app.services.auth import hash_password, get_user_by_email


# Default admin credentials
ADMIN_EMAIL = "kenil.thakkar@gmail.com"
ADMIN_PASSWORD = "winner"
ADMIN_NAME = "Kenil Thakkar"


async def seed_admin():
    """Create the default admin user if they don't exist."""
    async with async_session_maker() as db:
        # Check if user already exists
        existing_user = await get_user_by_email(db, ADMIN_EMAIL)

        if existing_user:
            # Update existing user to admin
            existing_user.is_admin = True
            existing_user.tier = SubscriptionTier.DEVELOPER
            existing_user.daily_transcription_limit = 0  # Unlimited
            existing_user.is_active = True
            await db.commit()
            print(f"Updated existing user {ADMIN_EMAIL} to admin with developer tier")
        else:
            # Create new admin user
            admin_user = User(
                email=ADMIN_EMAIL,
                hashed_password=hash_password(ADMIN_PASSWORD),
                full_name=ADMIN_NAME,
                is_admin=True,
                tier=SubscriptionTier.DEVELOPER,
                daily_transcription_limit=0,  # Unlimited
                is_active=True,
            )
            db.add(admin_user)
            await db.commit()
            print(f"Created admin user: {ADMIN_EMAIL}")

        print("Admin user details:")
        print(f"  Email: {ADMIN_EMAIL}")
        print(f"  Password: {ADMIN_PASSWORD}")
        print(f"  Tier: DEVELOPER (unlimited usage)")
        print(f"  Admin: Yes")


if __name__ == "__main__":
    asyncio.run(seed_admin())
