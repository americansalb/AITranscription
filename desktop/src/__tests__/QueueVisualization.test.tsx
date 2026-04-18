/**
 * Tests for QueueVisualization — horizontal done/current/upcoming chips.
 *
 * Scope: ux-engineer:0 PR-B. Pure-presentational. Focus: correct state class
 * per chip, ordering preserved from completed → current → upcoming, NOW badge
 * only on current, separators between chips.
 */
import { describe, it, expect, afterEach } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import QueueVisualization from "../components/QueueVisualization";
import type { SequenceTurnState } from "../components/SequenceBanner";

function turnFixture(overrides: Partial<SequenceTurnState> = {}): SequenceTurnState {
  return {
    current_holder: "developer:0",
    queue_remaining: ["tester:0", "manager:0"],
    queue_completed: ["architect:0"],
    started_at: "2026-04-18T23:00:00Z",
    turn_started_at: "2026-04-18T23:00:00Z",
    initiator: "human:0",
    topic: "t",
    ...overrides,
  };
}

afterEach(() => cleanup());

describe("QueueVisualization visibility", () => {
  it("renders nothing when turn is null", () => {
    const { container } = render(<QueueVisualization turn={null} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when turn is undefined", () => {
    const { container } = render(<QueueVisualization turn={undefined} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when all queues are empty and no current", () => {
    const { container } = render(
      <QueueVisualization
        turn={turnFixture({
          current_holder: null,
          queue_remaining: [],
          queue_completed: [],
        })}
      />,
    );
    expect(container.firstChild).toBeNull();
  });
});

describe("QueueVisualization chip rendering", () => {
  it("renders done → current → upcoming in that order", () => {
    render(<QueueVisualization turn={turnFixture()} />);
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(4);
    // DOM order reflects queue order
    expect(items[0].textContent).toContain("architect");
    expect(items[1].textContent).toContain("developer");
    expect(items[2].textContent).toContain("tester");
    expect(items[3].textContent).toContain("manager");
  });

  it("applies correct state classes to chips", () => {
    const { container } = render(<QueueVisualization turn={turnFixture()} />);
    const done = container.querySelector(".queue-chip-done");
    const current = container.querySelector(".queue-chip-current");
    const upcoming = container.querySelectorAll(".queue-chip-upcoming");
    expect(done?.textContent).toContain("architect");
    expect(current?.textContent).toContain("developer");
    expect(upcoming).toHaveLength(2);
  });

  it("shows NOW badge only on current chip", () => {
    render(<QueueVisualization turn={turnFixture()} />);
    const nowBadges = screen.getAllByText("NOW");
    expect(nowBadges).toHaveLength(1);
  });

  it("renders separator between chips but not after last", () => {
    const { container } = render(<QueueVisualization turn={turnFixture()} />);
    const separators = container.querySelectorAll(".queue-chip-separator");
    // 4 chips → 3 separators
    expect(separators).toHaveLength(3);
  });
});

describe("QueueVisualization accessibility", () => {
  it("container is a list with descriptive aria-label", () => {
    render(<QueueVisualization turn={turnFixture()} />);
    const list = screen.getByRole("list");
    expect(list.getAttribute("aria-label")).toMatch(/completed, current, upcoming/i);
  });

  it("each chip has an aria-label describing its role and state", () => {
    render(<QueueVisualization turn={turnFixture()} />);
    expect(
      screen.getByLabelText(/architect:0.*completed/i),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText(/developer:0.*on turn now/i),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText(/tester:0.*upcoming, position 3/i),
    ).toBeInTheDocument();
  });
});

describe("QueueVisualization edge cases", () => {
  it("handles only-completed queue", () => {
    render(
      <QueueVisualization
        turn={turnFixture({
          current_holder: null,
          queue_remaining: [],
          queue_completed: ["a:0", "b:0"],
        })}
      />,
    );
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(screen.queryByText("NOW")).toBeNull();
  });

  it("handles only-current no queue", () => {
    render(
      <QueueVisualization
        turn={turnFixture({
          queue_remaining: [],
          queue_completed: [],
        })}
      />,
    );
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(1);
    expect(screen.getAllByText("NOW")).toHaveLength(1);
  });

  it("handles role id without :instance (defaults to :0)", () => {
    render(
      <QueueVisualization
        turn={turnFixture({
          current_holder: "loneranger",
          queue_remaining: [],
          queue_completed: [],
        })}
      />,
    );
    expect(screen.getByText("loneranger")).toBeInTheDocument();
    expect(screen.getByText(":0")).toBeInTheDocument();
  });
});
