from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine, async_sessionmaker

from app.core.config import settings


def get_async_database_url(url: str) -> str:
    """Convert database URL to async format for SQLAlchemy + asyncpg.

    Render provides postgres:// URLs but SQLAlchemy async requires
    postgresql+asyncpg:// format.
    """
    if url.startswith("postgres://"):
        return url.replace("postgres://", "postgresql+asyncpg://", 1)
    elif url.startswith("postgresql://"):
        return url.replace("postgresql://", "postgresql+asyncpg://", 1)
    return url


# Create async engine with converted URL
database_url = get_async_database_url(settings.database_url)

engine = create_async_engine(
    database_url,
    echo=settings.debug,
    pool_pre_ping=True,
)

# Create session factory
async_session_maker = async_sessionmaker(
    engine,
    class_=AsyncSession,
    expire_on_commit=False,
)


async def get_db() -> AsyncSession:
    """Dependency that provides a database session."""
    async with async_session_maker() as session:
        try:
            yield session
        finally:
            await session.close()
