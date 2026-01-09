#!/usr/bin/env python3
"""
One-time database fix script.
Run this ONCE in Render shell: python fix_db.py
"""
import asyncio
import os
import asyncpg


async def fix_database():
    """Add missing columns to users table."""
    database_url = os.getenv("DATABASE_URL")
    if not database_url:
        print("ERROR: DATABASE_URL not set")
        return

    # Convert postgres:// to postgresql:// for asyncpg
    if database_url.startswith("postgres://"):
        database_url = database_url.replace("postgres://", "postgresql://", 1)

    print(f"Connecting to database...")
    conn = await asyncpg.connect(database_url)

    try:
        # Check if users table exists
        exists = await conn.fetchval(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'users')"
        )
        if not exists:
            print("ERROR: users table doesn't exist!")
            return

        print("Users table found. Checking columns...")

        # Get current columns
        rows = await conn.fetch(
            "SELECT column_name FROM information_schema.columns "
            "WHERE table_name = 'users' ORDER BY ordinal_position"
        )
        current_columns = [row['column_name'] for row in rows]
        print(f"Current columns: {', '.join(current_columns)}")

        # Columns that should exist based on migrations 001 and 002
        required_columns = {
            'is_admin': 'ALTER TABLE users ADD COLUMN is_admin BOOLEAN NOT NULL DEFAULT false',
            'total_audio_seconds': 'ALTER TABLE users ADD COLUMN total_audio_seconds INTEGER NOT NULL DEFAULT 0',
            'total_polish_tokens': 'ALTER TABLE users ADD COLUMN total_polish_tokens INTEGER NOT NULL DEFAULT 0',
            'total_transcriptions': 'ALTER TABLE users ADD COLUMN total_transcriptions INTEGER NOT NULL DEFAULT 0',
            'total_words': 'ALTER TABLE users ADD COLUMN total_words INTEGER NOT NULL DEFAULT 0',
            'typing_wpm': 'ALTER TABLE users ADD COLUMN typing_wpm INTEGER NOT NULL DEFAULT 40',
        }

        # Add missing columns
        added = []
        for col, sql in required_columns.items():
            if col not in current_columns:
                print(f"Adding missing column: {col}")
                await conn.execute(sql)
                added.append(col)
            else:
                print(f"✓ Column {col} already exists")

        if added:
            print(f"\n✅ Added columns: {', '.join(added)}")
            print("Database fixed! Login should work now.")
        else:
            print("\n✅ All columns already exist. Database is correct!")

    finally:
        await conn.close()


if __name__ == "__main__":
    asyncio.run(fix_database())
