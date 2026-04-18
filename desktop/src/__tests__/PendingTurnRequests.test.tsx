/**
 * Tests for PendingTurnRequests — agent raise-hand panel.
 *
 * Scope: ux-engineer:0 PR-F. Focus: visibility gating, Accept/Dismiss invoke
 * contract, age formatting, reason rendering.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import PendingTurnRequests from "../components/PendingTurnRequests";
import type {
  SequenceTurnState,
  SequenceTurnRequest,
} from "../components/SequenceBanner";

const NOW_ISO = "2026-04-18T23:10:00Z";

function request(
  overrides: Partial<SequenceTurnRequest> = {},
): SequenceTurnRequest {
  return {
    requester: "tester:0",
    requested_at: NOW_ISO,
    ...overrides,
  };
}

function turnFixture(
  requests: SequenceTurnRequest[] = [],
): SequenceTurnState {
  return {
    current_holder: "developer:0",
    queue_remaining: ["manager:0"],
    queue_completed: [],
    started_at: "2026-04-18T23:00:00Z",
    turn_started_at: "2026-04-18T23:05:00Z",
    initiator: "human:0",
    topic: "t",
    pending_requests: requests,
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
  vi.useFakeTimers();
  vi.setSystemTime(new Date(NOW_ISO));
  (window as unknown as { __TAURI__?: unknown }).__TAURI__ = { mock: true };
  (invoke as unknown as ReturnType<typeof vi.fn>).mockReset();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

describe("PendingTurnRequests visibility", () => {
  it("renders nothing when turn is null", () => {
    const { container } = render(
      <PendingTurnRequests turn={null} projectDir={PROJECT_DIR} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when projectDir is null", () => {
    const { container } = render(
      <PendingTurnRequests turn={turnFixture([request()])} projectDir={null} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when pending_requests undefined", () => {
    const turn = turnFixture();
    delete (turn as Partial<SequenceTurnState>).pending_requests;
    const { container } = render(
      <PendingTurnRequests turn={turn} projectDir={PROJECT_DIR} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when pending_requests empty", () => {
    const { container } = render(
      <PendingTurnRequests turn={turnFixture([])} projectDir={PROJECT_DIR} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders the panel when at least one request exists", () => {
    render(
      <PendingTurnRequests turn={turnFixture([request()])} projectDir={PROJECT_DIR} />,
    );
    expect(screen.getByRole("region")).toBeInTheDocument();
    expect(screen.getByText(/1 turn request/i)).toBeInTheDocument();
  });

  it("pluralizes title for multiple requests", () => {
    render(
      <PendingTurnRequests
        turn={turnFixture([
          request({ requester: "tester:0" }),
          request({ requester: "manager:0" }),
        ])}
        projectDir={PROJECT_DIR}
      />,
    );
    expect(screen.getByText(/2 turn requests/i)).toBeInTheDocument();
  });
});

describe("PendingTurnRequests content", () => {
  it("shows optional reason text when provided", () => {
    render(
      <PendingTurnRequests
        turn={turnFixture([
          request({ reason: "I have urgent security concerns to flag" }),
        ])}
        projectDir={PROJECT_DIR}
      />,
    );
    expect(
      screen.getByText(/urgent security concerns/i),
    ).toBeInTheDocument();
  });

  it("omits reason element when not provided", () => {
    const { container } = render(
      <PendingTurnRequests turn={turnFixture([request()])} projectDir={PROJECT_DIR} />,
    );
    expect(container.querySelector(".pending-turn-reason")).toBeNull();
  });

  it("formats age < 60s as seconds", () => {
    render(
      <PendingTurnRequests
        turn={turnFixture([
          request({ requested_at: "2026-04-18T23:09:30Z" }), // 30s ago
        ])}
        projectDir={PROJECT_DIR}
      />,
    );
    expect(screen.getByText(/30s ago/)).toBeInTheDocument();
  });

  it("formats age < 60m as minutes", () => {
    render(
      <PendingTurnRequests
        turn={turnFixture([
          request({ requested_at: "2026-04-18T23:03:00Z" }), // 7m ago
        ])}
        projectDir={PROJECT_DIR}
      />,
    );
    expect(screen.getByText(/7m ago/)).toBeInTheDocument();
  });

  it("formats age ≥ 60m as hours", () => {
    render(
      <PendingTurnRequests
        turn={turnFixture([
          request({ requested_at: "2026-04-18T21:10:00Z" }), // 2h ago
        ])}
        projectDir={PROJECT_DIR}
      />,
    );
    expect(screen.getByText(/2h ago/)).toBeInTheDocument();
  });
});

describe("PendingTurnRequests actions", () => {
  it("Accept invokes accept_turn_request with the correct requester", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <PendingTurnRequests
        turn={turnFixture([request({ requester: "tester:0" })])}
        projectDir={PROJECT_DIR}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: /accept turn request from tester:0/i,
      }),
    );
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("accept_turn_request", {
      projectDir: PROJECT_DIR,
      requester: "tester:0",
    });
  });

  it("Dismiss invokes dismiss_turn_request with the correct requester", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <PendingTurnRequests
        turn={turnFixture([request({ requester: "tester:0" })])}
        projectDir={PROJECT_DIR}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: /dismiss turn request from tester:0/i,
      }),
    );
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("dismiss_turn_request", {
      projectDir: PROJECT_DIR,
      requester: "tester:0",
    });
  });

  it("multiple requesters get independently labeled accept buttons", () => {
    render(
      <PendingTurnRequests
        turn={turnFixture([
          request({ requester: "tester:0" }),
          request({ requester: "manager:0" }),
        ])}
        projectDir={PROJECT_DIR}
      />,
    );
    expect(
      screen.getByRole("button", { name: /accept turn request from tester:0/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /accept turn request from manager:0/i }),
    ).toBeInTheDocument();
  });
});
