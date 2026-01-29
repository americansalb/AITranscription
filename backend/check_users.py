import asyncio
from sqlalchemy import text
from app.core.database import async_session_maker

async def check():
    async with async_session_maker() as db:
        result = await db.execute(text('SELECT id, email FROM users LIMIT 5'))
        rows = result.fetchall()
        print('Users in database:')
        for r in rows:
            print(f'  ID: {r[0]}, Email: {r[1]}')

asyncio.run(check())
