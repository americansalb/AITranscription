/**
 * Tests for HumanSequenceOverrideBar — Insert me next, End session.
 *
 * Scope: ux-engineer:0 PR-E. Focus: Tauri invoke contract for the two
 * actions, the inline destructive-confirm flow for End session, busy state.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import HumanSequenceOverrideBar from "../components/HumanSequenceOverrideBar";
import type { SequenceTurnState } from "../components/SequenceBanner";

function turnFixture(overrides: Partial<SequenceTurnState> = {}): SequenceTurnState {
  return {
    current_holder: "developer:0",
    queue_remaining: ["tester:0"],
    queue_completed: [],
    started_at: "2026-04-18T23:00:00Z",
    turn_started_at: "2026-04-18T23:00:00Z",
    initiator: "human:0",
    topic: "t",
    ...overrides,
  };
}

const PROJECT_DIR = "/tmp/test-proj";

function installMockInvoke() {
  const mockInvoke = invoke as unknown as ReturnType<typeof vi.fn>;
  mockInvoke.mockImplementation(async () => undefined);
  return mockInvoke;
}

async function flushMicrotasks() {
  for (let i = 0; i < 5; i++) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

beforeEach(() => {
  (window as unknown as { __TAURI__?: unknown }).__TAURI__ = { mock: true };
  (invoke as unknown as ReturnType<typeof vi.fn>).mockReset();
});

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

describe("HumanSequenceOverrideBar visibility", () => {
  it("renders nothing when turn is null", () => {
    const { container } = render(
      <HumanSequenceOverrideBar turn={null} projectDir={PROJECT_DIR} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when projectDir is null", () => {
    const { container } = render(
      <HumanSequenceOverrideBar turn={turnFixture()} projectDir={null} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders both buttons when sequence is active", () => {
    render(<HumanSequenceOverrideBar turn={turnFixture()} projectDir={PROJECT_DIR} />);
    expect(screen.getByRole("button", { name: /jump to the front/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /end the current sequence/i })).toBeInTheDocument();
  });
});

describe("HumanSequenceOverrideBar insert-me-next", () => {
  it("invokes human_insert_next on click", async () => {
    const mockInvoke = installMockInvoke();
    render(<HumanSequenceOverrideBar turn={turnFixture()} projectDir={PROJECT_DIR} />);
    fireEvent.click(screen.getByRole("button", { name: /jump to the front/i }));
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("human_insert_next", {
      projectDir: PROJECT_DIR,
    });
  });
});

describe("HumanSequenceOverrideBar end-session confirm flow", () => {
  it("first click reveals confirm row instead of firing end_sequence", async () => {
    const mockInvoke = installMockInvoke();
    render(<HumanSequenceOverrideBar turn={turnFixture()} projectDir={PROJECT_DIR} />);
    fireEvent.click(screen.getByRole("button", { name: /end the current sequence/i }));
    await flushMicrotasks();
    // Confirm prompt appears
    expect(screen.getByText(/End this session\?/i)).toBeInTheDocument();
    // end_sequence NOT called yet
    expect(mockInvoke).not.toHaveBeenCalledWith(
      "end_sequence",
      expect.anything(),
    );
  });

  it("second click (End in confirm row) invokes end_sequence", async () => {
    const mockInvoke = installMockInvoke();
    render(<HumanSequenceOverrideBar turn={turnFixture()} projectDir={PROJECT_DIR} />);
    fireEvent.click(screen.getByRole("button", { name: /end the current sequence/i }));
    // Confirm row now visible; it has an "End" button (exact match)
    const endBtn = screen.getByRole("button", { name: "End" });
    fireEvent.click(endBtn);
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("end_sequence", {
      projectDir: PROJECT_DIR,
    });
  });

  it("Cancel returns to the two-button layout without invoking", async () => {
    const mockInvoke = installMockInvoke();
    render(<HumanSequenceOverrideBar turn={turnFixture()} projectDir={PROJECT_DIR} />);
    fireEvent.click(screen.getByRole("button", { name: /end the current sequence/i }));
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    await flushMicrotasks();
    // Original "End session" button is back
    expect(screen.getByRole("button", { name: /end the current sequence/i })).toBeInTheDocument();
    expect(screen.queryByText(/End this session\?/i)).toBeNull();
    expect(mockInvoke).not.toHaveBeenCalledWith(
      "end_sequence",
      expect.anything(),
    );
  });
});

describe("HumanSequenceOverrideBar accessibility", () => {
  it("labels the region for screen readers", () => {
    render(<HumanSequenceOverrideBar turn={turnFixture()} projectDir={PROJECT_DIR} />);
    expect(screen.getByRole("region", { name: /human controls for active sequence/i })).toBeInTheDocument();
  });
});

describe("HumanSequenceOverrideBar end-my-turn branch (PR-H)", () => {
  it('swaps "Insert me next" for "End my turn" when human is current_holder', () => {
    render(
      <HumanSequenceOverrideBar
        turn={turnFixture({ current_holder: "human:0" })}
        projectDir={PROJECT_DIR}
      />,
    );
    expect(screen.getByRole("button", { name: /end your current turn/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /jump to the front/i })).toBeNull();
  });

  it("End my turn invokes pass_turn", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <HumanSequenceOverrideBar
        turn={turnFixture({ current_holder: "human:0" })}
        projectDir={PROJECT_DIR}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /end your current turn/i }));
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("pass_turn", { projectDir: PROJECT_DIR });
  });

  it("still shows Insert me next when current_holder is another role", () => {
    render(
      <HumanSequenceOverrideBar
        turn={turnFixture({ current_holder: "developer:0" })}
        projectDir={PROJECT_DIR}
      />,
    );
    expect(screen.getByRole("button", { name: /jump to the front/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /end your current turn/i })).toBeNull();
  });
});
