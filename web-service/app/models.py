"""SQLAlchemy models for the web service."""

import enum
from datetime import datetime, timezone

from sqlalchemy import (
    Boolean,
    DateTime,
    Enum,
    Float,
    ForeignKey,
    Integer,
    JSON,
    String,
    Text,
    UniqueConstraint,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base


def _utcnow() -> datetime:
    return datetime.now(timezone.utc)


class SubscriptionTier(str, enum.Enum):
    FREE = "free"
    PRO = "pro"
    BYOK = "byok"


class WebUser(Base):
    """User account for the web service."""

    __tablename__ = "web_users"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    email: Mapped[str] = mapped_column(String(255), unique=True, nullable=False, index=True)
    hashed_password: Mapped[str] = mapped_column(String(255), nullable=False)
    full_name: Mapped[str | None] = mapped_column(String(255), nullable=True)
    tier: Mapped[SubscriptionTier] = mapped_column(
        Enum(SubscriptionTier), default=SubscriptionTier.FREE, nullable=False
    )
    is_active: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)

    # Stripe
    stripe_customer_id: Mapped[str | None] = mapped_column(String(255), nullable=True)
    stripe_subscription_id: Mapped[str | None] = mapped_column(String(255), nullable=True)

    # BYOK keys (encrypted at rest in production)
    byok_anthropic_key: Mapped[str | None] = mapped_column(String(512), nullable=True)
    byok_openai_key: Mapped[str | None] = mapped_column(String(512), nullable=True)
    byok_google_key: Mapped[str | None] = mapped_column(String(512), nullable=True)

    # Usage tracking (monthly)
    monthly_tokens_used: Mapped[int] = mapped_column(Integer, default=0, nullable=False)
    monthly_cost_usd: Mapped[float] = mapped_column(Float, default=0.0, nullable=False)
    usage_reset_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)

    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )

    # Relationships
    projects: Mapped[list["Project"]] = relationship(back_populates="owner", lazy="selectin")


class Project(Base):
    """A collaboration project owned by a user."""

    __tablename__ = "web_projects"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    name: Mapped[str] = mapped_column(String(100), nullable=False)
    owner_id: Mapped[int] = mapped_column(ForeignKey("web_users.id"), nullable=False)
    is_active: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )

    # Relationships
    owner: Mapped["WebUser"] = relationship(back_populates="projects")
    roles: Mapped[list["ProjectRole"]] = relationship(back_populates="project", lazy="selectin")
    messages: Mapped[list["Message"]] = relationship(back_populates="project", lazy="noload")


class ProjectRole(Base):
    """A role configuration within a project (e.g., developer, architect)."""

    __tablename__ = "web_project_roles"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    project_id: Mapped[int] = mapped_column(ForeignKey("web_projects.id"), nullable=False)
    slug: Mapped[str] = mapped_column(String(50), nullable=False)
    title: Mapped[str] = mapped_column(String(100), nullable=False)
    briefing: Mapped[str] = mapped_column(Text, default="", nullable=False)

    # Provider assignment
    provider: Mapped[str] = mapped_column(String(50), default="anthropic", nullable=False)
    model: Mapped[str] = mapped_column(String(100), default="claude-sonnet-4-6", nullable=False)
    max_instances: Mapped[int] = mapped_column(Integer, default=1, nullable=False)

    # Agent state
    is_agent_running: Mapped[bool] = mapped_column(Boolean, default=False, nullable=False)

    project: Mapped["Project"] = relationship(back_populates="roles")

    __table_args__ = (
        UniqueConstraint("project_id", "slug", name="uq_project_role_slug"),
    )


class Message(Base):
    """A message on the project board."""

    __tablename__ = "web_messages"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    project_id: Mapped[int] = mapped_column(ForeignKey("web_projects.id"), nullable=False, index=True)
    from_role: Mapped[str] = mapped_column(String(100), nullable=False)
    to_role: Mapped[str] = mapped_column(String(100), nullable=False)
    msg_type: Mapped[str] = mapped_column(String(50), nullable=False)
    subject: Mapped[str] = mapped_column(String(500), default="", nullable=False)
    body: Mapped[str] = mapped_column(Text, nullable=False)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False, index=True
    )

    project: Mapped["Project"] = relationship(back_populates="messages")


class UsageRecord(Base):
    """Per-request usage tracking for billing."""

    __tablename__ = "web_usage_records"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    user_id: Mapped[int] = mapped_column(ForeignKey("web_users.id"), nullable=False, index=True)
    project_id: Mapped[int] = mapped_column(ForeignKey("web_projects.id"), nullable=False)
    model: Mapped[str] = mapped_column(String(100), nullable=False)
    provider: Mapped[str] = mapped_column(String(50), nullable=False)
    input_tokens: Mapped[int] = mapped_column(Integer, nullable=False)
    output_tokens: Mapped[int] = mapped_column(Integer, nullable=False)
    raw_cost_usd: Mapped[float] = mapped_column(Float, nullable=False)
    marked_up_cost_usd: Mapped[float] = mapped_column(Float, nullable=False)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )


class DiscussionMode(str, enum.Enum):
    DELPHI = "delphi"
    OXFORD = "oxford"
    RED_TEAM = "red_team"
    CONTINUOUS = "continuous"


class DiscussionPhase(str, enum.Enum):
    PREPARING = "preparing"
    SUBMITTING = "submitting"
    AGGREGATING = "aggregating"
    REVIEWING = "reviewing"
    PAUSED = "paused"
    COMPLETE = "complete"


class Discussion(Base):
    """A structured discussion within a project."""

    __tablename__ = "web_discussions"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    project_id: Mapped[int] = mapped_column(ForeignKey("web_projects.id"), nullable=False, index=True)
    mode: Mapped[DiscussionMode] = mapped_column(Enum(DiscussionMode), nullable=False)
    topic: Mapped[str] = mapped_column(Text, nullable=False)
    is_active: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    phase: Mapped[DiscussionPhase] = mapped_column(
        Enum(DiscussionPhase), default=DiscussionPhase.PREPARING, nullable=False
    )
    moderator: Mapped[str | None] = mapped_column(String(100), nullable=True)
    participants: Mapped[dict] = mapped_column(JSON, default=list, nullable=False)
    current_round: Mapped[int] = mapped_column(Integer, default=0, nullable=False)

    # Settings
    max_rounds: Mapped[int] = mapped_column(Integer, default=10, nullable=False)
    timeout_minutes: Mapped[int] = mapped_column(Integer, default=15, nullable=False)
    auto_close_timeout_seconds: Mapped[int] = mapped_column(Integer, default=60, nullable=False)

    # Oxford mode teams
    teams: Mapped[dict | None] = mapped_column(JSON, nullable=True)

    started_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )
    ended_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)

    # Relationships
    rounds: Mapped[list["DiscussionRound"]] = relationship(
        back_populates="discussion", lazy="selectin", order_by="DiscussionRound.number"
    )


class DiscussionRound(Base):
    """A single round within a discussion."""

    __tablename__ = "web_discussion_rounds"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    discussion_id: Mapped[int] = mapped_column(ForeignKey("web_discussions.id"), nullable=False)
    number: Mapped[int] = mapped_column(Integer, nullable=False)
    topic: Mapped[str | None] = mapped_column(Text, nullable=True)

    # Continuous review trigger info
    auto_triggered: Mapped[bool] = mapped_column(Boolean, default=False, nullable=False)
    trigger_from: Mapped[str | None] = mapped_column(String(100), nullable=True)
    trigger_message_id: Mapped[int | None] = mapped_column(Integer, nullable=True)

    opened_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )
    closed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)

    # Aggregate result (JSON: tally, anonymized submissions, etc.)
    aggregate: Mapped[dict | None] = mapped_column(JSON, nullable=True)
    aggregate_message_id: Mapped[int | None] = mapped_column(Integer, nullable=True)

    discussion: Mapped["Discussion"] = relationship(back_populates="rounds")
    submissions: Mapped[list["DiscussionSubmission"]] = relationship(
        back_populates="round", lazy="selectin"
    )

    __table_args__ = (
        UniqueConstraint("discussion_id", "number", name="uq_discussion_round_number"),
    )


class DiscussionSubmission(Base):
    """A participant's submission in a discussion round."""

    __tablename__ = "web_discussion_submissions"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    round_id: Mapped[int] = mapped_column(ForeignKey("web_discussion_rounds.id"), nullable=False)
    from_role: Mapped[str] = mapped_column(String(100), nullable=False)
    message_id: Mapped[int] = mapped_column(ForeignKey("web_messages.id"), nullable=False)
    submitted_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )
