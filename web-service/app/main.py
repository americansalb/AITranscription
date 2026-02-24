"""Vaak Web Service — FastAPI entry point.

Separate from the desktop backend. Shares models/schemas via the shared/ package.
"""

import logging

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.config import settings
from app.api import auth, projects, messages, billing, providers

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

app = FastAPI(
    title=settings.app_name,
    version=settings.version,
    description="Multi-provider AI collaboration platform",
)

# CORS
app.add_middleware(
    CORSMiddleware,
    allow_origins=settings.cors_origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Routes
app.include_router(auth.router, prefix="/api/v1/auth", tags=["auth"])
app.include_router(projects.router, prefix="/api/v1/projects", tags=["projects"])
app.include_router(messages.router, prefix="/api/v1/messages", tags=["messages"])
app.include_router(billing.router, prefix="/api/v1/billing", tags=["billing"])
app.include_router(providers.router, prefix="/api/v1/providers", tags=["providers"])


@app.get("/health")
async def health_check():
    return {
        "status": "ok",
        "version": settings.version,
        "providers_configured": {
            "anthropic": bool(settings.anthropic_api_key),
            "openai": bool(settings.openai_api_key),
            "google": bool(settings.google_ai_api_key),
        },
    }
