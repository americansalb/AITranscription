/**
 * Tests for SequenceBanner — the top-of-CollabTab session summary.
 *
 * Scope: ux-engineer:0 PR-A. Pure-presentational component; no Tauri invokes,
 * so no invoke mock. Focus is on visible state: topic, holder badge, elapsed
 * timer ticking, pause state, self-turn highlight, queue-position hint.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import SequenceBanner, {
  type SequenceTurnState,
} from "../components/SequenceBanner";

const NOW_ISO = "2026-04-18T23:00:00Z";

function turnFixture(overrides: Partial<SequenceTurnState> = {}): SequenceTurnState {
  return {
    current_holder: "developer:0",
    queue_remaining: ["tester:0", "manager:0"],
    queue_completed: ["architect:0"],
    started_at: NOW_ISO,
    turn_started_at: NOW_ISO,
    initiator: "human:0",
    topic: "Design authentication flow",
    paused_for_human: false,
    ...overrides,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(NOW_ISO));
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("SequenceBanner visibility", () => {
  it("renders nothing when turn is null", () => {
    const { container } = render(
      <SequenceBanner turn={null} selfRoleInstance={null} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when turn is undefined", () => {
    const { container } = render(
      <SequenceBanner turn={undefined} selfRoleInstance={null} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders banner when turn is active", () => {
    render(<SequenceBanner turn={turnFixture()} selfRoleInstance={null} />);
    expect(screen.getByRole("region")).toBeInTheDocument();
  });
});

describe("SequenceBanner content", () => {
  it("shows topic, current holder, and stage counter", () => {
    render(<SequenceBanner turn={turnFixture()} selfRoleInstance={null} />);
    expect(screen.getByText("Design authentication flow")).toBeInTheDocument();
    expect(screen.getByText("developer:0")).toBeInTheDocument();
    expect(screen.getByText("Stage 2 of 4")).toBeInTheDocument();
  });

  it("shows initiator, remaining count, and completed count in meta row", () => {
    render(<SequenceBanner turn={turnFixture()} selfRoleInstance={null} />);
    expect(screen.getByText(/Started by human:0/)).toBeInTheDocument();
    expect(screen.getByText(/2 after current/)).toBeInTheDocument();
    expect(screen.getByText(/1 done/)).toBeInTheDocument();
  });

  it("renders em-dash for null current_holder (e.g., sequence just ended)", () => {
    render(
      <SequenceBanner
        turn={turnFixture({ current_holder: null })}
        selfRoleInstance={null}
      />,
    );
    const badge = screen.getByText("—");
    expect(badge).toBeInTheDocument();
  });

  it("shows pause note when paused_for_human", () => {
    render(
      <SequenceBanner
        turn={turnFixture({ paused_for_human: true })}
        selfRoleInstance={null}
      />,
    );
    expect(screen.getByText(/Paused — human is speaking/i)).toBeInTheDocument();
  });

  it("omits pause note when not paused", () => {
    render(<SequenceBanner turn={turnFixture()} selfRoleInstance={null} />);
    expect(screen.queryByText(/Paused — human is speaking/i)).toBeNull();
  });
});

describe("SequenceBanner self-turn indication", () => {
  it("shows YOUR TURN when selfRoleInstance matches current_holder", () => {
    render(
      <SequenceBanner
        turn={turnFixture()}
        selfRoleInstance="developer:0"
      />,
    );
    expect(screen.getByText(/YOUR TURN/i)).toBeInTheDocument();
  });

  it("shows queue position when self is upcoming (1-indexed ordinal)", () => {
    render(
      <SequenceBanner
        turn={turnFixture()}
        selfRoleInstance="manager:0"
      />,
    );
    // "manager:0" is 2nd in remaining queue
    expect(screen.getByText(/You are 2nd in queue/i)).toBeInTheDocument();
  });

  it("does not show self hint when selfRoleInstance is null", () => {
    render(
      <SequenceBanner turn={turnFixture()} selfRoleInstance={null} />,
    );
    expect(screen.queryByText(/YOUR TURN/i)).toBeNull();
    expect(screen.queryByText(/in queue/i)).toBeNull();
  });

  it("does not show self hint when self is completed", () => {
    render(
      <SequenceBanner
        turn={turnFixture()}
        selfRoleInstance="architect:0"
      />,
    );
    expect(screen.queryByText(/YOUR TURN/i)).toBeNull();
    expect(screen.queryByText(/in queue/i)).toBeNull();
  });

  it("applies self-turn class when selfRoleInstance matches current_holder", () => {
    const { container } = render(
      <SequenceBanner
        turn={turnFixture()}
        selfRoleInstance="developer:0"
      />,
    );
    const banner = container.querySelector(".sequence-banner");
    expect(banner?.classList.contains("sequence-banner-self-turn")).toBe(true);
  });
});

describe("SequenceBanner elapsed time ticker", () => {
  it("initial elapsed renders 0s when turn started now", () => {
    render(<SequenceBanner turn={turnFixture()} selfRoleInstance={null} />);
    const elapsedEl = screen.getByTitle("How long this turn has been held");
    expect(elapsedEl.textContent).toBe("0s");
  });

  it("ticks elapsed time forward every 1s", () => {
    render(<SequenceBanner turn={turnFixture()} selfRoleInstance={null} />);
    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    const elapsedEl = screen.getByTitle("How long this turn has been held");
    expect(elapsedEl.textContent).toBe("30s");
  });

  it("formats elapsed as minutes+seconds past 60s", () => {
    render(<SequenceBanner turn={turnFixture()} selfRoleInstance={null} />);
    act(() => {
      vi.advanceTimersByTime(95_000);
    });
    const elapsedEl = screen.getByTitle("How long this turn has been held");
    expect(elapsedEl.textContent).toBe("1m 35s");
  });

  it("formats elapsed as hours past 60m", () => {
    render(<SequenceBanner turn={turnFixture()} selfRoleInstance={null} />);
    act(() => {
      vi.advanceTimersByTime(2 * 60 * 60 * 1000 + 5 * 60 * 1000);
    });
    const elapsedEl = screen.getByTitle("How long this turn has been held");
    expect(elapsedEl.textContent).toBe("2h 5m");
  });
});

describe("SequenceBanner accessibility", () => {
  it("has role=region and descriptive aria-label", () => {
    render(<SequenceBanner turn={turnFixture()} selfRoleInstance={null} />);
    const region = screen.getByRole("region");
    expect(region.getAttribute("aria-label")).toContain(
      "Design authentication flow",
    );
  });
});
