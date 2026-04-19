/**
 * Tests for QuickLaunchBar — guards the pipeline-removal gate.
 *
 * Pipeline was removed from the launcher because (a) new pipelines
 * are rejected by both the MCP and Tauri gates, and (b) the pill
 * was the default selection, which turned every default quick-launch
 * into an error after the Tauri gate landed.
 *
 * These tests lock in the removal so a future re-introduction requires
 * a deliberate test update.
 */
import { describe, it, expect, afterEach } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QuickLaunchBar } from "../components/QuickLaunchBar";

afterEach(() => {
  cleanup();
});

describe("QuickLaunchBar — pipeline removal", () => {
  it("does not render a Pipeline format pill", () => {
    render(
      <QuickLaunchBar
        discussionActive={false}
        launching={false}
        onLaunch={() => { /* noop */ }}
        onOpenAdvanced={() => { /* noop */ }}
      />,
    );
    expect(screen.queryByRole("radio", { name: /Pipeline format/i })).toBeNull();
    // The four remaining formats must still render.
    expect(screen.getByRole("radio", { name: /Delphi format/i })).not.toBeNull();
    expect(screen.getByRole("radio", { name: /Oxford format/i })).not.toBeNull();
    expect(screen.getByRole("radio", { name: /Red Team format/i })).not.toBeNull();
    expect(screen.getByRole("radio", { name: /Continuous format/i })).not.toBeNull();
  });

  it("does not launch with mode=pipeline when the user clicks Go on defaults", () => {
    const launches: Array<{ format: string; topic: string }> = [];
    render(
      <QuickLaunchBar
        discussionActive={false}
        launching={false}
        onLaunch={(format, topic) => { launches.push({ format, topic }); }}
        onOpenAdvanced={() => { /* noop */ }}
      />,
    );
    fireEvent.change(screen.getByLabelText("Discussion topic"), {
      target: { value: "any topic" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Start discussion" }));
    expect(launches).toHaveLength(1);
    expect(launches[0].format).not.toBe("pipeline");
    // Default should be a safe, user-visible option that the Tauri gate will accept.
    expect(launches[0].format).toBe("delphi");
  });
});
