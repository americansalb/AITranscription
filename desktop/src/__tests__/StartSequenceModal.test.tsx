/**
 * Tests for StartSequenceModal — PR-G human-initiated start.
 *
 * Focus: visibility gating, participant selection, reorder, discussion_control
 * invoke contract, error surface, validation.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import StartSequenceModal, {
  type StartSequenceCandidate,
} from "../components/StartSequenceModal";

const PROJECT_DIR = "/tmp/test-proj";

const CANDIDATES: StartSequenceCandidate[] = [
  { id: "developer:0", title: "Developer" },
  { id: "tester:0", title: "Tester" },
  { id: "manager:0", title: "Project Manager" },
];

function installMockInvoke(returnOrError: unknown = undefined) {
  const mockInvoke = invoke as unknown as ReturnType<typeof vi.fn>;
  mockInvoke.mockImplementation(async () => {
    if (returnOrError instanceof Error) throw returnOrError;
    return returnOrError;
  });
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

describe("StartSequenceModal visibility", () => {
  it("renders nothing when open=false", () => {
    const { container } = render(
      <StartSequenceModal
        open={false}
        onClose={() => {}}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders dialog when open=true", () => {
    render(
      <StartSequenceModal
        open={true}
        onClose={() => {}}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("Start Sequence")).toBeInTheDocument();
  });
});

describe("StartSequenceModal participant selection", () => {
  it("adds a candidate to the selected list when their chip is clicked", () => {
    render(
      <StartSequenceModal
        open={true}
        onClose={() => {}}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.click(screen.getByText("Tester"));
    expect(screen.getByText(/#1/)).toBeInTheDocument();
  });

  it("removes a candidate from selected when its remove button is clicked", () => {
    render(
      <StartSequenceModal
        open={true}
        onClose={() => {}}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.click(screen.getByText("Tester"));
    fireEvent.click(screen.getByRole("button", { name: /remove tester:0/i }));
    expect(screen.queryByText(/#1/)).toBeNull();
  });

  it("move-up swaps positions and move-down swaps back", () => {
    render(
      <StartSequenceModal
        open={true}
        onClose={() => {}}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.click(screen.getByText("Developer"));
    fireEvent.click(screen.getByText("Tester"));
    fireEvent.click(screen.getByText("Project Manager"));
    // Now sorted: developer:0 (#1), tester:0 (#2), manager:0 (#3)
    fireEvent.click(screen.getByRole("button", { name: /move manager:0 up/i }));
    const items = screen.getAllByRole("listitem");
    // After move-up of manager:0 from #3, order becomes developer:0 (#1), manager:0 (#2), tester:0 (#3)
    expect(items[1].textContent).toContain("manager");
    expect(items[2].textContent).toContain("tester");
  });

  it("Start button disabled without topic", () => {
    render(
      <StartSequenceModal
        open={true}
        onClose={() => {}}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.click(screen.getByText("Developer"));
    expect(
      screen.getByRole("button", { name: /start sequence with 1 participant/i }),
    ).toBeDisabled();
  });

  it("Start button disabled without participants", () => {
    render(
      <StartSequenceModal
        open={true}
        onClose={() => {}}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.change(screen.getByPlaceholderText(/what is this sequence about/i), {
      target: { value: "Test topic" },
    });
    expect(
      screen.getByRole("button", { name: /start sequence with 0 participants/i }),
    ).toBeDisabled();
  });
});

describe("StartSequenceModal submission", () => {
  it("invokes discussion_control with correct args on successful submit", async () => {
    const mockInvoke = installMockInvoke();
    const onClose = vi.fn();
    render(
      <StartSequenceModal
        open={true}
        onClose={onClose}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.change(screen.getByPlaceholderText(/what is this sequence about/i), {
      target: { value: "Smoke test" },
    });
    fireEvent.change(screen.getByPlaceholderText(/what outcome/i), {
      target: { value: "Verify gate" },
    });
    fireEvent.click(screen.getByText("Developer"));
    fireEvent.click(screen.getByText("Tester"));
    fireEvent.click(
      screen.getByRole("button", { name: /start sequence with 2 participants/i }),
    );
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith("discussion_control", {
      dir: PROJECT_DIR,
      action: "start_sequence",
      topic: "Smoke test",
      goal: "Verify gate",
      participants: ["developer:0", "tester:0"],
    });
    expect(onClose).toHaveBeenCalled();
  });

  it("shows error and does not close on rejection", async () => {
    installMockInvoke(new Error("ERR_SEQUENCE_ALREADY_ACTIVE"));
    const onClose = vi.fn();
    render(
      <StartSequenceModal
        open={true}
        onClose={onClose}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.change(screen.getByPlaceholderText(/what is this sequence about/i), {
      target: { value: "Topic" },
    });
    fireEvent.click(screen.getByText("Developer"));
    fireEvent.click(screen.getByRole("button", { name: /start sequence/i }));
    await flushMicrotasks();
    expect(screen.getByRole("alert").textContent).toMatch(/ALREADY_ACTIVE/);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("sends null goal when goal field is empty", async () => {
    const mockInvoke = installMockInvoke();
    render(
      <StartSequenceModal
        open={true}
        onClose={() => {}}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.change(screen.getByPlaceholderText(/what is this sequence about/i), {
      target: { value: "Topic" },
    });
    fireEvent.click(screen.getByText("Developer"));
    fireEvent.click(screen.getByRole("button", { name: /start sequence/i }));
    await flushMicrotasks();
    expect(mockInvoke).toHaveBeenCalledWith(
      "discussion_control",
      expect.objectContaining({ goal: null }),
    );
  });
});

describe("StartSequenceModal keyboard + close", () => {
  it("Cancel button calls onClose", () => {
    const onClose = vi.fn();
    render(
      <StartSequenceModal
        open={true}
        onClose={onClose}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    expect(onClose).toHaveBeenCalled();
  });

  it("Backdrop click calls onClose", () => {
    const onClose = vi.fn();
    const { container } = render(
      <StartSequenceModal
        open={true}
        onClose={onClose}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    const backdrop = container.querySelector(".start-sequence-modal-backdrop");
    fireEvent.click(backdrop as Element);
    expect(onClose).toHaveBeenCalled();
  });

  it("clicking inside the modal does NOT close it", () => {
    const onClose = vi.fn();
    render(
      <StartSequenceModal
        open={true}
        onClose={onClose}
        projectDir={PROJECT_DIR}
        candidates={CANDIDATES}
      />,
    );
    fireEvent.click(screen.getByText("Start Sequence"));
    expect(onClose).not.toHaveBeenCalled();
  });
});

describe("StartSequenceModal empty candidates", () => {
  it("shows empty message when no candidates and no selected", () => {
    render(
      <StartSequenceModal
        open={true}
        onClose={() => {}}
        projectDir={PROJECT_DIR}
        candidates={[]}
      />,
    );
    expect(screen.getByText(/no active roles available/i)).toBeInTheDocument();
  });
});
