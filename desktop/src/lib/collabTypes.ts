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
  settings: {
    heartbeat_timeout_seconds: number;
    message_retention_days: number;
    workflow_type?: string;
    workflow_colors?: Record<string, string>;
    auto_collab?: boolean;
    human_in_loop?: boolean;
  };
}

export interface SessionBinding {
  role: string;
  instance: number;
  session_id: string;
  claimed_at: string;
  last_heartbeat: string;
  status: "active" | "idle" | "gone" | "stale";
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
