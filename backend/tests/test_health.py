"""Tests for the health check and root endpoints."""
import pytest


async def test_root_endpoint(client):
    """Root endpoint returns API info."""
    response = await client.get("/")
    assert response.status_code == 200
    data = response.json()
    assert data["name"] == "Vaak"
    assert "version" in data
    assert data["health"] == "/api/v1/health"


async def test_health_check(client):
    """Health endpoint returns status and config flags."""
    response = await client.get("/api/v1/health")
    assert response.status_code == 200
    data = response.json()
    assert data["status"] == "healthy"
    assert data["version"] == "0.1.0"
    assert "groq_configured" in data
    assert "anthropic_configured" in data
