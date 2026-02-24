import { useEffect, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import { useProjectStore, useMessageStore } from "../lib/stores";
import type { BoardMessage } from "../lib/api";

const ROLE_COLORS: Record<string, string> = {
  manager: "var(--role-manager)",
  architect: "var(--role-architect)",
  developer: "var(--role-developer)",
  tester: "var(--role-tester)",
};

function getRoleColor(slug: string): string {
  const base = slug.split(":")[0];
  return ROLE_COLORS[base] || "var(--text-muted)";
}

const PROVIDER_MODELS: Record<string, Array<{ id: string; label: string }>> = {
  anthropic: [
    { id: "claude-opus-4-6", label: "Claude Opus 4.6" },
    { id: "claude-sonnet-4-6", label: "Claude Sonnet 4.6" },
    { id: "claude-haiku-4-5-20251001", label: "Claude Haiku 4.5" },
  ],
  openai: [
    { id: "gpt-4o", label: "GPT-4o" },
    { id: "gpt-4o-mini", label: "GPT-4o Mini" },
    { id: "o3", label: "o3" },
  ],
  google: [
    { id: "gemini-2.0-flash", label: "Gemini 2.0 Flash" },
    { id: "gemini-2.0-pro", label: "Gemini 2.0 Pro" },
  ],
};

function MessageCard({ msg }: { msg: BoardMessage }) {
  const fromRole = msg.from.split(":")[0];
  return (
    <div
      className="card"
      style={{
        borderLeft: `3px solid ${getRoleColor(fromRole)}`,
        padding: "var(--space-3)",
        marginBottom: "var(--space-2)",
      }}
      role="article"
      aria-label={`Message from ${msg.from}: ${msg.subject}`}
    >
      <div style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-2)",
        marginBottom: "var(--space-1)",
        fontSize: "var(--text-sm)",
      }}>
        <span style={{ color: getRoleColor(fromRole), fontWeight: "var(--weight-semibold)" }}>
          {msg.from}
        </span>
        <span style={{ color: "var(--text-muted)" }}>{"\u2192"}</span>
        <span style={{ color: "var(--text-secondary)" }}>{msg.to}</span>
        <span className={`badge badge-${msg.type === "directive" ? "error" : msg.type === "approval" ? "success" : "accent"}`}>
          {msg.type}
        </span>
        <span style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)", marginLeft: "auto" }}>
          {formatTime(msg.timestamp)}
        </span>
      </div>
      {msg.subject && (
        <div style={{ fontWeight: "var(--weight-medium)", marginBottom: "var(--space-1)" }}>
          {msg.subject}
        </div>
      )}
      <div style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", whiteSpace: "pre-wrap" }}>
        {msg.body}
      </div>
    </div>
  );
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    const diff = Math.floor((Date.now() - d.getTime()) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return d.toLocaleDateString();
  } catch {
    return iso;
  }
}

export function ProjectPage() {
  const { projectId } = useParams<{ projectId: string }>();
  const project = useProjectStore((s) => s.activeProject);
  const selectProject = useProjectStore((s) => s.selectProject);
  const updateRoleProvider = useProjectStore((s) => s.updateRoleProvider);
  const startAgent = useProjectStore((s) => s.startAgent);
  const projectLoading = useProjectStore((s) => s.loading);

  const messages = useMessageStore((s) => s.messages);
  const loadMessages = useMessageStore((s) => s.loadMessages);
  const connectWs = useMessageStore((s) => s.connectWs);
  const disconnectWs = useMessageStore((s) => s.disconnectWs);
  const connected = useMessageStore((s) => s.connected);
  const sendMsg = useMessageStore((s) => s.sendMessage);

  const [msgTo, setMsgTo] = useState("all");
  const [msgBody, setMsgBody] = useState("");
  const [sending, setSending] = useState(false);

  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (projectId) {
      selectProject(projectId);
      loadMessages(projectId);
      connectWs(projectId);
    }
    return () => disconnectWs();
    // Zustand selectors return stable references, so only projectId triggers re-runs
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  // Auto-scroll on new messages
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length]);

  const handleSend = async () => {
    if (!projectId || !msgBody.trim() || sending) return;
    setSending(true);
    await sendMsg(projectId, msgTo, "directive", "", msgBody.trim());
    setMsgBody("");
    setSending(false);
  };

  if (projectLoading && !project) {
    return (
      <div className="loading-overlay" role="status">
        <div className="spinner" />
        <span>Loading project...</span>
      </div>
    );
  }

  if (!project) {
    return (
      <div className="empty-state">
        <div className="empty-state-icon">{"\u2753"}</div>
        <div className="empty-state-title">Project not found</div>
        <div className="empty-state-desc">This project may have been deleted or you don't have access.</div>
      </div>
    );
  }

  const roles = Object.entries(project.roles);

  return (
    <>
      <div className="page-header">
        <div>
          <h1 style={{ fontSize: "var(--text-xl)", fontWeight: "var(--weight-bold)" }}>{project.name}</h1>
          <div style={{ display: "flex", alignItems: "center", gap: "var(--space-2)", marginTop: "var(--space-1)" }}>
            <div className={`status-dot ${connected ? "working" : "vacant"}`} />
            <span style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>
              {connected ? "Connected" : "Disconnected"}
            </span>
            <span style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>
              {"\u00B7"} {roles.length} roles {"\u00B7"} {messages.length} messages
            </span>
          </div>
        </div>
      </div>

      <div className="page-body" style={{ display: "flex", gap: "var(--space-4)", height: "100%" }}>
        {/* Left: Team roster + provider config */}
        <div style={{ width: 300, flexShrink: 0, overflowY: "auto" }}>
          <h2 style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)", marginBottom: "var(--space-3)" }}>
            Team
          </h2>

          {roles.length === 0 ? (
            <div className="empty-state" style={{ padding: "var(--space-6) var(--space-4)" }}>
              <div className="empty-state-title" style={{ fontSize: "var(--text-sm)" }}>No roles configured</div>
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
              {roles.map(([slug, role]) => (
                <div
                  key={slug}
                  className="card"
                  style={{ borderLeft: `3px solid ${getRoleColor(slug)}`, padding: "var(--space-3)" }}
                >
                  <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                    <span style={{ fontWeight: "var(--weight-medium)", fontSize: "var(--text-sm)" }}>
                      {role.title}
                    </span>
                    <button
                      className="btn btn-ghost"
                      style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)" }}
                      onClick={() => startAgent(slug)}
                      aria-label={`Start ${role.title} agent`}
                    >
                      Start
                    </button>
                  </div>

                  {/* Provider selector */}
                  <div style={{ marginTop: "var(--space-2)", display: "flex", gap: "var(--space-1)" }}>
                    <select
                      className="input"
                      style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)" }}
                      value={role.provider?.provider || "anthropic"}
                      onChange={(e) => {
                        const provider = e.target.value;
                        const models = PROVIDER_MODELS[provider];
                        const model = models?.[0]?.id || "";
                        updateRoleProvider(slug, provider, model);
                      }}
                      aria-label={`Provider for ${role.title}`}
                    >
                      <option value="anthropic">Claude</option>
                      <option value="openai">GPT</option>
                      <option value="google">Gemini</option>
                    </select>
                    <select
                      className="input"
                      style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)" }}
                      value={role.provider?.model || ""}
                      onChange={(e) => {
                        updateRoleProvider(slug, role.provider?.provider || "anthropic", e.target.value);
                      }}
                      aria-label={`Model for ${role.title}`}
                    >
                      {(PROVIDER_MODELS[role.provider?.provider || "anthropic"] || []).map((m) => (
                        <option key={m.id} value={m.id}>{m.label}</option>
                      ))}
                    </select>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Right: Message board */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
          <h2 style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)", marginBottom: "var(--space-3)" }}>
            Messages
          </h2>

          <div
            style={{ flex: 1, overflowY: "auto", marginBottom: "var(--space-3)" }}
            role="log"
            aria-label="Message board"
            aria-live="polite"
          >
            {messages.length === 0 ? (
              <div className="empty-state" style={{ padding: "var(--space-8) var(--space-4)" }}>
                <div className="empty-state-icon">{"\uD83D\uDCAC"}</div>
                <div className="empty-state-title">No messages yet</div>
                <div className="empty-state-desc">
                  Start the agents and send a directive to kick things off.
                </div>
              </div>
            ) : (
              messages.map((msg) => <MessageCard key={msg.id} msg={msg} />)
            )}
            <div ref={messagesEndRef} />
          </div>

          {/* Compose */}
          <div style={{
            display: "flex",
            gap: "var(--space-2)",
            padding: "var(--space-3)",
            background: "var(--bg-secondary)",
            borderRadius: "var(--radius-md)",
            border: "1px solid var(--border)",
          }}>
            <select
              className="input"
              style={{ width: 120, flexShrink: 0 }}
              value={msgTo}
              onChange={(e) => setMsgTo(e.target.value)}
              aria-label="Send to"
            >
              <option value="all">Everyone</option>
              {roles.map(([slug, role]) => (
                <option key={slug} value={slug}>{role.title}</option>
              ))}
            </select>
            <input
              className="input"
              type="text"
              value={msgBody}
              onChange={(e) => setMsgBody(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
              placeholder="Type a message..."
              disabled={sending}
              aria-label="Message text"
            />
            <button
              className="btn btn-primary"
              onClick={handleSend}
              disabled={sending || !msgBody.trim()}
              aria-label="Send message"
            >
              {sending ? <div className="spinner" style={{ width: 14, height: 14 }} /> : "Send"}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
