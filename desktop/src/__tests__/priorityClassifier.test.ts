/**
 * Tests for priorityClassifier — regex-based message priority classification.
 *
 * Covers:
 *   - Critical pattern detection (errors, failures, exceptions)
 *   - Low priority pattern detection (confirmations, status)
 *   - Normal priority fallback
 *   - Case sensitivity (CRITICAL vs critical)
 *   - Boundary cases (empty string, mixed patterns)
 *   - Priority toggle persistence
 */
import { describe, it, expect, beforeEach } from "vitest";
import {
  classifyPriority,
  getStoredPriorityEnabled,
  savePriorityEnabled,
} from "../lib/priorityClassifier";


// =============================================================================
// CRITICAL CLASSIFICATION
// =============================================================================

describe("classifyPriority — critical", () => {
  it("detects 'error' as critical", () => {
    expect(classifyPriority("There was an error in the build")).toBe("critical");
  });

  it("detects 'exception' as critical", () => {
    expect(classifyPriority("Unhandled exception in worker thread")).toBe("critical");
  });

  it("detects 'failed' as critical", () => {
    expect(classifyPriority("The deployment failed")).toBe("critical");
  });

  it("detects 'crash' as critical", () => {
    expect(classifyPriority("Application crash detected")).toBe("critical");
  });

  it("detects 'fatal' as critical", () => {
    expect(classifyPriority("Fatal: could not read from remote")).toBe("critical");
  });

  it("detects 'panic' as critical", () => {
    expect(classifyPriority("kernel panic - not syncing")).toBe("critical");
  });

  // NOTE: "panicked" (past tense) does NOT match \bpanic\b — gap in classifier
  it("does NOT detect 'panicked' (past tense) — known classifier gap", () => {
    expect(classifyPriority("thread 'main' panicked at...")).toBe("normal");
  });

  it("detects 'cannot' as critical", () => {
    expect(classifyPriority("Cannot find module 'react'")).toBe("critical");
  });

  it("detects 'timeout' as critical", () => {
    expect(classifyPriority("Request timeout after 30s")).toBe("critical");
  });

  it("detects 'timed out' as critical", () => {
    expect(classifyPriority("Connection timed out")).toBe("critical");
  });

  it("detects uppercase 'ERROR' as critical", () => {
    expect(classifyPriority("ERROR: file not found")).toBe("critical");
  });

  it("detects uppercase 'FAILED' as critical", () => {
    expect(classifyPriority("FAILED to connect")).toBe("critical");
  });

  it("detects uppercase 'CRITICAL' as critical", () => {
    expect(classifyPriority("CRITICAL: database down")).toBe("critical");
  });

  it("detects 'syntax error' case-insensitively", () => {
    expect(classifyPriority("Syntax Error on line 42")).toBe("critical");
  });

  it("detects 'stack trace' as critical", () => {
    expect(classifyPriority("See stack trace below")).toBe("critical");
  });

  it("detects 'build failed' as critical", () => {
    expect(classifyPriority("Build failed with 3 errors")).toBe("critical");
  });

  it("detects 'compilation error' as critical", () => {
    expect(classifyPriority("Compilation error in module")).toBe("critical");
  });

  it("detects 'test failed' as critical", () => {
    expect(classifyPriority("3 tests failed")).toBe("critical");
  });

  it("detects 'type error' (with space) as critical", () => {
    expect(classifyPriority("A type error occurred")).toBe("critical");
  });

  // NOTE: "TypeError" (one word, no space) does NOT match \btype error\b — gap in classifier
  it("does NOT detect 'TypeError' (no space) — known classifier gap", () => {
    expect(classifyPriority("TypeError: undefined is not a function")).toBe("normal");
  });

  it("detects 'reference error' (with space) as critical", () => {
    expect(classifyPriority("A reference error was thrown")).toBe("critical");
  });

  // NOTE: "ReferenceError" (one word) does NOT match \breference error\b — gap in classifier
  it("does NOT detect 'ReferenceError' (no space) — known classifier gap", () => {
    expect(classifyPriority("ReferenceError: x is not defined")).toBe("normal");
  });

  it("detects 'unable to' as critical", () => {
    expect(classifyPriority("Unable to resolve dependency")).toBe("critical");
  });

  it("detects 'denied' as critical", () => {
    expect(classifyPriority("Permission denied")).toBe("critical");
  });
});


// =============================================================================
// LOW PRIORITY CLASSIFICATION
// =============================================================================

describe("classifyPriority — low", () => {
  it("detects 'saved' at start as low", () => {
    expect(classifyPriority("Saved")).toBe("low");
  });

  it("detects 'done' at start as low", () => {
    expect(classifyPriority("Done")).toBe("low");
  });

  it("detects 'success' at start as low", () => {
    expect(classifyPriority("Success")).toBe("low");
  });

  it("detects 'completed' at start as low", () => {
    expect(classifyPriority("Completed")).toBe("low");
  });

  it("detects 'saved successfully' as low", () => {
    expect(classifyPriority("File saved successfully")).toBe("low");
  });

  it("detects 'no changes' as low", () => {
    expect(classifyPriority("No changes detected")).toBe("low");
  });

  it("detects 'up to date' as low", () => {
    expect(classifyPriority("Everything is up to date")).toBe("low");
  });

  it("detects 'already exists' as low", () => {
    expect(classifyPriority("File already exists")).toBe("low");
  });

  it("detects 'nothing to commit' as low", () => {
    expect(classifyPriority("nothing to commit")).toBe("low");
  });

  it("detects 'nothing to do' as low", () => {
    expect(classifyPriority("nothing to do")).toBe("low");
  });

  it("detects 'created file' pattern as low", () => {
    expect(classifyPriority("Created file")).toBe("low");
  });

  it("detects 'updated config' pattern as low", () => {
    expect(classifyPriority("Updated config")).toBe("low");
  });
});


// =============================================================================
// NORMAL PRIORITY (FALLBACK)
// =============================================================================

describe("classifyPriority — normal", () => {
  it("classifies generic message as normal", () => {
    expect(classifyPriority("Processing your request")).toBe("normal");
  });

  it("classifies empty string as normal", () => {
    expect(classifyPriority("")).toBe("normal");
  });

  it("classifies regular text as normal", () => {
    expect(classifyPriority("The weather is nice today")).toBe("normal");
  });

  it("classifies code-related but non-error text as normal", () => {
    expect(classifyPriority("Refactoring the auth module")).toBe("normal");
  });

  it("classifies informational message as normal", () => {
    expect(classifyPriority("Starting build process...")).toBe("normal");
  });
});


// =============================================================================
// PRIORITY PRECEDENCE
// =============================================================================

describe("classifyPriority — precedence", () => {
  it("critical takes precedence over low when both match", () => {
    // "saved" at start is low, but "error" makes it critical
    expect(classifyPriority("Error: saved file was corrupted")).toBe("critical");
  });

  it("classifies message with error in mid-sentence as critical", () => {
    expect(classifyPriority("The operation failed unexpectedly")).toBe("critical");
  });
});


// =============================================================================
// PRIORITY TOGGLE PERSISTENCE
// =============================================================================

describe("priority toggle", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("defaults to false (off) when no stored value", () => {
    expect(getStoredPriorityEnabled()).toBe(false);
  });

  it("savePriorityEnabled(true) makes get return true", () => {
    savePriorityEnabled(true);
    expect(getStoredPriorityEnabled()).toBe(true);
  });

  it("savePriorityEnabled(false) makes get return false", () => {
    savePriorityEnabled(true);
    savePriorityEnabled(false);
    expect(getStoredPriorityEnabled()).toBe(false);
  });

  it("persists to localStorage with correct key", () => {
    savePriorityEnabled(true);
    expect(localStorage.getItem("vaak_priority_enabled")).toBe("true");
  });
});
