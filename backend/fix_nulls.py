import asyncio
from sqlalchemy import text
from app.core.database import async_session_maker

async def fix():
    async with async_session_maker() as db:
        result1 = await db.execute(text("UPDATE transcripts SET formality = 'neutral' WHERE formality IS NULL"))
        result2 = await db.execute(text("UPDATE transcripts SET transcript_type = 'input' WHERE transcript_type IS NULL"))
        await db.commit()
        print(f"Fixed {result1.rowcount} NULL formality values")
        print(f"Fixed {result2.rowcount} NULL transcript_type values")

asyncio.run(fix())
