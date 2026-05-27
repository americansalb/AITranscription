export interface CompanionConfig {
  role: string;
  optional: boolean;
  default_enabled: boolean;
}

export interface RoleConfig {
  title: string;
  description: string;
  max_instances: number;
  permissions: string[];
  created_at: string;
  tags?: string[];
  companions?: CompanionConfig[];
  custom?: boolean;
  /** Optional HTTPS URL to a hand-illustrated avatar overriding the procedural
   * default. Shipped in project.json since 139a389; runtime-read by CollabTab
   * (roster role cards) + RolesTab (config cards). Per character-avatar-system-
   * spec-2026-05-17.md v6.9 §3.1 + §4 + privacy forward-flag in §4.1 (v6.10
   * pending). */
  avatar_url?: string;
  /** Optional 6-axis character stats (TD/AR/CP/DO/PD/JA, each 1-10). Per
   * character-stats-system-2026-05-16.md + cca848e Phase 1 schema; rendered
   * by Phase 2.E stats radar in RoleBriefingModal expanded surface. */
  stats?: { td: number; ar: number; cp: number; do: number; pd: number; ja: number };
}

export interface ProjectConfig {
  project_id: string;
  name: string;
  description: string;
  created_at: string;
  updated_at: string;
  roles: Record<string, RoleConfig>;
  roster?: RosterSlot[];
  role_groups?: RoleGroup[];
  settings: {
    heartbeat_timeout_seconds: number;
    message_retention_days: number;
    workflow_type?: string;
    workflow_colors?: Record<string, string>;
    auto_collab?: boolean;
    human_in_loop?: boolean;
    discussion_mode?: string;
    currency_enabled?: boolean;
  };
}

export interface SessionBinding {
  role: string;
  instance: number;
  session_id: string;
  claimed_at: string;
  last_heartbeat: string;
  status: "active" | "idle" | "gone" | "stale";
  activity?: "working" | "standby" | "idle" | "disconnected";
  last_working_at?: string;
  active_section?: string;
}

export interface QuestionChoice {
  id: string;
  label: string;
  desc?: string;
  /** Decision-panel v1 flag #6: mark the option the asker recommends. */
  recommended?: boolean;
}

export interface BoardMessage {
  id: number;
  from: string;
  to: string;
  type: string;
  timestamp: string;
  subject: string;
  body: string;
  metadata: Record<string, unknown> & {
    choices?: QuestionChoice[];
    question_id?: string;
    in_reply_to?: number;
    choice_id?: string;
    vote_type?: string;
    proposed_value?: string;
    vote?: string;
    /** Decision-panel v1 flag #3: allow free-form "Other" answer that fires as a directive. */
    allow_other?: boolean;
    /** Decision-panel v1 flag #2: agent-side hash for dedup; UI computes fallback if absent. */
    question_hash?: string;
  };
}

/** Decision-panel v1 — resolution log entry persisted in .vaak/decisions.jsonl. */
export interface DecisionResolution {
  decision_id: number;
  /** "resolve" | "cancel" */
  kind: string;
  option_id?: string;
  other_text?: string;
  /** For cancel: "author_cancel" | "stale_archive" | "board_resolved" */
  reason?: string;
  at: string;
  by: string;
}

export interface RoleStatus {
  slug: string;
  title: string;
  active_instances: number;
  max_instances: number;
  status: "active" | "idle" | "gone" | "stale" | "vacant";
}

export interface FileClaim {
  role_instance: string;
  files: string[];
  description: string;
  claimed_at: string;
  session_id: string;
  /** Active-claims v1 (architect msg 5044 + ui-arch:1 msg 5048): per-claim
   * liveness derived from `.vaak/sessions/<role>-<inst>.json:last_alive_at_ms`.
   * "active" | "stale" | "unknown". Optional — undefined means pre-keepalive
   * Tauri binary or seat-file missing. */
  alive_state?: "active" | "stale" | "unknown";
}

export interface ParsedProject {
  config: ProjectConfig;
  sessions: SessionBinding[];
  messages: BoardMessage[];
  role_statuses: RoleStatus[];
  claims: FileClaim[];
}

export interface SessionInfo {
  session_id: string;
  last_heartbeat: number;
  hostname: string;
  cwd: string;
  name: string;
}

// ==================== Section Types ====================

export interface Section {
  slug: string;
  name: string;
  created_at: string;
  message_count: number;
  last_activity: string | null;
  is_active: boolean;
}

// ==================== Discussion Mode Types ====================

/** Valid discussion modes */
export type DiscussionMode = "open" | "directed" | "delphi" | "oxford" | "continuous";

/** Valid message types including the new "submission" type */
export type MessageType =
  | "directive"
  | "question"
  | "answer"
  | "status"
  | "handoff"
  | "review"
  | "approval"
  | "revision"
  | "broadcast"
  | "submission"
  | "moderation";

/** A single submission within a Delphi round */
export interface DiscussionSubmission {
  from: string;           // Real role:instance ID (anonymized at display time)
  message_id: number;     // Reference to the message in board.jsonl
  submitted_at: string;   // ISO 8601 timestamp
}

/** A single round within a discussion */
export interface DiscussionRound {
  number: number;
  opened_at: string;             // ISO 8601
  closed_at: string | null;      // null if still open
  submissions: DiscussionSubmission[];
  aggregate_message_id: number | null;  // Message ID of the aggregate post
  auto_triggered?: boolean;      // true for continuous-mode micro-rounds
  topic?: string;                // Per-round topic (continuous mode: the status message that triggered it)
}

/** Discussion settings */
export interface DiscussionSettings {
  max_rounds: number;
  timeout_minutes: number;
  expire_paused_after_minutes: number;
  auto_close_timeout_seconds?: number;  // Continuous mode: seconds before silence = consent (default 60)
}

/** Active discussion state — stored in .vaak/discussion.json */
export interface DiscussionState {
  active: boolean;
  mode: DiscussionMode | null;
  topic: string;
  started_at: string | null;     // ISO 8601
  moderator: string | null;      // role:instance
  participants: string[];        // role:instance IDs
  current_round: number;
  phase: "submitting" | "aggregating" | "reviewing" | "paused" | "complete" | null;
  paused_at: string | null;
  expire_at: string | null;
  previous_phase: string | null;
  rounds: DiscussionRound[];
  settings: DiscussionSettings;
}

// ==================== Phase D — Delphi Discussion (spec v2 LOCKED 2026-05-27) ====================
//
// Serde-parity twins of `desktop/src-tauri/src/collab.rs::pub mod delphi`.
// Per spec §5.6: every field declared here MUST appear in the corresponding
// Rust struct AND the JSON schema on disk. Mismatch → silent strip on read
// → fields become undefined with no compile error. This burned the currency
// toggle (c9d4825) and multiple Oxford fields. PR-review acceptance: every
// PR that adds a Delphi field must show the TS interface + Rust struct
// diff side-by-side.
//
// State file: .vaak/active-delphi-debate.json
// Event log:  .vaak/delphi-discussions.jsonl
// Archive:    .vaak/delphi-completed/<discussion_id>.json (on end)

/** Spec §2 phase machine state. Variants are snake_case in JSON. */
export type DelphiPhase =
  | "none"
  | "setup"
  | "opening"
  | "submitting"
  | "aggregating"
  | "reviewing"
  | "ended";

/** Spec §3.1 convergence-end semantics. */
export type DelphiConvergenceMode = "moderator" | "max_rounds" | "hybrid";

/** Spec §3.10 outcome values. */
export type DelphiOutcome =
  | "converged"
  | "max_rounds_reached"
  | "abandoned"
  | "aborted_quorum_loss"
  | "human_override"
  | "oxford_preemption";

/**
 * Spec §4.2 — a single submission within a Delphi round. `anonymous_id` is
 * set post-shuffle. `from` and `submitted_at` are moderator-visible only;
 * participant/audience views strip these fields per spec §5 privacy.
 */
export interface DelphiSubmission {
  from: string;
  anonymous_id?: string | null;
  content: string;
  /** sha256:<hex> of `content`. */
  content_hash: string;
  /** Earlier-revision hashes (oldest first). Empty on first submit. */
  revision_hash_chain: string[];
  submitted_at: string;
}

/** Spec §3.7 — audience-question queued during `reviewing`. */
export interface DelphiAudienceQuestion {
  asker: string;
  question: string;
  posted_at: string;
}

/** Spec §4.2 — a single round within a Delphi discussion. */
export interface DelphiRound {
  number: number;
  opened_at: string;
  closed_at: string | null;
  prompt: string;
  submissions: DelphiSubmission[];
  /**
   * `{anonymous_id → real_seat}`. Empty until aggregate runs. After debate
   * ends, public via the archived completed-file per spec §5.
   */
  unshuffle_map: Record<string, string>;
  /** Hex-encoded cryptographic seed. Generated at round-open. */
  unshuffle_seed: string;
  aggregate_message_id: number | null;
  /** Participants who did not submit by close. */
  non_submitters: string[];
  audience_questions: DelphiAudienceQuestion[];
}

/**
 * Spec §4.2 — snapshot of the currently-active Delphi. Atomically written
 * after each state-changing MCP tool call. Archived (not deleted) on end.
 */
export interface ActiveDelphiDebate {
  discussion_id: number;
  moderator: string;
  participants: string[];
  audience: string[];
  topic: string;
  max_rounds: number;
  convergence_criterion: DelphiConvergenceMode;
  convergence_reward_copper: number;
  phase: DelphiPhase;
  current_round: number;
  phase_started_at: string | null;
  blind_gate_active: boolean;
  /**
   * LOCKED v2 §6.3 — default false (normal mode); true disables the
   * DM-to-moderator carve-out for stricter blind protocols.
   */
  blind_gate_strict: boolean;
  submission_soft_floor_secs: number;
  submission_hard_floor_secs: number;
  review_floor_secs: number;
  started_at: string;
  rounds: DelphiRound[];
}

/**
 * Spec §4.1 — lifecycle event in delphi-discussions.jsonl. Tagged union by
 * `event` discriminant. Mirrors Rust `DelphiEvent` enum's serde
 * (tag = "event", rename_all = "snake_case").
 */
export type DelphiEvent =
  | {
      event: "initiate";
      discussion_id: number;
      timestamp: string;
      moderator: string;
      participants: string[];
      audience: string[];
      topic: string;
      max_rounds: number;
      convergence_criterion: DelphiConvergenceMode;
      convergence_reward_copper: number;
      submission_soft_floor_secs: number;
      submission_hard_floor_secs: number;
      review_floor_secs: number;
      blind_gate_strict: boolean;
    }
  | {
      event: "round_opened";
      discussion_id: number;
      round: number;
      prompt: string;
      timestamp: string;
    }
  | {
      event: "submission";
      discussion_id: number;
      round: number;
      seat: string;
      content_hash: string;
      revision_number: number;
      timestamp: string;
    }
  | {
      event: "round_closed";
      discussion_id: number;
      round: number;
      aggregate_message_id: number;
      submissions_count: number;
      non_submitters: string[];
      unshuffle_seed: string;
      timestamp: string;
    }
  | {
      event: "audience_question";
      discussion_id: number;
      round: number;
      from: string;
      question: string;
      timestamp: string;
    }
  | {
      event: "react";
      discussion_id: number;
      caller: string;
      emoji: string;
      timestamp: string;
    }
  | {
      event: "kicked";
      discussion_id: number;
      seat: string;
      reason: string;
      timestamp: string;
    }
  | {
      event: "ended";
      discussion_id: number;
      outcome: DelphiOutcome;
      rounds_completed: number;
      convergence_reward_distributed_copper: number;
      reward_recipients: string[];
      timestamp: string;
    };

/** LOCKED constants — keep in sync with Rust `pub mod delphi` consts. */
export const DELPHI_DEFAULTS = {
  CONVERGENCE_REWARD_COPPER: 0,
  MAX_ROUNDS: 5,
  SUBMISSION_SOFT_FLOOR_SECS: 180,
  SUBMISSION_HARD_FLOOR_SECS: 360,
  REVIEW_FLOOR_SECS: 300,
  AUDIENCE_QUESTION_RATE_LIMIT_SECS: 60,
  AUDIENCE_QUESTION_QUEUE_CAP: 5,
  REACT_RATE_LIMIT_PER_MIN: 3,
  REACT_RATE_LIMIT_WINDOW_SECS: 60,
  MODERATOR_VACANCY_TIMEOUT_SECS: 300,
  MIN_PARTICIPANTS: 2,
} as const;

// ==================== Team Roster Types ====================

/** Computed status for a roster slot */
export type RosterSlotStatus = "vacant" | "standby" | "working";

/** A single slot in the project roster (stored in project.json > roster[]) */
export interface RosterSlot {
  role: string;
  instance: number;
  added_at: string;  // ISO 8601
}

/** A roster slot with computed status (returned by roster_get command) */
export interface RosterSlotWithStatus extends RosterSlot {
  status: RosterSlotStatus;
  session_id?: string;  // Present when staffed
  activity?: string;    // "working" | "standby" etc.
}

/** Response from roster_get Tauri command */
export interface RosterResponse {
  slots: RosterSlotWithStatus[];
  catalog: Record<string, RoleConfig>;  // Available role definitions
}

/** A role entry within a group/preset */
export interface RoleGroupEntry {
  slug: string;
  instances: number;
}

/** A reusable group of roles (preset or custom) */
export interface RoleGroup {
  slug: string;
  name: string;
  icon: string;
  description: string;
  builtin: boolean;
  roles: RoleGroupEntry[];
  parent?: string | null;  // parent group slug — null/undefined = top-level
  order?: number;          // sort position within parent
}

/** @deprecated Use RoleGroup instead */
export interface TeamTemplate {
  id: string;
  name: string;
  description: string;
  roles: { role: string; count: number }[];
}

// ==================== Team Launcher Types ====================

/** A Claude agent spawned from the Team Launcher UI */
export interface SpawnedAgent {
  pid: number;
  role: string;
  instance: number;
  spawned_at: string;  // ISO 8601
}
