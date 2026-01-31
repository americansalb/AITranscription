export interface CollabParticipant {
  role: string;
  session_id: string;
  joined_at: string;
  last_heartbeat: number;
}

export interface CollabMessage {
  number: number;
  role: string;
  timestamp: string;
  text: string;
}

export interface CollabState {
  collab_id: string;
  project_dir: string;
  participants: CollabParticipant[];
  message_count: number;
  messages: CollabMessage[];
  last_activity: string;
}

export interface CollabUpdate {
  collaborations: CollabState[];
}
