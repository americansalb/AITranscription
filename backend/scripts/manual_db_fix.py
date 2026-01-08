#!/usr/bin/env python3
"""
Manual database fix - adds missing columns without touching migrations.
Run this ONCE in the Render shell to fix login without breaking anything.
"""
import asyncio
from sqlalchemy import text
from app.core.database import engine

async def fix_database():
    """Add missing columns that are preventing login."""
    async with engine.begin() as conn:
        print("Checking database state...")

        # Check if users table exists
        result = await conn.execute(text(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'users')"
        ))
        if not result.scalar():
            print("ERROR: users table doesn't exist! Database is empty.")
            return

        print("Users table exists. Checking columns...")

        # List current columns
        result = await conn.execute(text(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'users' ORDER BY ordinal_position"
        ))
        current_columns = [row[0] for row in result.fetchall()]
        print(f"Current columns: {current_columns}")

        # Required columns for login to work
        required_columns = {
            'is_admin': 'ALTER TABLE users ADD COLUMN is_admin BOOLEAN NOT NULL DEFAULT false',
            'total_transcriptions': 'ALTER TABLE users ADD COLUMN total_transcriptions INTEGER NOT NULL DEFAULT 0',
            'total_words': 'ALTER TABLE users ADD COLUMN total_words INTEGER NOT NULL DEFAULT 0',
            'typing_wpm': 'ALTER TABLE users ADD COLUMN typing_wpm INTEGER NOT NULL DEFAULT 40',
            'daily_transcription_limit': 'ALTER TABLE users ADD COLUMN daily_transcription_limit INTEGER NOT NULL DEFAULT 100',
        }

        # Add missing columns
        for col, sql in required_columns.items():
            if col not in current_columns:
                print(f"Adding missing column: {col}")
                await conn.execute(text(sql))
            else:
                print(f"Column {col} already exists")

        print("\nDatabase fix complete! Login should work now.")

if __name__ == "__main__":
    asyncio.run(fix_database())
