"""Billing endpoints — Stripe integration, subscription management, usage dashboard."""

from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel

router = APIRouter()


class SubscriptionStatus(BaseModel):
    tier: str  # "free", "pro", "byok"
    status: str  # "active", "past_due", "canceled"
    current_period_end: str | None = None
    usage_tokens: int = 0
    usage_limit_tokens: int = 0


class CreateCheckoutRequest(BaseModel):
    tier: str  # "pro" or "byok"
    success_url: str
    cancel_url: str


class CheckoutResponse(BaseModel):
    checkout_url: str


# --- Endpoints ---

@router.get("/subscription", response_model=SubscriptionStatus)
async def get_subscription():
    """Get current user's subscription status and usage."""
    # TODO: fetch from Stripe + usage DB
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.post("/checkout", response_model=CheckoutResponse)
async def create_checkout(request: CreateCheckoutRequest):
    """Create a Stripe Checkout session for upgrading."""
    # TODO: create Stripe Checkout session
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.post("/portal")
async def customer_portal():
    """Redirect to Stripe Customer Portal for managing subscription."""
    # TODO: create Stripe portal session
    raise HTTPException(status_code=501, detail="Not implemented yet")


@router.post("/webhook")
async def stripe_webhook(request: Request):
    """Handle Stripe webhook events (subscription changes, payment failures)."""
    # TODO: verify webhook signature, handle events
    raise HTTPException(status_code=501, detail="Not implemented yet")
