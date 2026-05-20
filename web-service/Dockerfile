FROM python:3.13-slim AS base

WORKDIR /app

# System deps for asyncpg, bcrypt
RUN apt-get update && apt-get install -y --no-install-recommends \
    libpq-dev gcc && \
    rm -rf /var/lib/apt/lists/*

# Install shared package first (vaak-shared)
COPY shared/ /app/shared/
RUN pip install --no-cache-dir /app/shared/

# Install web-service deps
COPY web-service/pyproject.toml /app/web-service/
RUN pip install --no-cache-dir /app/web-service/

# Copy app code
COPY web-service/app/ /app/web-service/app/
COPY web-service/alembic/ /app/web-service/alembic/
COPY web-service/alembic.ini /app/web-service/

WORKDIR /app/web-service

# Run migrations then start uvicorn
# NOTE: --workers 1 is REQUIRED — agent runtime uses in-memory state
CMD ["sh", "-c", "alembic upgrade head && uvicorn app.main:app --host 0.0.0.0 --port 8000 --workers 1"]

EXPOSE 8000
