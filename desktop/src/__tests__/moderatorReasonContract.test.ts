/**
 * Tests for the moderator-reason contract shared between CollabTab.tsx and
 * the Rust backend (`normalize_action_reason` in main.rs).
 *
 * Soft contract: backend never rejects on reason. The threshold is "use
 * caller value when ≥3 chars after trim, else fall back to the Rust-side
 * action default." This file locks the threshold constant on the TS side;
 * the Rust unit tests in main.rs lock the same number on the backend side.
 *
 * History:
 *   - pr-reason-params (0f0911c): hard rejection at <3 chars (initial design)
 *   - pr-reason-relax (this PR): soft rejection — backend defaults rather
 *     than errors out. Triggered by human msg 462 reporting friction on the
 *     End button.
 */
import { describe, it, expect } from "vitest";
import { MODERATOR_REASON_MIN_CHARS } from "../components/CollabTab";

describe("moderator-reason contract — threshold matches backend", () => {
  it("MODERATOR_REASON_MIN_CHARS matches the Rust normalize_action_reason threshold", () => {
    // If this changes, normalize_action_reason in main.rs must change in
    // lockstep — both sides agree the threshold is 3 trimmed characters
    // for "informative enough to override the default."
    expect(MODERATOR_REASON_MIN_CHARS).toBe(3);
  });

  it("threshold is a positive integer", () => {
    // Sanity guard against accidental string/float assignment.
    expect(Number.isInteger(MODERATOR_REASON_MIN_CHARS)).toBe(true);
    expect(MODERATOR_REASON_MIN_CHARS).toBeGreaterThan(0);
  });
});
