from pydantic import BaseModel


class ErrorResponse(BaseModel):
    """Standard error response."""

    error: str
    detail: str | None = None


class HealthResponse(BaseModel):
    """Response from the health check endpoint."""

    status: str
    version: str
    providers_configured: dict[str, bool] = {}
