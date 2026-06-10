// §7 smoke path: launch → see feed → open card → choose option → mute.
// Tauri APIs mocked; this verifies the wired surface, not the webview.
// (True Playwright-in-Tauri run remains a LATER.md item.)
import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ParsedProject } from "../store/types";

const invokeCalls: Array<{ cmd: string; args: Record<string, unknown> }> = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown>) => {
    invokeCalls.push({ cmd, args });
    if (cmd === "watch_project_dir") return fixture();
    return null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

function fixture(): ParsedProject {
  const now = new Date().toISOString();
  return {
    config: {
      project_id: "p1",
      name: "SmokeProject",
      description: "",
      created_at: now,
      updated_at: now,
      roles: {},
      settings: { heartbeat_timeout_seconds: 60, message_retention_days: 30 },
    },
    sessions: [
      {
        role: "code-interpreter",
        instance: 0,
        session_id: "s1",
        claimed_at: now,
        last_heartbeat: now,
        status: "active",
        last_working_at: now,
      },
    ],
    messages: [
      {
        id: 1,
        from: "code-interpreter:0",
        to: "all",
        type: "status",
        timestamp: now,
        subject: "Relay update",
        body: "the relay speaks",
        metadata: {},
      },
      {
        id: 2,
        from: "code-interpreter:0",
        to: "human:0",
        type: "question",
        timestamp: now,
        subject: "Pick one",
        body: "decision body",
        metadata: { choices: [{ id: "a", label: "Run kill-test with bug A" }] },
      },
      {
        id: 3,
        from: "developer:0",
        to: "all",
        type: "status",
        timestamp: now,
        subject: "noise",
        body: "engine noise",
        metadata: {},
      },
    ],
    role_statuses: [
      { slug: "code-interpreter", title: "Code Translator", active_instances: 1, max_instances: 1, status: "active" },
      { slug: "developer", title: "Developer", active_instances: 0, max_instances: 3, status: "vacant" },
    ],
    claims: [],
  };
}

describe("ui2 smoke — launch → feed → card → choose → mute", () => {
  beforeEach(async () => {
    cleanup();
    invokeCalls.length = 0;
    localStorage.setItem("vaak_collab_project_dir", JSON.stringify("C:\\fake\\project"));
    const { useUi2Store } = await import("../store/store");
    useUi2Store.setState({
      projectDir: null,
      project: null,
      mutedAtId: null,
      expandedRows: new Set(),
      engineRoomOpen: false,
    });
  });

  it("walks the five-step path", async () => {
    const { default: Ui2App } = await import("../Ui2App");
    const user = userEvent.setup();
    render(<Ui2App />);

    // 1. launch: connects from the bootstrap hint and renders the shell
    await waitFor(() => expect(screen.getByText("SmokeProject")).toBeTruthy());

    // 2. see feed: relay post expanded; engine noise collapsed to a digest
    await waitFor(() => expect(screen.getByText("the relay speaks")).toBeTruthy());
    expect(screen.queryByText("engine noise")).toBeNull();
    expect(screen.getByText(/1 engine events/)).toBeTruthy();

    // liveness: working dot for relay, vacancy summarized — zombie rule holds
    expect(screen.getByLabelText(/Code Translator .* working/)).toBeTruthy();

    // 3. open card: active in the dock with its option rendered as a button
    const option = await screen.findByRole("button", { name: "Run kill-test with bug A" });

    // 4. choose option: resolution goes to the board with in_reply_to + choice_id
    await user.click(option);
    await waitFor(() => {
      const sent = invokeCalls.find((c) => c.cmd === "send_team_message");
      expect(sent).toBeTruthy();
      expect(sent?.args.metadata).toMatchObject({ in_reply_to: 2, choice_id: "a" });
    });

    // 5. mute: pressed state + standing directive posted to the board
    const mute = screen.getByRole("button", { name: "Mute all" });
    await user.click(mute);
    expect(screen.getByRole("button", { name: "Unmute room" })).toBeTruthy();
    await waitFor(() => {
      const sends = invokeCalls.filter((c) => c.cmd === "send_team_message");
      const directive = sends[sends.length - 1];
      expect(directive?.args.body).toMatch(/muted the room/);
    });
  });
});
