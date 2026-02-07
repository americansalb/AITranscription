export interface RoleConfig {
  title: string;
  description: string;
  max_instances: number;
  permissions: string[];
  created_at: string;
}

export interface ProjectConfig {
  project_id: string;
  name: string;
  description: string;
  created_at: string;
  updated_at: string;
  roles: Record<string, RoleConfig>;
  roster?: RosterSlot[];
  settings: {
    heartbeat_timeout_seconds: number;
    message_retention_days: number;
    workflow_type?: string;
    workflow_colors?: Record<string, string>;
    auto_collab?: boolean;
    human_in_loop?: boolean;
    discussion_mode?: string;
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

/** Preset team templates for quick setup */
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
