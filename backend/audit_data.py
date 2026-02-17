"""Audit all data sources to find contradictions."""
import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from dotenv import load_dotenv
load_dotenv()

from sqlalchemy import text
from app.core.database import engine


async def audit():
    async with engine.begin() as conn:
        user_id = 1  # kevin@aalb.org

        # 1. Raw transcript counts by type
        r = await conn.execute(text("""
            SELECT transcript_type, COUNT(*), COALESCE(SUM(word_count), 0)::bigint,
                   COALESCE(SUM(audio_duration_seconds), 0)::bigint
            FROM transcripts WHERE user_id = :uid
            GROUP BY transcript_type
        """), {"uid": user_id})
        print("=== TRANSCRIPTS BY TYPE ===")
        for row in r.all():
            print(f"  {row[0]}: {row[1]} transcripts, {row[2]} words, {row[3]}s audio")

        # 2. Total across ALL types
        r = await conn.execute(text("""
            SELECT COUNT(*), COALESCE(SUM(word_count), 0)::bigint
            FROM transcripts WHERE user_id = :uid
        """), {"uid": user_id})
        row = r.one()
        print(f"\n=== ALL TRANSCRIPTS: {row[0]} transcripts, {row[1]} words ===")

        # 3. User model stored values
        r = await conn.execute(text("""
            SELECT total_words, total_transcriptions, total_audio_seconds
            FROM users WHERE id = :uid
        """), {"uid": user_id})
        row = r.one()
        print(f"\n=== USER MODEL: {row[0]} words, {row[1]} transcriptions, {row[2]}s audio ===")

        # 4. Daily word record
        r = await conn.execute(text("""
            SELECT date(created_at), SUM(word_count)::bigint as daily_words
            FROM transcripts WHERE user_id = :uid AND transcript_type = 'input'
            GROUP BY date(created_at)
            ORDER BY daily_words DESC LIMIT 5
        """), {"uid": user_id})
        print("\n=== TOP 5 DAILY WORD COUNTS (input only) ===")
        for row in r.all():
            print(f"  {row[0]}: {row[1]} words")

        # 5. Weekly word record
        r = await conn.execute(text("""
            SELECT extract(isoyear from created_at)::int, extract(week from created_at)::int,
                   SUM(word_count)::bigint
            FROM transcripts WHERE user_id = :uid AND transcript_type = 'input'
            GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 3
        """), {"uid": user_id})
        print("\n=== TOP 3 WEEKLY WORD COUNTS (input only) ===")
        for row in r.all():
            print(f"  Year {row[0]} Week {row[1]}: {row[2]} words")

        # 6. Monthly word record
        r = await conn.execute(text("""
            SELECT extract(year from created_at)::int, extract(month from created_at)::int,
                   SUM(word_count)::bigint
            FROM transcripts WHERE user_id = :uid AND transcript_type = 'input'
            GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 3
        """), {"uid": user_id})
        print("\n=== TOP 3 MONTHLY WORD COUNTS (input only) ===")
        for row in r.all():
            print(f"  {row[0]}-{row[1]:02d}: {row[2]} words")

        # 7. WPM outliers still remaining
        r = await conn.execute(text("""
            SELECT id, word_count, audio_duration_seconds, words_per_minute
            FROM transcripts WHERE user_id = :uid AND words_per_minute > 250
            ORDER BY words_per_minute DESC LIMIT 10
        """), {"uid": user_id})
        print("\n=== WPM OUTLIERS (>250) ===")
        rows = r.all()
        if not rows:
            print("  None! All clean.")
        for row in rows:
            print(f"  id={row[0]}: {row[1]} words, {row[2]:.1f}s, {row[3]:.0f} WPM")

        # 8. Unlocked achievements that may be bogus
        r = await conn.execute(text("""
            SELECT ad.name, ad.metric_key, ad.threshold, ua.current_value
            FROM user_achievements ua
            JOIN achievement_definitions ad ON ad.id = ua.achievement_id
            WHERE ua.user_id = :uid
            ORDER BY ua.unlocked_at DESC
        """), {"uid": user_id})
        print("\n=== UNLOCKED ACHIEVEMENTS ===")
        for row in r.all():
            print(f"  {row[0]}: metric={row[1]}, threshold={row[2]}, value_at_unlock={row[3]}")

    print("\nDone!")


if __name__ == "__main__":
    asyncio.run(audit())
