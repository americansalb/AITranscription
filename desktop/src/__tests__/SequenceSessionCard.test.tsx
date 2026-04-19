/**
 * Tests for SequenceSessionCard — pr-pipeline-unified-controls PR-3a.
 *
 * Scope: when isPipelineMode=true, the read-only visualization (banner + queue)
 * renders but the interactive children (HumanSequenceOverrideBar,
 * PendingTurnRequests, ModeratorSequencePanel) are HIDDEN. Their button
 * handlers invoke active_sequence-specific Tauri commands that don't operate
 * on pipeline state; PR-3b will rewire them.
 *
 * When isPipelineMode=false (default, sequence mode), all 5 children render
 * as before.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import SequenceSessionCard from "../components/SequenceSessionCard";
import type { SequenceTurnState } from "../components/SequenceBanner";

afterEach(() => cleanup());

// Mock the Tauri invoke so HumanSequenceOverrideBar / ModeratorSequencePanel /
// PendingTurnRequests don't try to import the real @tauri-apps module in jsdom.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

function turn(): SequenceTurnState {
  return {
    current_holder: "developer:0",
    queue_remaining: ["tester:0", "manager:0"],
    queue_completed: ["architect:0"],
    started_at: "2026-04-19T00:00:00Z",
    turn_started_at: "2026-04-19T00:00:00Z",
    initiator: "human:0",
    topic: "t",
  };
}

describe("SequenceSessionCard pipeline-mode rendering", () => {
  it("returns null when turn is null regardless of mode", () => {
    const { container } = render(
      <SequenceSessionCard
        turn={null}
        projectDir="/tmp"
        availableRoleInstances={[]}
        isPipelineMode={false}
      />
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders read-only visualization in pipeline mode (banner + queue)", () => {
    render(
      <SequenceSessionCard
        turn={turn()}
        projectDir="/tmp"
        availableRoleInstances={[]}
        isPipelineMode={true}
      />
    );
    // Banner / queue display the current holder. SequenceBanner uses the
    // role label; QueueVisualization shows chips for completed/current/upcoming.
    expect(screen.getAllByText(/developer:0/).length).toBeGreaterThan(0);
  });

  it("RENDERS HumanSequenceOverrideBar in pipeline mode (PR-3b)", () => {
    // PR-3b updated: the override bar now renders in pipeline mode too. Its
    // button handlers detect isPipelineMode and route to pipeline_advance /
    // pipeline_insert_self_next / end_discussion instead of the sequence
    // commands. Mode is opaque to the user — buttons look identical.
    render(
      <SequenceSessionCard
        turn={turn()}
        projectDir="/tmp"
        availableRoleInstances={[]}
        isPipelineMode={true}
      />
    );
    expect(screen.getByLabelText(/human controls for active sequence/i)).toBeTruthy();
  });

  it("PR-5: shows the auto-advance indicator in pipeline mode", () => {
    render(
      <SequenceSessionCard
        turn={turn()}
        projectDir="/tmp"
        availableRoleInstances={[]}
        isPipelineMode={true}
      />
    );
    expect(screen.getByLabelText(/auto-advance behavior/i)).toBeTruthy();
    expect(screen.getByText(/auto-advance:/i)).toBeTruthy();
    expect(screen.getByText(/300s/i)).toBeTruthy();
  });

  it("PR-5: hides the auto-advance indicator in sequence mode", () => {
    render(
      <SequenceSessionCard
        turn={turn()}
        projectDir="/tmp"
        availableRoleInstances={[]}
        isPipelineMode={false}
      />
    );
    expect(screen.queryByLabelText(/auto-advance behavior/i)).toBeNull();
  });

  it("HIDES ModeratorSequencePanel in pipeline mode", () => {
    render(
      <SequenceSessionCard
        turn={turn()}
        projectDir="/tmp"
        availableRoleInstances={[{ id: "tech-leader:1", title: "Tech Leader" }]}
        isPipelineMode={true}
      />
    );
    // ModeratorSequencePanel header is "Moderator controls".
    expect(screen.queryByText(/moderator controls/i)).toBeNull();
  });

  it("RENDERS HumanSequenceOverrideBar in sequence mode (default)", () => {
    render(
      <SequenceSessionCard
        turn={turn()}
        projectDir="/tmp"
        availableRoleInstances={[]}
      />
    );
    // Default isPipelineMode is false -> override bar visible. The override
    // bar renders "Jump to the front of the queue for your next turn" when
    // current_holder != human:0 (our fixture has developer:0 holding).
    expect(screen.getAllByLabelText(/jump to the front of the queue/i).length).toBeGreaterThan(0);
  });

  it("RENDERS ModeratorSequencePanel in sequence mode (default)", () => {
    render(
      <SequenceSessionCard
        turn={turn()}
        projectDir="/tmp"
        availableRoleInstances={[{ id: "tech-leader:1", title: "Tech Leader" }]}
      />
    );
    expect(screen.getAllByText(/moderator controls/i).length).toBeGreaterThan(0);
  });
});
