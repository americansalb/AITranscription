"""
Script to seed achievement definitions into the database.
Run with: python -m scripts.seed_achievements
"""

import asyncio
import sys
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent))

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.database import async_session_maker, engine
# Import all models to ensure relationships are resolved
from app.models import user, dictionary, learning, gamification, transcript  # noqa: F401
from app.models.gamification import AchievementDefinition
from app.services.achievement_seeder import generate_all_achievements, get_achievement_count


async def seed_achievements(force: bool = False):
    """Seed all achievement definitions into the database."""
    async with async_session_maker() as session:
        # Check if already seeded
        result = await session.execute(select(AchievementDefinition).limit(1))
        existing = result.scalar_one_or_none()

        if existing and not force:
            # Count existing
            result = await session.execute(
                select(AchievementDefinition.id)
            )
            count = len(result.all())
            print(f"Achievements already seeded ({count} definitions). Use --force to re-seed.")
            return

        if existing and force:
            # Delete all existing
            await session.execute(
                AchievementDefinition.__table__.delete()
            )
            print("Cleared existing achievement definitions.")

        # Generate and insert all achievements
        print("Generating achievement definitions...")
        achievements = generate_all_achievements()
        print(f"Generated {len(achievements)} achievement definitions.")

        print("Inserting into database...")
        for i, achievement_data in enumerate(achievements):
            # Convert enum objects to their lowercase string values
            category = achievement_data["category"]
            rarity = achievement_data["rarity"]
            # Enums inherit from str, so category.value gives lowercase like "volume"
            category_val = category.value if hasattr(category, "value") else str(category).lower()
            rarity_val = rarity.value if hasattr(rarity, "value") else str(rarity).lower()

            # Debug first one
            if i == 0:
                print(f"  Debug: category={category}, value={category_val}")
                print(f"  Debug: rarity={rarity}, value={rarity_val}")

            achievement = AchievementDefinition(
                id=achievement_data["id"],
                name=achievement_data["name"],
                description=achievement_data["description"],
                category=category_val,
                rarity=rarity_val,
                xp_reward=achievement_data["xp_reward"],
                icon=achievement_data["icon"],
                tier=achievement_data["tier"],
                threshold=achievement_data["threshold"],
                metric_type=achievement_data["metric_type"],
                is_hidden=achievement_data.get("is_hidden", False),
                parent_id=achievement_data.get("parent_id"),
            )
            session.add(achievement)

            # Batch commit every 100 records
            if (i + 1) % 100 == 0:
                await session.flush()
                print(f"  Inserted {i + 1}/{len(achievements)}...")

        await session.commit()
        print(f"Successfully seeded {len(achievements)} achievement definitions!")

        # Print summary by category
        print("\nSummary by category:")
        category_counts = {}
        rarity_counts = {"common": 0, "rare": 0, "epic": 0, "legendary": 0}
        for a in achievements:
            cat = a["category"].value if hasattr(a["category"], "value") else str(a["category"])
            category_counts[cat] = category_counts.get(cat, 0) + 1
            rar = a["rarity"].value if hasattr(a["rarity"], "value") else str(a["rarity"])
            rarity_counts[rar] = rarity_counts.get(rar, 0) + 1

        for cat, count in sorted(category_counts.items()):
            print(f"  {cat}: {count}")

        print("\nSummary by rarity:")
        for rar, count in rarity_counts.items():
            print(f"  {rar}: {count}")


def main():
    force = "--force" in sys.argv
    asyncio.run(seed_achievements(force))


if __name__ == "__main__":
    main()
