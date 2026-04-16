/**
 * Tests for collabTypes — ModeratorError parsing + drift guards.
 *
 * Covers:
 *   - parseModeratorError happy path for each ModeratorErrorCode variant
 *   - parseModeratorError returns null for untagged errors
 *   - parseModeratorError handles unknown codes without crashing
 *   - Drift guard placeholder: toast-renders-parsed-code-not-raw
 *     (tech-leader msg 383 directive; skipped until PR H3 v2)
 *
 * Provenance: pr-t7 — tech-leader msg 383 addition to tester msg 371 matrix.
 */
import { describe, it, expect } from "vitest";
import {
  parseModeratorError,
  type ModeratorError,
  type ModeratorErrorCode,
} from "../lib/collabTypes";

describe("parseModeratorError — active unit tests", () => {
  it("returns null when the message has no error_code prefix", () => {
    expect(parseModeratorError("plain error without prefix")).toBeNull();
    expect(parseModeratorError("")).toBeNull();
    expect(parseModeratorError("   ")).toBeNull();
  });

  it("parses CAPABILITY_NOT_SUPPORTED_FOR_FORMAT with capability + format", () => {
    const raw =
      "[error_code: CAPABILITY_NOT_SUPPORTED_FOR_FORMAT] capability='reorder_pipeline' format='delphi': reorder is pipeline-only";
    const parsed = parseModeratorError(raw);
    expect(parsed).not.toBeNull();
    expect(parsed!.code).toBe("CAPABILITY_NOT_SUPPORTED_FOR_FORMAT");
    expect(parsed!.capability).toBe("reorder_pipeline");
    expect(parsed!.format).toBe("delphi");
    expect(parsed!.rawMessage).toBe(raw);
    // moderator field should be undefined for this variant
    expect(parsed!.moderator).toBeUndefined();
  });

  it("parses HUMAN_BYPASS_YIELDS_TO_MODERATOR with moderator", () => {
    const raw =
      "[error_code: HUMAN_BYPASS_YIELDS_TO_MODERATOR] moderator='moderator:0': route through the claimed moderator";
    const parsed = parseModeratorError(raw);
    expect(parsed).not.toBeNull();
    expect(parsed!.code).toBe("HUMAN_BYPASS_YIELDS_TO_MODERATOR");
    expect(parsed!.moderator).toBe("moderator:0");
    expect(parsed!.rawMessage).toBe(raw);
    // capability/format should be undefined for this variant
    expect(parsed!.capability).toBeUndefined();
    expect(parsed!.format).toBeUndefined();
  });

  it("returns a parsed shell for unknown codes instead of crashing", () => {
    // Safety: when Rust adds a new code ahead of the TS mirror, parse must
    // still return *something* so callers can log the raw message. This is
    // the fallback that makes the drift-guard test (below) load-bearing.
    const raw = "[error_code: UNKNOWN_FUTURE_CODE] some payload";
    const parsed = parseModeratorError(raw);
    expect(parsed).not.toBeNull();
    expect(parsed!.code).toBe("UNKNOWN_FUTURE_CODE" as ModeratorErrorCode);
    expect(parsed!.rawMessage).toBe(raw);
  });
});

describe("drift-guard placeholders (tech-leader msg 383)", () => {
  // Skipped: requires mounting CollabTab with a mock handleEndDiscussion that
  // throws the tagged error string. PR H3 v2 will wire the parseModeratorError
  // call into a toast element with role="alert" + aria-live="assertive".
  // Unskip when PR H3 v2 ships.
  it.skip(
    "test_moderator_error_toast_renders_parsed_code_not_raw",
    async () => {
      // Assertion shape once PR H3 v2 ships:
      //   1. mount <CollabTab ... handleEndDiscussion={failingHandler} />
      //     where failingHandler throws
      //       "[error_code: CAPABILITY_NOT_SUPPORTED_FOR_FORMAT] capability='reorder' format='delphi'"
      //   2. fire user event to trigger the handler
      //   3. await screen.findByRole("alert")
      //   4. expect that alert to contain /reorder.*not available.*delphi/i
      //      — NOT the raw '[error_code: ...]' string
      //   5. expect alert has aria-live="assertive" per platform msg 175
      throw new Error("unimplemented — gated on PR H3 v2");
    },
  );

  // Skipped mirror of the Rust cargo test at vaak-mcp.rs
  // (test_ts_types_match_rust_enum_variants). Both assert the same drift:
  // adding a ModeratorErrorCode variant here without a matching Rust enum
  // variant should fail CI. This TS-side version catches it faster since
  // the parse function lives here.
  it.skip(
    "ModeratorErrorCode union matches Rust enum variants",
    () => {
      // Assertion shape once Rust promotes the string-prefixed errors to a
      // real enum:
      //   1. read collab.rs or wherever ModeratorError: enum { ... } lives
      //   2. extract variant names (SCREAMING_SNAKE or camelCase, per naming
      //      convention chosen at promotion time)
      //   3. compare with the `ModeratorErrorCode` union literal set from
      //      this file (import or parse)
      //   4. assert set equality; fail with symmetric diff
      throw new Error(
        "unimplemented — gated on Rust enum promotion of error codes",
      );
    },
  );
});
