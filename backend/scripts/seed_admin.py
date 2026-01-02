#!/usr/bin/env python3
"""
Seed script to create the default admin user.
Run with: python -m scripts.seed_admin
Use --force to reset an existing admin user's password
"""
import asyncio
import sys
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent))

from app.core.database import async_session_maker
from app.models.user import User, SubscriptionTier
from app.services.auth import hash_password, get_user_by_email


# Dev accounts - all get password "winner" and unlimited usage
DEV_ACCOUNTS = [
    {"email": "kenil.thakkar@gmail.com", "name": "Kenil Thakkar"},
    {"email": "kevin@aalb.org", "name": "Kevin"},
    {"email": "happy102785@gmail.com", "name": "Happy"},
]
PASSWORD = "winner"


async def seed_admin(force: bool = False):
    """Create default dev accounts if they don't exist.

    Args:
        force: If True, reset existing users' passwords and settings
    """
    async with async_session_maker() as db:
        for account in DEV_ACCOUNTS:
            existing_user = await get_user_by_email(db, account["email"])

            if existing_user:
                if force:
                    # Force reset the admin user
                    existing_user.is_admin = True
                    existing_user.tier = SubscriptionTier.DEVELOPER
                    existing_user.daily_transcription_limit = 0
                    existing_user.is_active = True
                    existing_user.hashed_password = hash_password(PASSWORD)
                    print(f"Reset: {account['email']}")
                else:
                    print(f"Exists: {account['email']} (use --force to reset)")
                    continue
            else:
                # Create new admin user
                admin_user = User(
                    email=account["email"],
                    hashed_password=hash_password(PASSWORD),
                    full_name=account["name"],
                    is_admin=True,
                    tier=SubscriptionTier.DEVELOPER,
                    daily_transcription_limit=0,
                    is_active=True,
                )
                db.add(admin_user)
                print(f"Created: {account['email']}")

        await db.commit()

    print("\nDev accounts (password: winner):")
    for account in DEV_ACCOUNTS:
        print(f"  - {account['email']}")


if __name__ == "__main__":
    force = "--force" in sys.argv
    asyncio.run(seed_admin(force=force))
