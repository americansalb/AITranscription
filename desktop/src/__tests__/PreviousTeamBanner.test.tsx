/**
 * Tests for PreviousTeamBanner — the PR3 Relaunch UI.
 *
 * Scope agreed in pipeline 2026-04-18 (tester:0 msg 205 / tester:1 msg 209).
 * Covers the double-click race matrix ux-engineer:0 laid out in msg 185 plus
 * the unmount-remount edge case tester:1 added in msg 197. These tests
 * document where the UI debounce is load-bearing and — for the remount case
 * — where it is structurally insufficient. That last gap is what motivates
 * the Rust-side AtomicBool guard tracked in PR2.5 (pr-spawned-manifest-
 * durability).
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import PreviousTeamBanner, {
  STAGGER_MS,
  POST_RELAUNCH_BUFFER_MS,
  debounceWindowMs,
  type PreviousTeamEntry,
} from "../components/PreviousTeamBanner";

// ── test harness ─────────────────────────────────────────────────────────────

// Two dead entries + one alive = deadCount of 2 for the banner's rendering math.
const MANIFEST_MIXED: PreviousTeamEntry[] = [
  { role: "developer", instance: 0, pid: 1000, spawned_at: "2026-04-18T20:00:00Z", alive: false },
  { role: "manager", instance: 0, pid: 1001, spawned_at: "2026-04-18T20:00:00Z", alive: false },
  { role: "moderator", instance: 0, pid: 1002, spawned_at: "2026-04-18T20:00:00Z", alive: true },
];

type InvokeSpec = {
  peek?: PreviousTeamEntry[] | Error;
  relaunch?: number | Error;
  discard?: unknown | Error;
};

function installInvokeMock(spec: InvokeSpec) {
  const mockInvoke = invoke as unknown as ReturnType<typeof vi.fn>;
  mockInvoke.mockImplementation(async (command: string) => {
    switch (command) {
      case "peek_spawned_manifest": {
        if (spec.peek instanceof Error) throw spec.peek;
        return spec.peek ?? [];
      }
      case "relaunch_spawned": {
        if (spec.relaunch instanceof Error) throw spec.relaunch;
        return spec.relaunch ?? 0;
      }
      case "discard_spawned_manifest": {
        if (spec.discard instanceof Error) throw spec.discard;
        return spec.discard;
      }
      default:
        return undefined;
    }
  });
  return mockInvoke;
}

// Parent normally wraps the banner in a confirm flow. Tests exercise both the
// "user confirms → execute fires" path and the "user cancels → execute never
// fires" path by varying this helper.
function autoConfirmOnRequestLaunch(capturedQueued: { value: number }) {
  return async (count: number, execute: () => Promise<number>) => {
    capturedQueued.value = await execute();
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  (window as unknown as { __TAURI__?: unknown }).__TAURI__ = { mock: true };
  (invoke as unknown as ReturnType<typeof vi.fn>).mockReset();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

// flushMicrotasks — dynamic `import("@tauri-apps/api/core")` inside the banner
// resolves through the microtask queue, which vi.useFakeTimers does not drain.
// We advance fake timers by zero to let the event loop chew through microtasks
// queued during rendering + user interaction.
async function flushMicrotasks() {
  // Multiple passes let chained awaits resolve.
  for (let i = 0; i < 5; i++) {
    await act(async () => {
      await Promise.resolve();
      vi.advanceTimersByTime(0);
    });
  }
}

// ── exported-constant sanity check ──────────────────────────────────────────

describe("PreviousTeamBanner exported timing constants", () => {
  it("STAGGER_MS matches launcher.rs sleep(Duration::from_secs(2))", () => {
    // Coupling note per ux-engineer:0 msg 204: if developer:0 changes
    // launcher.rs:1243 sleep, update this constant and test together.
    expect(STAGGER_MS).toBe(2000);
  });

  it("POST_RELAUNCH_BUFFER_MS covers spawn startup + disk settle", () => {
    expect(POST_RELAUNCH_BUFFER_MS).toBe(1500);
  });

  it("debounceWindowMs(queuedCount) = queued * STAGGER_MS + buffer", () => {
    expect(debounceWindowMs(0)).toBe(POST_RELAUNCH_BUFFER_MS);
    expect(debounceWindowMs(1)).toBe(STAGGER_MS + POST_RELAUNCH_BUFFER_MS);
    expect(debounceWindowMs(3)).toBe(3 * STAGGER_MS + POST_RELAUNCH_BUFFER_MS);
    expect(debounceWindowMs(10)).toBe(10 * STAGGER_MS + POST_RELAUNCH_BUFFER_MS);
  });
});

// ── #1 golden path ──────────────────────────────────────────────────────────

describe("PreviousTeamBanner golden path", () => {
  it("invokes relaunch_spawned exactly once and locks the button while queued", async () => {
    const mockInvoke = installInvokeMock({ peek: MANIFEST_MIXED, relaunch: 2 });
    const captured = { value: -1 };

    render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={true}
        onRequestLaunch={autoConfirmOnRequestLaunch(captured)}
      />,
    );
    await flushMicrotasks();

    const button = screen.getByRole("button", { name: /relaunch 2 roles/i });
    expect(button).toBeEnabled();

    fireEvent.click(button);
    await flushMicrotasks();

    const relaunchCalls = mockInvoke.mock.calls.filter(([c]) => c === "relaunch_spawned");
    expect(relaunchCalls).toHaveLength(1);
    expect(relaunchCalls[0][1]).toEqual({ projectDir: "/tmp/test-proj" });
    expect(captured.value).toBe(2);

    // Button is disabled while relaunching and shows the queued count.
    expect(button).toBeDisabled();
    expect(button.textContent).toMatch(/relaunching 2/i);
  });
});

// ── #2 UI debounce blocks the rapid second click ────────────────────────────

describe("PreviousTeamBanner UI debounce", () => {
  it("ignores a second click while button is disabled", async () => {
    const mockInvoke = installInvokeMock({ peek: MANIFEST_MIXED, relaunch: 2 });
    const captured = { value: -1 };

    render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={true}
        onRequestLaunch={autoConfirmOnRequestLaunch(captured)}
      />,
    );
    await flushMicrotasks();
    const button = screen.getByRole("button", { name: /relaunch 2 roles/i });

    fireEvent.click(button);
    await flushMicrotasks();
    fireEvent.click(button); // rapid second click; button is disabled
    await flushMicrotasks();

    const relaunchCalls = mockInvoke.mock.calls.filter(([c]) => c === "relaunch_spawned");
    expect(relaunchCalls).toHaveLength(1);
  });
});

// ── #3 post-window re-enable ────────────────────────────────────────────────

describe("PreviousTeamBanner post-window re-enable", () => {
  it("re-enables the button after debounceWindowMs(queued) and allows a second relaunch", async () => {
    const mockInvoke = installInvokeMock({ peek: MANIFEST_MIXED, relaunch: 2 });
    const captured = { value: -1 };

    render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={true}
        onRequestLaunch={autoConfirmOnRequestLaunch(captured)}
      />,
    );
    await flushMicrotasks();
    const button = screen.getByRole("button", { name: /relaunch 2 roles/i });

    fireEvent.click(button);
    await flushMicrotasks();
    expect(button).toBeDisabled();

    // One tick short of the window — still disabled.
    await act(async () => {
      vi.advanceTimersByTime(debounceWindowMs(2) - 1);
    });
    expect(button).toBeDisabled();

    // Cross the threshold — banner clears relaunching state.
    await act(async () => {
      vi.advanceTimersByTime(2);
    });
    await flushMicrotasks();
    expect(button).toBeEnabled();

    fireEvent.click(button);
    await flushMicrotasks();
    const relaunchCalls = mockInvoke.mock.calls.filter(([c]) => c === "relaunch_spawned");
    expect(relaunchCalls).toHaveLength(2);
  });
});

// ── #4 zero-queued does not lock the button ─────────────────────────────────

describe("PreviousTeamBanner zero-queued edge", () => {
  it("leaves the button enabled when relaunch_spawned returns 0", async () => {
    // ux-engineer:0 msg 185: "if relaunch_spawned returns queued: 0, relaunching
    // is NOT set true and button stays enabled. User can retry without the
    // debounce window locking them out."
    const mockInvoke = installInvokeMock({ peek: MANIFEST_MIXED, relaunch: 0 });
    const captured = { value: -1 };

    render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={true}
        onRequestLaunch={autoConfirmOnRequestLaunch(captured)}
      />,
    );
    await flushMicrotasks();
    const button = screen.getByRole("button", { name: /relaunch 2 roles/i });

    fireEvent.click(button);
    await flushMicrotasks();

    expect(captured.value).toBe(0);
    expect(button).toBeEnabled();

    fireEvent.click(button);
    await flushMicrotasks();
    const relaunchCalls = mockInvoke.mock.calls.filter(([c]) => c === "relaunch_spawned");
    expect(relaunchCalls).toHaveLength(2);
  });
});

// ── #5 error path clears state ──────────────────────────────────────────────

describe("PreviousTeamBanner error path", () => {
  it("clears relaunching state when relaunch_spawned rejects", async () => {
    installInvokeMock({ peek: MANIFEST_MIXED, relaunch: new Error("bad") });
    const captured = { value: -1 };
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});

    render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={true}
        onRequestLaunch={autoConfirmOnRequestLaunch(captured)}
      />,
    );
    await flushMicrotasks();
    const button = screen.getByRole("button", { name: /relaunch 2 roles/i });

    fireEvent.click(button);
    await flushMicrotasks();

    expect(captured.value).toBe(0); // runRelaunch returns 0 on error
    expect(button).toBeEnabled();   // no stuck-disabled
    consoleError.mockRestore();
  });
});

// ── #6 unmount-remount mid-relaunch ─────────────────────────────────────────
// This test documents a gap that UI debounce alone cannot close. Local state
// resets on unmount. A newly-mounted banner has no knowledge that a prior
// instance queued spawns that are still running. Only the Rust-side
// AtomicBool (msg 168/182/194/191) prevents a fresh click from re-queuing
// the same entries. Tester:1 msg 197 raised this; ux-engineer:0 msg 185
// acknowledged the gap and endorsed the AtomicBool.

describe("PreviousTeamBanner unmount-remount gap (motivates Rust AtomicBool)", () => {
  it("a remounted banner does NOT inherit the prior instance's debounce", async () => {
    const mockInvoke = installInvokeMock({ peek: MANIFEST_MIXED, relaunch: 2 });
    const captured = { value: -1 };

    const { unmount } = render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={true}
        onRequestLaunch={autoConfirmOnRequestLaunch(captured)}
      />,
    );
    await flushMicrotasks();
    fireEvent.click(screen.getByRole("button", { name: /relaunch 2 roles/i }));
    await flushMicrotasks();

    // Unmount BEFORE debounceWindowMs(2) elapses. Bg spawn thread in Rust
    // (not modeled here) is still running. Local debounce state dies with
    // the component.
    unmount();

    // Re-mount fresh. New instance sees fresh state — button immediately
    // enabled. If the user clicks again, invoke("relaunch_spawned") fires
    // while the previous queue is still in-flight on the Rust side.
    render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={true}
        onRequestLaunch={autoConfirmOnRequestLaunch(captured)}
      />,
    );
    await flushMicrotasks();
    const button = screen.getByRole("button", { name: /relaunch 2 roles/i });
    expect(button).toBeEnabled();

    fireEvent.click(button);
    await flushMicrotasks();

    // Two invokes total — the gap is real from the UI's POV.
    const relaunchCalls = mockInvoke.mock.calls.filter(([c]) => c === "relaunch_spawned");
    expect(relaunchCalls).toHaveLength(2);
  });
});

// ── parent-cancel path (no invoke fires) ────────────────────────────────────

describe("PreviousTeamBanner cancel from parent", () => {
  it("does NOT call relaunch_spawned when onRequestLaunch never calls execute", async () => {
    const mockInvoke = installInvokeMock({ peek: MANIFEST_MIXED, relaunch: 2 });
    const onRequestLaunch = vi.fn(); // does not call execute

    render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={true}
        onRequestLaunch={onRequestLaunch}
      />,
    );
    await flushMicrotasks();

    fireEvent.click(screen.getByRole("button", { name: /relaunch 2 roles/i }));
    await flushMicrotasks();

    expect(onRequestLaunch).toHaveBeenCalledTimes(1);
    const relaunchCalls = mockInvoke.mock.calls.filter(([c]) => c === "relaunch_spawned");
    expect(relaunchCalls).toHaveLength(0);
  });
});

// ── hidden states ───────────────────────────────────────────────────────────

describe("PreviousTeamBanner hidden states", () => {
  it("renders nothing when projectDir is null", async () => {
    installInvokeMock({ peek: MANIFEST_MIXED });
    const { container } = render(
      <PreviousTeamBanner
        projectDir={null}
        claudeInstalled={true}
        onRequestLaunch={vi.fn()}
      />,
    );
    await flushMicrotasks();
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when deadCount is zero (all alive)", async () => {
    const allAlive: PreviousTeamEntry[] = [
      { role: "developer", instance: 0, pid: 1000, spawned_at: "2026-04-18T20:00:00Z", alive: true },
    ];
    installInvokeMock({ peek: allAlive });
    const { container } = render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={true}
        onRequestLaunch={vi.fn()}
      />,
    );
    await flushMicrotasks();
    expect(container.firstChild).toBeNull();
  });

  it("disables the relaunch button (but still renders banner) when Claude CLI missing", async () => {
    installInvokeMock({ peek: MANIFEST_MIXED });
    render(
      <PreviousTeamBanner
        projectDir="/tmp/test-proj"
        claudeInstalled={false}
        onRequestLaunch={vi.fn()}
      />,
    );
    await flushMicrotasks();
    const button = screen.getByRole("button", { name: /claude cli not installed/i });
    expect(button).toBeDisabled();
  });
});
