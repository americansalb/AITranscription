// Collaborate v2 shared types — P1 scope.
// Keep in sync with .vaak/v2/seats.json schema per §B.2.1.

export type SeatState = "active" | "standby" | "disconnected";

export interface Seat {
  role: string;
  instance: number;
  agent_id: string | null;
  transport_bound: boolean;
  state: SeatState;
  last_heartbeat_ms?: number;
  first_joined_at_ms?: number;
}

export interface SeatsFile {
  schema_version: number;
  seats: Seat[];
}

export type Flow = "assembly_line" | "round_robin" | "oxford" | "delphi" | "open";

export interface SectionState {
  schema_version: number;
  section: string;
  flow: Flow;
  current_speaker: { role: string; instance: number } | null;
  work_allowed: boolean;
  moderator: { role: string; instance: number; participates: boolean } | null;
}
