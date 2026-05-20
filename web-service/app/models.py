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
    projects: Mapped[list["Project"]] = relationship(
        back_populates="owner", lazy="selectin", cascade="all, delete-orphan"
    )


class ProjectMode(str, enum.Enum):
    """Vaaklite v1: project intent.

    * `coding` — original AI coding-collaboration projects (default for
      backwards compatibility with existing rows).
    * `discussion` — Vaaklite discussion + document drafting projects.
      Per human msg 5730: roles + assembly-mode-style rotation + role
      creation sessions + user-account-scoped session persistence, with
      a clean intuitive UI focused on document creation rather than
      code editing.
    """

    CODING = "coding"
    DISCUSSION = "discussion"


class Project(Base):
    """A collaboration project owned by a user."""

    __tablename__ = "web_projects"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    name: Mapped[str] = mapped_column(String(100), nullable=False)
    owner_id: Mapped[int] = mapped_column(ForeignKey("web_users.id"), nullable=False)
    is_active: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    # Vaaklite v1 (per architect msg 5738 spec lock): mode discriminator.
    # Existing rows default to `coding` so no migration of legacy state is
    # required. Discussion mode unlocks document drafting + section-rotation.
    mode: Mapped[ProjectMode] = mapped_column(
        Enum(ProjectMode), default=ProjectMode.CODING, nullable=False, server_default="coding"
    )
    # Vaaklite v1: optional template slug applied at create-time to
    # auto-seed the role roster (e.g., `simple-rotation`, `delphi-debate`,
    # `oxford-review`). Coding-mode projects leave it null.
    template: Mapped[str | None] = mapped_column(String(64), nullable=True)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )

    # Relationships (cascade delete: removing a project removes its roles, messages, etc.)
    owner: Mapped["WebUser"] = relationship(back_populates="projects")
    roles: Mapped[list["ProjectRole"]] = relationship(
        back_populates="project", lazy="selectin", cascade="all, delete-orphan"
    )
    messages: Mapped[list["Message"]] = relationship(
        back_populates="project", lazy="noload", cascade="all, delete-orphan"
    )
    documents: Mapped[list["Document"]] = relationship(
        back_populates="project", lazy="selectin", cascade="all, delete-orphan"
    )


class ProjectRole(Base):
    """A role configuration within a project (e.g., developer, architect)."""

    __tablename__ = "web_project_roles"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    project_id: Mapped[int] = mapped_column(ForeignKey("web_projects.id"), nullable=False)
    slug: Mapped[str] = mapped_column(String(50), nullable=False)
    title: Mapped[str] = mapped_column(String(100), nullable=False)
    briefing: Mapped[str] = mapped_column(Text, default="", nullable=False)

    # Role metadata (stored as JSON arrays)
    tags: Mapped[list] = mapped_column(JSON, default=list, nullable=False)
    permissions: Mapped[list] = mapped_column(JSON, default=list, nullable=False)

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
        back_populates="discussion", lazy="selectin", order_by="DiscussionRound.number",
        cascade="all, delete-orphan",
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
        back_populates="round", lazy="selectin", cascade="all, delete-orphan",
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

    round: Mapped["DiscussionRound"] = relationship(back_populates="submissions")


# ==================== Vaaklite v1: discussion-mode document drafting ====================
# Schema additions per architect msg 5738 spec lock (human msg 5730 directive).
#
# Vaaklite turns a Project (mode=discussion) into a section-rotated document
# drafting session: agents take turns drafting sections of a markdown
# document, with phases (drafting → review → revision → final). The schema
# mirrors the existing Discussion / DiscussionRound / DiscussionSubmission
# triad but is document-centric rather than message-centric — output is the
# rendered markdown doc, not a structured-question tally.


class DocumentPhase(str, enum.Enum):
    """Lifecycle of a Vaaklite drafting document.

    Mirrors the floor.phase progression in the desktop app but document-
    scoped: the document moves through drafting (sections being authored
    one-by-one), review (peers comment), revision (assigned roles refine
    flagged sections), and final (locked, downloadable).
    """

    DRAFTING = "drafting"
    REVIEW = "review"
    REVISION = "revision"
    FINAL = "final"


class DocumentSectionStatus(str, enum.Enum):
    """Per-section status inside a Vaaklite document.

    Tracks which sections have been authored, are queued for the assigned
    role's drafting turn, are awaiting peer review, or have been accepted
    into the final draft.
    """

    PENDING = "pending"
    DRAFTING = "drafting"
    REVIEW_PENDING = "review_pending"
    ACCEPTED = "accepted"


class Document(Base):
    """A Vaaklite document being drafted by the team.

    One Project (mode=discussion) can host multiple Documents. Each Document
    has an ordered list of DocumentSections; the `current_section_idx` field
    is the "mic" — the section currently being drafted by `current_role`.
    Markdown is materialized in `rendered_markdown` after sections accept
    so the human + agents can read the full doc at any time.
    """

    __tablename__ = "web_documents"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    project_id: Mapped[int] = mapped_column(
        ForeignKey("web_projects.id"), nullable=False, index=True
    )
    title: Mapped[str] = mapped_column(String(200), nullable=False)
    topic: Mapped[str] = mapped_column(Text, default="", nullable=False)
    phase: Mapped[DocumentPhase] = mapped_column(
        Enum(DocumentPhase), default=DocumentPhase.DRAFTING, nullable=False
    )
    # The current "mic" — index into DocumentSection.idx of the section
    # currently being drafted. NULL when no section is active (e.g., between
    # phases or after FINAL).
    current_section_idx: Mapped[int | None] = mapped_column(Integer, nullable=True)
    current_role: Mapped[str | None] = mapped_column(String(100), nullable=True)
    # Rendered markdown — concatenated accepted sections + drafting-in-progress
    # preview. Maintained by the section-accept path so reads are fast.
    rendered_markdown: Mapped[str] = mapped_column(Text, default="", nullable=False)
    # Final downloadable artifact — set when phase transitions to FINAL.
    final_markdown: Mapped[str | None] = mapped_column(Text, nullable=True)
    finalized_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )

    project: Mapped["Project"] = relationship(back_populates="documents")
    sections: Mapped[list["DocumentSection"]] = relationship(
        back_populates="document",
        lazy="selectin",
        order_by="DocumentSection.idx",
        cascade="all, delete-orphan",
    )
    turns: Mapped[list["DraftingTurn"]] = relationship(
        back_populates="document",
        lazy="noload",
        order_by="DraftingTurn.started_at",
        cascade="all, delete-orphan",
    )


class DocumentSection(Base):
    """A single section in a Vaaklite document.

    Sections are ordered by `idx`. `assigned_role` is the role slug expected
    to draft this section (rotation order). `status` flows pending →
    drafting → review_pending → accepted as the team works through the doc.
    """

    __tablename__ = "web_document_sections"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    document_id: Mapped[int] = mapped_column(
        ForeignKey("web_documents.id"), nullable=False, index=True
    )
    idx: Mapped[int] = mapped_column(Integer, nullable=False)
    title: Mapped[str] = mapped_column(String(200), nullable=False)
    # Role slug expected to draft this section. Filled at template apply.
    assigned_role: Mapped[str | None] = mapped_column(String(100), nullable=True)
    body: Mapped[str] = mapped_column(Text, default="", nullable=False)
    status: Mapped[DocumentSectionStatus] = mapped_column(
        Enum(DocumentSectionStatus),
        default=DocumentSectionStatus.PENDING,
        nullable=False,
    )
    # Optional reviewer-supplied notes attached when status flips to
    # review_pending. Cleared on accept.
    review_notes: Mapped[str | None] = mapped_column(Text, nullable=True)
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )

    document: Mapped["Document"] = relationship(back_populates="sections")

    __table_args__ = (
        UniqueConstraint("document_id", "idx", name="uq_document_section_idx"),
    )


class DraftingTurn(Base):
    """Audit log: one row per agent-drafts-a-section turn.

    Captures the rotation history so the UI can show "Section 3 was
    drafted by writer:0 from 14:02 to 14:09" and the team can see who
    contributed what. `output_diff` stores the body the role wrote
    during the turn (or a structured patch — v1 stores raw body).
    """

    __tablename__ = "web_drafting_turns"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    document_id: Mapped[int] = mapped_column(
        ForeignKey("web_documents.id"), nullable=False, index=True
    )
    section_idx: Mapped[int] = mapped_column(Integer, nullable=False)
    role_seat: Mapped[str] = mapped_column(String(100), nullable=False)
    output_body: Mapped[str] = mapped_column(Text, default="", nullable=False)
    started_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=_utcnow, nullable=False
    )
    completed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)

    document: Mapped["Document"] = relationship(back_populates="turns")
