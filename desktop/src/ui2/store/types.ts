// UI2 domain types. The engine contract (BoardMessage etc.) is imported from
// collabTypes — that module is the data contract, not the old surface.
import type { BoardMessage, ParsedProject } from "../../lib/collabTypes";

export type { BoardMessage, ParsedProject };

/** Treatment classes — one per rule of the IA decision table (R1–R7). */
export type Treatment =
  | { rule: "R1"; kind: "human" } // expanded, operator's own voice
  | { rule: "R2"; kind: "card" } // decision card: dock + inline row
  | { rule: "R3"; kind: "relay" } // expanded relay post
  | { rule: "R4"; kind: "verdict"; discussionKey: string } // verdict digest row
  | { rule: "R5"; kind: "lifecycle"; discussionKey: string } // fold into discussion row
  | { rule: "R6"; kind: "dm-violation" } // engine room only, flagged
  | { rule: "R7"; kind: "engine" }; // fold into time-burst digest row

export interface ClassifiedMessage {
  msg: BoardMessage;
  treatment: Treatment;
}

/** A row the Signal Feed actually renders. */
export type FeedRow =
  | { kind: "message"; key: string; msg: BoardMessage; voice: "human" | "relay" }
  | { kind: "card"; key: string; msg: BoardMessage }
  | {
      kind: "discussion";
      key: string;
      discussionKey: string;
      label: string;
      eventCount: number;
      verdict: BoardMessage | null;
      lastTimestamp: string;
      events: BoardMessage[];
    }
  | {
      kind: "burst";
      key: string;
      count: number;
      protocolViolations: number;
      firstTimestamp: string;
      lastTimestamp: string;
      events: BoardMessage[];
    };

/** Liveness — cognition and connection are distinct facts (token sheet §6 ann. 2). */
export type SeatLiveness = "working" | "warm-zombie" | "dead" | "vacant";

export interface SeatDot {
  role: string;
  instance: number;
  title: string;
  liveness: SeatLiveness;
  lastWorkingAt: string | null;
}

export interface DecisionCardState {
  msg: BoardMessage;
  status: "active" | "queued" | "resolved";
  blockedBy: number | null;
  resolvedChoice: string | null;
  resolvedAt: string | null;
}
