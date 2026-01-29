import asyncio
from sqlalchemy import text
from app.core.database import async_session_maker

async def fix():
    async with async_session_maker() as db:
        # Check transcript 827
        result = await db.execute(text("SELECT id, formality, transcript_type FROM transcripts WHERE id = 827"))
        row = result.fetchone()
        if row:
            print(f"Transcript 827: formality={row[1]}, transcript_type={row[2]}")
        
        # Fix all NULL values
        result1 = await db.execute(text("UPDATE transcripts SET formality = 'neutral' WHERE formality IS NULL"))
        result2 = await db.execute(text("UPDATE transcripts SET transcript_type = 'input' WHERE transcript_type IS NULL"))
        await db.commit()
        print(f"Fixed {result1.rowcount} NULL formality values")
        print(f"Fixed {result2.rowcount} NULL transcript_type values")
        
        # Check again
        result = await db.execute(text("SELECT id, formality, transcript_type FROM transcripts WHERE id = 827"))
        row = result.fetchone()
        if row:
            print(f"After fix - Transcript 827: formality={row[1]}, transcript_type={row[2]}")

asyncio.run(fix())
