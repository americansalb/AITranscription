import { useEffect, useRef, useState, useCallback } from "react";
import { useParams } from "react-router-dom";
import { useProjectStore, useMessageStore } from "../lib/stores";
import { getModelCatalog, getActiveDiscussion, deleteMessage, buzzAgent, interruptAgent, listSections, createSection, switchSection } from "../lib/api";
import type { BoardMessage, DiscussionResponse, SectionInfo } from "../lib/api";
import { useUIStore } from "../lib/stores";
import { DiscussionPanel } from "../components/DiscussionPanel";
import { RoleBriefingModal } from "../components/RoleBriefingModal";
import { FileClaimsPanel } from "../components/FileClaimsPanel";

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

// Fallback models if backend is unreachable
const FALLBACK_MODELS: Record<string, Array<{ id: string; label: string }>> = {
  anthropic: [
    { id: "claude-sonnet-4-6", label: "Claude Sonnet 4.6" },
  ],
  openai: [
    { id: "gpt-4o", label: "GPT-4o" },
  ],
  google: [
    { id: "gemini-2.0-flash", label: "Gemini 2.0 Flash" },
  ],
};

function MessageCard({ msg, onDelete }: { msg: BoardMessage; onDelete: (id: number) => void }) {
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
        <button
          className="btn btn-ghost"
          style={{ fontSize: "var(--text-xs)", padding: "0 var(--space-1)", color: "var(--text-muted)" }}
          onClick={() => onDelete(msg.id)}
          aria-label={`Delete message from ${msg.from}`}
          title="Delete message"
        >
          {"\u2715"}
        </button>
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
  const [providerModels, setProviderModels] = useState<Record<string, Array<{ id: string; label: string }>>>(FALLBACK_MODELS);
  const [discussion, setDiscussion] = useState<DiscussionResponse | null>(null);
  const [sections, setSections] = useState<SectionInfo[]>([]);
  const [activeSection, setActiveSection] = useState<string | null>(null);
  const [newSectionName, setNewSectionName] = useState("");
  const [showSections, setShowSections] = useState(false);
  const addToast = useUIStore((s) => s.addToast);
  const [briefingRole, setBriefingRole] = useState<{ slug: string; title: string } | null>(null);
  const [visibleMsgCount, setVisibleMsgCount] = useState(50);
  const [interruptTarget, setInterruptTarget] = useState<{ slug: string; title: string } | null>(null);
  const [interruptReason, setInterruptReason] = useState("");

  // Persist compose draft to localStorage
  const draftKey = projectId ? `vaak_draft_${projectId}` : null;
  const [msgSubject, setMsgSubject] = useState("");

  const messagesEndRef = useRef<HTMLDivElement>(null);

  const refreshDiscussion = useCallback(() => {
    if (!projectId) return;
    getActiveDiscussion(projectId)
      .then(setDiscussion)
      .catch(() => setDiscussion(null));
  }, [projectId]);

  const handleDeleteMessage = useCallback(async (messageId: number) => {
    if (!projectId) return;
    try {
      await deleteMessage(projectId, messageId);
      // Remove from local state immediately
      useMessageStore.setState((s) => ({
        messages: s.messages.filter((m) => m.id !== messageId),
      }));
    } catch (e) {
      addToast("Failed to delete message", "error");
    }
  }, [projectId, addToast]);

  const handleBuzz = useCallback(async (roleSlug: string) => {
    if (!projectId) return;
    try {
      await buzzAgent(projectId, roleSlug);
      addToast(`Buzzed ${roleSlug}`, "success");
    } catch (e) {
      addToast("Failed to buzz agent", "error");
    }
  }, [projectId, addToast]);

  const handleInterrupt = useCallback(async () => {
    if (!projectId || !interruptTarget || !interruptReason.trim()) return;
    try {
      await interruptAgent(projectId, interruptTarget.slug, interruptReason.trim());
      addToast(`Interrupted ${interruptTarget.title}`, "success");
      setInterruptTarget(null);
      setInterruptReason("");
    } catch {
      addToast("Failed to send interrupt", "error");
    }
  }, [projectId, interruptTarget, interruptReason, addToast]);

  const loadSections = useCallback(async () => {
    if (!projectId) return;
    try {
      const secs = await listSections(projectId);
      setSections(secs);
    } catch {
      // Sections may not be supported yet
    }
  }, [projectId]);

  const handleCreateSection = useCallback(async () => {
    if (!projectId || !newSectionName.trim()) return;
    try {
      const sec = await createSection(projectId, newSectionName.trim());
      setSections((prev) => [...prev, sec]);
      setNewSectionName("");
      addToast(`Section "${sec.name}" created`, "success");
    } catch (e) {
      addToast("Failed to create section", "error");
    }
  }, [projectId, newSectionName, addToast]);

  const handleSwitchSection = useCallback(async (slug: string) => {
    if (!projectId) return;
    try {
      await switchSection(projectId, slug);
      setActiveSection(slug);
      // Reload messages for the new section
      loadMessages(projectId);
      addToast(`Switched to section: ${slug}`, "info");
    } catch (e) {
      addToast("Failed to switch section", "error");
    }
  }, [projectId, loadMessages, addToast]);

  // Restore draft from localStorage
  useEffect(() => {
    if (draftKey) {
      const saved = localStorage.getItem(draftKey);
      if (saved) setMsgBody(saved);
    }
  }, [draftKey]);

  // Save draft to localStorage on change
  useEffect(() => {
    if (draftKey && msgBody) {
      localStorage.setItem(draftKey, msgBody);
    } else if (draftKey) {
      localStorage.removeItem(draftKey);
    }
  }, [draftKey, msgBody]);

  // Fetch model catalog from backend (single source of truth)
  useEffect(() => {
    getModelCatalog()
      .then((catalog) => {
        const grouped: Record<string, Array<{ id: string; label: string }>> = {};
        for (const m of catalog.models) {
          if (!grouped[m.provider]) grouped[m.provider] = [];
          grouped[m.provider].push({ id: m.id, label: m.name });
        }
        setProviderModels(grouped);
      })
      .catch((e) => console.error("[ProjectPage] Failed to fetch model catalog:", e));
  }, []);

  useEffect(() => {
    if (projectId) {
      selectProject(projectId);
      loadMessages(projectId);
      connectWs(projectId);
      refreshDiscussion();
      loadSections();
    }
    return () => disconnectWs();
    // Zustand selectors return stable references, so only projectId triggers re-runs
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  // Auto-scroll on new messages + refresh discussion state on new messages
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    // Discussion events arrive as WS messages — refresh discussion state
    refreshDiscussion();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [messages.length]);

  const handleSend = async () => {
    if (!projectId || !msgBody.trim() || sending) return;
    setSending(true);
    await sendMsg(projectId, msgTo, "directive", msgSubject.trim(), msgBody.trim());
    setMsgBody("");
    setMsgSubject("");
    if (draftKey) localStorage.removeItem(draftKey);
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

          <FileClaimsPanel projectId={projectId!} />

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
                    <button
                      className="btn btn-ghost"
                      style={{ fontWeight: "var(--weight-medium)", fontSize: "var(--text-sm)", padding: 0, textDecoration: "underline dotted" }}
                      onClick={() => setBriefingRole({ slug, title: role.title })}
                      aria-label={`View ${role.title} briefing`}
                      title="View briefing"
                    >
                      {role.title}
                    </button>
                    <div style={{ display: "flex", gap: "var(--space-1)" }}>
                      <button
                        className="btn btn-ghost"
                        style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-1)" }}
                        onClick={() => handleBuzz(slug)}
                        aria-label={`Buzz ${role.title}`}
                        title="Send wake-up signal"
                      >
                        {"\uD83D\uDCE2"}
                      </button>
                      <button
                        className="btn btn-ghost"
                        style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)" }}
                        onClick={() => startAgent(slug)}
                        aria-label={`Start ${role.title} agent`}
                      >
                        Start
                      </button>
                    </div>
                  </div>

                  {/* Provider selector */}
                  <div style={{ marginTop: "var(--space-2)", display: "flex", gap: "var(--space-1)" }}>
                    <select
                      className="input"
                      style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)" }}
                      value={role.provider?.provider || "anthropic"}
                      onChange={(e) => {
                        const provider = e.target.value;
                        const models = providerModels[provider];
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
                      {(providerModels[role.provider?.provider || "anthropic"] || []).map((m) => (
                        <option key={m.id} value={m.id}>{m.label}</option>
                      ))}
                    </select>
                  </div>
                </div>
              ))}
            </div>
          )}

          {/* Discussion panel below team roster */}
          <DiscussionPanel
            projectId={projectId!}
            discussion={discussion}
            onRefresh={refreshDiscussion}
          />
        </div>

        {/* Right: Message board */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: "var(--space-3)" }}>
            <h2 style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)" }}>
              Messages
              {activeSection && (
                <span style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)", marginLeft: "var(--space-2)" }}>
                  ({activeSection})
                </span>
              )}
            </h2>
            <button
              className="btn btn-ghost"
              style={{ fontSize: "var(--text-xs)" }}
              onClick={() => setShowSections(!showSections)}
              aria-label="Toggle sections"
            >
              Sections {showSections ? "\u25B2" : "\u25BC"}
            </button>
          </div>

          {showSections && (
            <div style={{
              display: "flex",
              flexWrap: "wrap",
              gap: "var(--space-1)",
              marginBottom: "var(--space-2)",
              padding: "var(--space-2)",
              background: "var(--bg-tertiary)",
              borderRadius: "var(--radius-sm)",
            }}>
              {sections.map((sec) => (
                <button
                  key={sec.slug}
                  className={`badge ${sec.slug === activeSection ? "badge-accent" : ""}`}
                  style={{ cursor: "pointer", padding: "var(--space-1) var(--space-2)" }}
                  onClick={() => handleSwitchSection(sec.slug)}
                  aria-pressed={sec.slug === activeSection}
                >
                  {sec.name} ({sec.message_count})
                </button>
              ))}
              <div style={{ display: "flex", gap: "var(--space-1)", marginLeft: "var(--space-2)" }}>
                <input
                  className="input"
                  style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)", width: 120 }}
                  value={newSectionName}
                  onChange={(e) => setNewSectionName(e.target.value)}
                  placeholder="New section..."
                  onKeyDown={(e) => { if (e.key === "Enter") handleCreateSection(); }}
                  aria-label="New section name"
                />
                <button
                  className="btn btn-ghost"
                  style={{ fontSize: "var(--text-xs)" }}
                  onClick={handleCreateSection}
                  disabled={!newSectionName.trim()}
                >
                  +
                </button>
              </div>
            </div>
          )}

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
              <>
                {messages.length > visibleMsgCount && (
                  <button
                    className="btn btn-ghost"
                    style={{ width: "100%", fontSize: "var(--text-xs)", marginBottom: "var(--space-2)" }}
                    onClick={() => setVisibleMsgCount((c) => c + 50)}
                    aria-label={`Load more messages (showing ${visibleMsgCount} of ${messages.length})`}
                  >
                    Load more ({messages.length - visibleMsgCount} hidden)
                  </button>
                )}
                {messages.slice(-visibleMsgCount).map((msg) => (
                  <MessageCard key={msg.id} msg={msg} onDelete={handleDeleteMessage} />
                ))}
              </>
            )}
            <div ref={messagesEndRef} />
          </div>

          {/* Compose */}
          <div style={{
            padding: "var(--space-3)",
            background: "var(--bg-secondary)",
            borderRadius: "var(--radius-md)",
            border: "1px solid var(--border)",
          }}>
            <div style={{ display: "flex", gap: "var(--space-2)", marginBottom: "var(--space-2)" }}>
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
                value={msgSubject}
                onChange={(e) => setMsgSubject(e.target.value)}
                placeholder="Subject (optional)"
                disabled={sending}
                aria-label="Message subject"
                style={{ flex: 1 }}
              />
            </div>
            <div style={{ display: "flex", gap: "var(--space-2)" }}>
              <input
                className="input"
                type="text"
                value={msgBody}
                onChange={(e) => setMsgBody(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
                placeholder="Type a message..."
                disabled={sending}
                aria-label="Message text"
                style={{ flex: 1 }}
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
      </div>

      {/* Role briefing modal */}
      {briefingRole && (
        <RoleBriefingModal
          projectId={projectId!}
          roleSlug={briefingRole.slug}
          roleTitle={briefingRole.title}
          onClose={() => setBriefingRole(null)}
        />
      )}
    </>
  );
}
