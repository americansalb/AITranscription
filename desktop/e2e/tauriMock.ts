// Browser-mode Tauri IPC mock for ui2 e2e (register item ②, msg 360/361).
// Injected via addInitScript BEFORE any app code: it defines
// window.__TAURI_INTERNALS__ — the layer @tauri-apps/api/core delegates to —
// so the production import graph (store → invoke/listen) runs unmodified and
// only the final IPC hop is fake. Recorded limitation: the real Rust commands
// and WebView2-specific paint are NOT exercised here.
//
// MOCK-ROT WARNING (review msg 365): the ParsedProject fixture below is
// hand-maintained against the Rust source of truth — collab.rs:542
// (ParsedProject) and the BoardMessage/SessionBinding/RoleStatus structs
// around it. If the engine contract evolves, update this shape or the suite
// proves nothing. Generator-fixture idea tracked in src/ui2/LATER.md.

export interface MockWindow {
  __UI2_SENT: Array<{ cmd: string; args: Record<string, unknown> }>;
}

export function tauriMockSource(messageCount: number): string {
  return `
(() => {
  const N = ${messageCount};
  const sent = [];
  window.__UI2_SENT = sent;

  function pad(n) { return String(n).padStart(2, "0"); }
  function tsFor(i) {
    const h = 8 + Math.floor(i / 3600) % 12;
    return "2026-06-09T" + pad(h) + ":" + pad(Math.floor(i / 60) % 60) + ":" + pad(i % 60) + "Z";
  }

  const messages = [];
  for (let i = 1; i <= N; i++) {
    const base = { id: i, subject: "msg " + i, body: "body ".repeat(16) + i, timestamp: tsFor(i), metadata: {} };
    if (i === N - 1) {
      messages.push({ ...base, from: "code-interpreter:0", to: "human:0", type: "question",
        subject: "Phase gate decision", body: "Approve the phase?",
        metadata: { choices: [{ id: "a", label: "Approve and continue" }] } });
    } else if (i % 10 === 1) {
      messages.push({ ...base, from: "code-interpreter:0", to: "all", type: "status" });
    } else if (i % 10 === 0) {
      messages.push({ ...base, from: "human:0", to: "all", type: "directive" });
    } else {
      messages.push({ ...base, from: "developer:0", to: "all", type: "status" });
    }
  }

  const project = {
    config: { project_id: "e2e", name: "E2EProject", description: "", created_at: tsFor(0),
      updated_at: tsFor(0), roles: {}, settings: { heartbeat_timeout_seconds: 60, message_retention_days: 30 } },
    sessions: [{ role: "code-interpreter", instance: 0, session_id: "s1", claimed_at: tsFor(0),
      last_heartbeat: new Date().toISOString(), status: "active", last_working_at: new Date().toISOString() }],
    messages,
    role_statuses: [
      { slug: "code-interpreter", title: "Code Translator", active_instances: 1, max_instances: 1, status: "active" },
      { slug: "developer", title: "Developer", active_instances: 0, max_instances: 3, status: "vacant" },
    ],
    claims: [],
  };

  let nextId = N + 1;
  let callbackId = 1;

  window.__TAURI_INTERNALS__ = {
    transformCallback: function (cb) {
      const id = callbackId++;
      window["_" + id] = cb;
      return id;
    },
    invoke: function (cmd, args) {
      args = args || {};
      if (cmd === "watch_project_dir") return Promise.resolve(project);
      if (cmd === "send_team_message") {
        sent.push({ cmd, args });
        // UI-originated sends are the human's; tests may impersonate an
        // agent via metadata.__mock_from to exercise non-human rules
        const from = (args.metadata && args.metadata.__mock_from) || "human:0";
        project.messages = project.messages.concat([{
          id: nextId++, from,
          to: args.to, type: args.msg_type || "directive", timestamp: new Date().toISOString(),
          subject: args.subject || "", body: args.body || "", metadata: args.metadata || {},
        }]);
        return Promise.resolve(null);
      }
      if (cmd === "plugin:event|listen") return Promise.resolve(callbackId);
      if (cmd === "plugin:event|unlisten") return Promise.resolve(null);
      return Promise.resolve(null);
    },
  };

  try { localStorage.setItem("vaak_collab_project_dir", JSON.stringify("C:\\\\e2e\\\\fake")); } catch (e) {}
})();
`;
}
