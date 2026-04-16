/**
 * Tests for the moderator-reason contract shared between CollabTab.tsx and
 * the Rust backend (validate_action_reason in main.rs).
 *
 * Locks the constants used as default reasons for end/pause/resume_discussion
 * invokes when no user-supplied reason is available (slash commands, toggle
 * button). Drift between these and the Rust validator's ≥3-char rule would
 * cause silent invoke failures in production.
 *
 * Provenance: pr-reason-params — tech-leader msg 423 directive #5
 * (TS test locking the new invoke signature contract).
 */
import { describe, it, expect } from "vitest";
import {
  MODERATOR_REASON_MIN_CHARS,
  DEFAULT_END_REASON,
  DEFAULT_PAUSE_REASON,
  DEFAULT_RESUME_REASON,
} from "../components/CollabTab";

describe("moderator-reason contract — default reasons satisfy backend validator", () => {
  it("MODERATOR_REASON_MIN_CHARS matches the Rust validator's ≥3-char rule", () => {
    // If this changes, validate_action_reason in main.rs must change in
    // lockstep — both sides agree the floor is 3 trimmed characters.
    expect(MODERATOR_REASON_MIN_CHARS).toBe(3);
  });

  it("DEFAULT_END_REASON satisfies the minimum length after trim", () => {
    expect(DEFAULT_END_REASON.trim().length).toBeGreaterThanOrEqual(MODERATOR_REASON_MIN_CHARS);
  });

  it("DEFAULT_PAUSE_REASON satisfies the minimum length after trim", () => {
    expect(DEFAULT_PAUSE_REASON.trim().length).toBeGreaterThanOrEqual(MODERATOR_REASON_MIN_CHARS);
  });

  it("DEFAULT_RESUME_REASON satisfies the minimum length after trim", () => {
    expect(DEFAULT_RESUME_REASON.trim().length).toBeGreaterThanOrEqual(MODERATOR_REASON_MIN_CHARS);
  });

  it("default reasons are distinguishable so audit can tell them apart", () => {
    const set = new Set([DEFAULT_END_REASON, DEFAULT_PAUSE_REASON, DEFAULT_RESUME_REASON]);
    expect(set.size).toBe(3);
  });
});
