"""Billing endpoints — Stripe integration, subscription management."""

import logging

from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.config import settings
from app.database import get_db
from app.models import SubscriptionTier, WebUser

logger = logging.getLogger(__name__)
router = APIRouter()


# --- Schemas ---

class CreateCheckoutRequest(BaseModel):
    plan: str  # "pro" or "byok"


# --- Endpoints ---

@router.get("/status")
async def get_subscription_status(user: WebUser = Depends(get_current_user)):
    """Get current user's subscription status and usage."""
    limit = settings.free_tier_monthly_tokens
    if user.tier == SubscriptionTier.PRO:
        limit = settings.pro_tier_monthly_tokens
    elif user.tier == SubscriptionTier.BYOK:
        limit = 999_999_999

    subscription_status = "none"
    if user.stripe_subscription_id:
        try:
            import stripe
            stripe.api_key = settings.stripe_secret_key
            sub = stripe.Subscription.retrieve(user.stripe_subscription_id)
            subscription_status = sub.status
        except Exception as e:
            logger.error("Failed to fetch Stripe subscription: %s", e)
            subscription_status = "unknown"

    is_active = subscription_status == "active" or user.tier != SubscriptionTier.FREE

    return {
        "active": is_active,
        "plan": user.tier.value,
        "usage": {
            "tokens_used": user.monthly_tokens_used,
            "tokens_limit": limit,
            "cost_usd": round(user.monthly_cost_usd, 4),
        },
    }


@router.post("/checkout")
async def create_checkout(
    request: CreateCheckoutRequest,
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Create a Stripe Checkout session for upgrading."""
    if not settings.stripe_secret_key:
        raise HTTPException(status_code=503, detail="Billing not configured")

    plan = request.plan
    if plan not in ("pro", "byok"):
        raise HTTPException(status_code=400, detail="Invalid plan. Choose 'pro' or 'byok'.")

    price_id = settings.stripe_price_pro if plan == "pro" else settings.stripe_price_byok
    if not price_id:
        raise HTTPException(status_code=503, detail=f"Price not configured for {plan} plan")

    # Build callback URLs from CORS origins
    base_url = settings.cors_origins[0] if settings.cors_origins else "http://localhost:3000"
    success_url = f"{base_url}/billing?session_id={{CHECKOUT_SESSION_ID}}"
    cancel_url = f"{base_url}/billing"

    try:
        import stripe
        stripe.api_key = settings.stripe_secret_key

        # Get or create Stripe customer
        if not user.stripe_customer_id:
            customer = stripe.Customer.create(
                email=user.email,
                metadata={"user_id": str(user.id)},
            )
            user.stripe_customer_id = customer.id
            await db.commit()

        session = stripe.checkout.Session.create(
            customer=user.stripe_customer_id,
            payment_method_types=["card"],
            line_items=[{"price": price_id, "quantity": 1}],
            mode="subscription",
            success_url=success_url,
            cancel_url=cancel_url,
            metadata={"user_id": str(user.id), "tier": plan},
        )

        return {"url": session.url}

    except Exception as e:
        logger.error("Stripe checkout creation failed: %s", e)
        raise HTTPException(status_code=500, detail="Failed to create checkout session")


@router.post("/portal")
async def customer_portal(
    db: AsyncSession = Depends(get_db),
    user: WebUser = Depends(get_current_user),
):
    """Redirect to Stripe Customer Portal for managing subscription."""
    if not user.stripe_customer_id:
        raise HTTPException(status_code=400, detail="No active subscription to manage")

    try:
        import stripe
        stripe.api_key = settings.stripe_secret_key

        session = stripe.billing_portal.Session.create(
            customer=user.stripe_customer_id,
            return_url=settings.cors_origins[0] if settings.cors_origins else "http://localhost:3000",
        )
        return {"portal_url": session.url}

    except Exception as e:
        logger.error("Stripe portal creation failed: %s", e)
        raise HTTPException(status_code=500, detail="Failed to create portal session")


@router.post("/webhook")
async def stripe_webhook(request: Request, db: AsyncSession = Depends(get_db)):
    """Handle Stripe webhook events."""
    if not settings.stripe_webhook_secret:
        raise HTTPException(status_code=503, detail="Webhook not configured")

    body = await request.body()
    sig = request.headers.get("stripe-signature")
    if not sig:
        raise HTTPException(status_code=400, detail="Missing signature")

    try:
        import stripe
        stripe.api_key = settings.stripe_secret_key

        event = stripe.Webhook.construct_event(
            body, sig, settings.stripe_webhook_secret
        )
    except Exception as e:
        logger.error("Webhook signature verification failed: %s", e)
        raise HTTPException(status_code=400, detail="Invalid signature")

    # Handle relevant events
    event_type = event["type"]
    data = event["data"]["object"]

    if event_type == "checkout.session.completed":
        await _handle_checkout_completed(db, data)
    elif event_type == "customer.subscription.updated":
        await _handle_subscription_updated(db, data)
    elif event_type == "customer.subscription.deleted":
        await _handle_subscription_deleted(db, data)
    elif event_type == "invoice.payment_failed":
        logger.warning("Payment failed for customer %s", data.get("customer"))

    return {"received": True}


# --- Webhook handlers ---

async def _handle_checkout_completed(db: AsyncSession, data: dict):
    user_id = data.get("metadata", {}).get("user_id")
    tier = data.get("metadata", {}).get("tier", "pro")
    subscription_id = data.get("subscription")

    if not user_id:
        logger.error("Checkout completed with no user_id in metadata")
        return

    result = await db.execute(select(WebUser).where(WebUser.id == int(user_id)))
    user = result.scalar_one_or_none()
    if not user:
        logger.error("Checkout completed for unknown user %s", user_id)
        return

    user.tier = SubscriptionTier.PRO if tier == "pro" else SubscriptionTier.BYOK
    user.stripe_subscription_id = subscription_id
    await db.commit()
    logger.info("User %s upgraded to %s tier", user_id, tier)


async def _handle_subscription_updated(db: AsyncSession, data: dict):
    sub_id = data.get("id")
    result = await db.execute(
        select(WebUser).where(WebUser.stripe_subscription_id == sub_id)
    )
    user = result.scalar_one_or_none()
    if not user:
        return

    status = data.get("status")
    if status == "canceled" or status == "unpaid":
        user.tier = SubscriptionTier.FREE
        user.stripe_subscription_id = None
        await db.commit()
        logger.info("User %d downgraded to free (subscription %s)", user.id, status)


async def _handle_subscription_deleted(db: AsyncSession, data: dict):
    sub_id = data.get("id")
    result = await db.execute(
        select(WebUser).where(WebUser.stripe_subscription_id == sub_id)
    )
    user = result.scalar_one_or_none()
    if not user:
        return

    user.tier = SubscriptionTier.FREE
    user.stripe_subscription_id = None
    await db.commit()
    logger.info("User %d subscription deleted, downgraded to free", user.id)
