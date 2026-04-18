/**
 * Tests for ModeratorSequencePanel — reorder/insert/remove/pause/skip.
 *
 * Scope: ux-engineer:0 PR-D. Exercises the Tauri invoke contract for each
 * control + the collapse/expand interaction + the insert-dropdown filtering
 * that excludes already-queued roles.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import ModeratorSequencePanel, {
  type ModeratorSequencePanelRosterEntry,
} from "../components/ModeratorSequencePanel";
import type { SequenceTurnState } from "../components/SequenceBanner";

function turnFixture(overrides: Partial<SequenceTurnState> = {}): SequenceTurnState {
  return {
    current_holder: "developer:0",
    queue_remaining: ["tester:0", "manager:0", "architect:0"],
    queue_completed: [],
    started_at: "2026-04-18T23:00:00Z",
    turn_started_at: "2026-04-18T23:00:00Z",
    initiator: "human:0",
    topic: "t",
    ...overrides,
  };
}

const ROSTER: ModeratorSequencePanelRosterEntry[] = [
  { id: "developer:0", title: "Developer" },
  { id: "tester:0", title: "Tester" },
  { id: "manager:0", title: "Project Manager" },
  { id: "architect:0", title: "Architect" },
  { id: "moderator:0", title: "Debate Moderator" },
];

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

describe("ModeratorSequencePanel visibility", () => {
  it("renders nothing when turn is null", () => {
    const { container } = render(
      <ModeratorSequencePanel
        turn={null}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when projectDir is null", () => {
    const { container } = render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={null}
        availableRoleInstances={ROSTER}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders collapsed toggle by default when active", () => {
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    const toggle = screen.getByRole("button", { name: /moderator controls/i });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    // Body controls should not be visible
    expect(screen.queryByText("Pause")).toBeNull();
  });
});

describe("ModeratorSequencePanel expand/collapse", () => {
  it("expanding reveals control row and queue list", () => {
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    const toggle = screen.getByRole("button", { name: /moderator controls/i });
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("button", { name: /pause the sequence/i })).toBeInTheDocument();
    expect(screen.getByText("Upcoming")).toBeInTheDocument();
  });

  it("summary shows N upcoming and paused flag", () => {
    render(
      <ModeratorSequencePanel
        turn={turnFixture({ paused_for_human: true })}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    expect(screen.getByText(/3 upcoming · paused/i)).toBeInTheDocument();
  });
});

describe("ModeratorSequencePanel pause/resume", () => {
  it("Pause button invokes pause_sequence", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    fireEvent.click(screen.getByRole("button", { name: /pause the sequence/i }));
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("pause_sequence", {
      projectDir: PROJECT_DIR,
    });
  });

  it("Resume button (when paused) invokes resume_sequence", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <ModeratorSequencePanel
        turn={turnFixture({ paused_for_human: true })}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    fireEvent.click(screen.getByRole("button", { name: /resume the sequence/i }));
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("resume_sequence", {
      projectDir: PROJECT_DIR,
    });
  });
});

describe("ModeratorSequencePanel skip", () => {
  it("Skip current invokes skip_current_turn", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    fireEvent.click(screen.getByRole("button", { name: /force-advance past/i }));
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("skip_current_turn", {
      projectDir: PROJECT_DIR,
    });
  });

  it("Skip disabled when no current holder", () => {
    render(
      <ModeratorSequencePanel
        turn={turnFixture({ current_holder: null })}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    const skipBtn = screen.getByRole("button", { name: /force-advance past/i });
    expect(skipBtn).toBeDisabled();
  });
});

describe("ModeratorSequencePanel reorder", () => {
  it("move-up invokes reorder_queue with swapped order", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    // Move manager:0 up (index 1 → 0)
    fireEvent.click(screen.getByRole("button", { name: /move manager:0 up/i }));
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("reorder_queue", {
      projectDir: PROJECT_DIR,
      newOrder: ["manager:0", "tester:0", "architect:0"],
    });
  });

  it("move-up disabled on first item", () => {
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    const btn = screen.getByRole("button", { name: /move tester:0 up/i });
    expect(btn).toBeDisabled();
  });

  it("move-down disabled on last item", () => {
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    const btn = screen.getByRole("button", { name: /move architect:0 down/i });
    expect(btn).toBeDisabled();
  });
});

describe("ModeratorSequencePanel remove", () => {
  it("remove invokes remove_role_from_queue with the target role", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    fireEvent.click(screen.getByRole("button", { name: /remove manager:0 from queue/i }));
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("remove_role_from_queue", {
      projectDir: PROJECT_DIR,
      roleInstance: "manager:0",
    });
  });
});

describe("ModeratorSequencePanel insert", () => {
  it("dropdown excludes already-queued and current-holder roles", () => {
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    const select = screen.getByRole("combobox", {
      name: /role to insert at end of queue/i,
    });
    // Expect moderator:0 (not in queue, not current) as option; other options excluded.
    expect(select.textContent).toContain("moderator:0");
    expect(select.textContent).not.toContain("developer:0");
    expect(select.textContent).not.toContain("tester:0");
  });

  it("insert invokes insert_role_in_queue with correct position", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    const select = screen.getByRole("combobox", {
      name: /role to insert at end of queue/i,
    });
    fireEvent.change(select, { target: { value: "moderator:0" } });
    fireEvent.click(screen.getByRole("button", { name: /add to end/i }));
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("insert_role_in_queue", {
      projectDir: PROJECT_DIR,
      roleInstance: "moderator:0",
      position: 3,
    });
  });

  it("insert disabled when no roles eligible", () => {
    render(
      <ModeratorSequencePanel
        turn={turnFixture()}
        projectDir={PROJECT_DIR}
        availableRoleInstances={[
          { id: "developer:0", title: "Developer" },
          { id: "tester:0", title: "Tester" },
          { id: "manager:0", title: "Project Manager" },
          { id: "architect:0", title: "Architect" },
        ]}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    const select = screen.getByRole("combobox");
    expect(select).toBeDisabled();
    expect(screen.getByText(/No eligible roles/i)).toBeInTheDocument();
  });
});

describe("ModeratorSequencePanel empty queue state", () => {
  it('shows "Queue is empty." message when no remaining roles', () => {
    render(
      <ModeratorSequencePanel
        turn={turnFixture({ queue_remaining: [] })}
        projectDir={PROJECT_DIR}
        availableRoleInstances={ROSTER}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /moderator controls/i }));
    expect(screen.getByText(/Queue is empty/i)).toBeInTheDocument();
  });
});
