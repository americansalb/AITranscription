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
    /** Preferred field name post-pr-r2-data-fields (0477758 backend).
     *  Parsers should prefer `session_mode` when present, falling back to
     *  `discussion_mode` for projects last written before the rename. */
    session_mode?: string;
    /** Legacy: deprecated alias for `session_mode`. Backend serde aliases the
     *  field on read, so both forms still resolve. New writes emit
     *  `session_mode` only. Kept in the type so projects last written
     *  pre-rename still typecheck. */
    discussion_mode?: string;
    work_mode?: string;
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
  };
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
export type DiscussionMode = "open" | "directed" | "delphi" | "oxford" | "continuous" | "pipeline" | "red_team";

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

// ==================== Termination & Automation Types ====================

/** How a discussion determines when to end */
export type TerminationStrategy =
  | { type: "fixed_rounds"; rounds: number }    // Stop after exactly N rounds
  | { type: "consensus"; threshold: number }     // Stop when convergence >= threshold (0.0-1.0)
  | { type: "moderator_call" }                   // Moderator decides when to close
  | { type: "time_bound"; minutes: number }      // Stop after N minutes
  | { type: "unlimited" };                       // Run until human/moderator explicitly closes

/** How much autonomy the moderator has */
export type AutomationLevel = "manual" | "semi" | "auto";

/** Audience gate — what the audience can do at this moment */
export type AudienceGate = "listening" | "voting" | "qa" | "commenting" | "open";

/** Audience configuration for a discussion */
export interface AudienceConfig {
  enabled: boolean;
  pool: string | null;           // pool name/ID
  size: number;                  // number of personas
  gate: AudienceGate;            // current permission level
}

/** Discussion settings — extended with termination and automation */
export interface DiscussionSettings {
  // New fields (Phase 1)
  termination?: TerminationStrategy;   // Optional for backward compat — defaults to FixedRounds(max_rounds)
  automation?: AutomationLevel;        // Optional for backward compat — defaults to "auto"
  audience?: AudienceConfig;           // Optional for backward compat — defaults to disabled
  // Legacy fields (still read for backward compat)
  max_rounds: number;
  timeout_minutes: number;
  expire_paused_after_minutes: number;
  auto_close_timeout_seconds?: number;  // Continuous mode: seconds before silence = consent (default 60)
}

// ==================== Format-Specific State Types ====================

/** Pipeline stage output */
export interface PipelineOutput {
  stage: number;
  agent: string;
  message_id: number;
  timestamp: string;
}

/** Pipeline-specific state */
export interface PipelineState {
  order: string[];             // Ordered list of role:instance IDs
  current_stage: number;       // 0-based index into order
  outputs: PipelineOutput[];   // Accumulated outputs from completed stages
}

/** Oxford debate teams */
export interface OxfordTeams {
  proposition: string[];       // role:instance IDs for the proposition team
  opposition: string[];        // role:instance IDs for the opposition team
}

/** Oxford debate vote tally */
export interface OxfordVotes {
  for_count: number;
  against_count: number;
  abstain_count: number;
}

/** Red team attack-defense pair */
export interface AttackDefensePair {
  attack_message_id: number;
  defense_message_id: number | null;
  severity: "critical" | "high" | "medium" | "low";
  status: "unaddressed" | "partially_addressed" | "addressed";
}

/** Continuous mode micro-round */
export interface MicroRound {
  id: string;
  trigger_message_id: number;
  trigger_from: string;
  topic: string;
  opened_at: string;
  closed_at: string | null;
  timeout_seconds: number;
  responses: Array<{
    from: string;
    vote: "agree" | "disagree" | "alternative";
    message_id: number;
  }>;
  result: "consent" | "rejected" | "alternative" | "pending";
}

/** Decision stream entry (resolved micro-round summary) */
export interface Decision {
  micro_round_id: string;
  topic: string;
  result: "consent" | "rejected" | "alternative";
  resolved_at: string;
  summary: string;
}

/** Discussion phase — shared across formats for now, will become per-format enums in Phase 2 */
export type DiscussionPhase =
  | "submitting" | "aggregating" | "reviewing"   // Delphi phases
  | "paused" | "complete"                         // Shared phases
  | "pipeline_active" | "pipeline_complete"       // Pipeline phases
  | "oxford_opening" | "oxford_rebuttal" | "oxford_closing" | "oxford_vote" | "oxford_declaration"  // Oxford phases
  | "red_team_attacking" | "red_team_defending" | "red_team_assessment"  // Red team phases
  | "continuous_active"                           // Continuous phase
  | null;

/** Active discussion state — stored in .vaak/discussion.json
 *
 * MIGRATION NOTE: This interface currently uses a flat structure with optional
 * mode-specific fields for backward compatibility. The target architecture is a
 * discriminated union (see DiscussionStateV2 below). During migration, both
 * formats are supported — new code should use the type guards (isPipelineDiscussion,
 * isDelphiDiscussion, etc.) to narrow the type safely.
 */
export interface DiscussionState {
  active: boolean;
  mode: DiscussionMode | null;
  topic: string;
  started_at: string | null;     // ISO 8601
  moderator: string | null;      // role:instance
  participants: string[];        // role:instance IDs
  current_round: number;
  phase: DiscussionPhase;
  paused_at: string | null;
  expire_at: string | null;
  previous_phase: string | null;
  rounds: DiscussionRound[];
  settings: DiscussionSettings;
  // Pipeline mode fields (only present when mode === "pipeline")
  pipeline_mode?: "discussion" | "action";
  pipeline_order?: string[];
  pipeline_stage?: number;
  pipeline_outputs?: PipelineOutput[];
  // Oxford mode fields (only present when mode === "oxford")
  oxford_teams?: OxfordTeams;
  oxford_votes?: OxfordVotes;
  oxford_motion?: string;
  // Red team mode fields (only present when mode === "red_team")
  attack_chains?: AttackDefensePair[];
  severity_summary?: Record<string, number>;
  unaddressed_count?: number;
  // Continuous mode fields (only present when mode === "continuous")
  micro_rounds?: MicroRound[];
  decision_stream?: Decision[];
  // Audience fields
  audience_state?: AudienceGate;
  audience_enabled?: boolean;
  // Stagnation detection
  stagnant_rounds?: number;
}

// ==================== Discussion Type Guards ====================

/** Narrow DiscussionState to pipeline mode */
export function isPipelineDiscussion(d: DiscussionState): d is DiscussionState & {
  mode: "pipeline";
  pipeline_order: string[];
  pipeline_stage: number;
  pipeline_outputs: PipelineOutput[];
} {
  return d.mode === "pipeline" && Array.isArray(d.pipeline_order);
}

/** Narrow DiscussionState to Delphi mode */
export function isDelphiDiscussion(d: DiscussionState): d is DiscussionState & {
  mode: "delphi";
} {
  return d.mode === "delphi";
}

/** Narrow DiscussionState to Oxford debate mode */
export function isOxfordDiscussion(d: DiscussionState): d is DiscussionState & {
  mode: "oxford";
  oxford_teams: OxfordTeams;
  oxford_votes: OxfordVotes;
  oxford_motion: string;
} {
  return d.mode === "oxford" && d.oxford_teams != null;
}

/** Narrow DiscussionState to Red Team mode */
export function isRedTeamDiscussion(d: DiscussionState): d is DiscussionState & {
  mode: "red_team";
  attack_chains: AttackDefensePair[];
} {
  return d.mode === "red_team" && Array.isArray(d.attack_chains);
}

/** Narrow DiscussionState to Continuous mode */
export function isContinuousDiscussion(d: DiscussionState): d is DiscussionState & {
  mode: "continuous";
  micro_rounds: MicroRound[];
  decision_stream: Decision[];
} {
  return d.mode === "continuous" && Array.isArray(d.micro_rounds);
}

/** Get the effective termination strategy, falling back to legacy max_rounds */
export function getTerminationStrategy(settings: DiscussionSettings): TerminationStrategy {
  if (settings.termination) return settings.termination;
  return { type: "fixed_rounds", rounds: settings.max_rounds };
}

/** Get the effective automation level, defaulting to "auto" */
export function getAutomationLevel(settings: DiscussionSettings): AutomationLevel {
  return settings.automation ?? "auto";
}

/** Get the effective audience config, defaulting to disabled */
export function getAudienceConfig(settings: DiscussionSettings): AudienceConfig {
  return settings.audience ?? { enabled: false, pool: null, size: 0, gate: "listening" };
}

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

// ==================== Moderator Error Types ====================
// Why: architect msg 352 directs the Rust-enum owner to own the TS mirror.
// Rust source-of-truth lives at `desktop/src-tauri/src/bin/vaak-mcp.rs`:
//   - format_capability_error()    → CAPABILITY_NOT_SUPPORTED_FOR_FORMAT
//   - human-yields-to-moderator    → HUMAN_BYPASS_YIELDS_TO_MODERATOR
// Errors are emitted as strings with a `[error_code: X]` prefix so callers can
// pattern-match without JSON-deserializing the whole error payload. When Rust
// adds a new variant, update the ModeratorErrorCode union below in the same PR.

/** Closed enum of moderator-action error codes emitted by `discussion_control`. */
export type ModeratorErrorCode =
  | "CAPABILITY_NOT_SUPPORTED_FOR_FORMAT"
  | "HUMAN_BYPASS_YIELDS_TO_MODERATOR";

/** Parsed shape of a tagged moderator error string. */
export interface ModeratorError {
  /** Discriminator — matches the `[error_code: X]` prefix. */
  code: ModeratorErrorCode;
  /** Full original error string from the Rust side (for logging/debugging). */
  rawMessage: string;
  /** Capability name, populated for CAPABILITY_NOT_SUPPORTED_FOR_FORMAT. */
  capability?: string;
  /** Session format name, populated for CAPABILITY_NOT_SUPPORTED_FOR_FORMAT. */
  format?: string;
  /** Moderator role:instance label, populated for HUMAN_BYPASS_YIELDS_TO_MODERATOR. */
  moderator?: string;
}

/**
 * Parse a Rust-side error string into a structured ModeratorError.
 * Returns null if the string has no `[error_code: X]` prefix — i.e. it's a
 * generic/unstructured error and should be rendered as plain text.
 *
 * Kept minimal on purpose: regex-extract the code, then extract fields by
 * their named keys. Adding a new variant means adding a case + a field.
 */
export function parseModeratorError(rawMessage: string): ModeratorError | null {
  const codeMatch = rawMessage.match(/^\[error_code: (\w+)\]/);
  if (!codeMatch) return null;
  const code = codeMatch[1] as ModeratorErrorCode;

  if (code === "CAPABILITY_NOT_SUPPORTED_FOR_FORMAT") {
    const cap = rawMessage.match(/capability='([^']+)'/)?.[1];
    const fmt = rawMessage.match(/format='([^']+)'/)?.[1];
    return { code, rawMessage, capability: cap, format: fmt };
  }
  if (code === "HUMAN_BYPASS_YIELDS_TO_MODERATOR") {
    const mod = rawMessage.match(/moderator='([^']+)'/)?.[1];
    return { code, rawMessage, moderator: mod };
  }
  // Unknown code — still return a parsed shell so callers can log it
  return { code, rawMessage };
}
