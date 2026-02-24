import { useEffect, useRef, useState, useCallback } from "react";
import { useParams } from "react-router-dom";
import { useProjectStore, useMessageStore } from "../lib/stores";
import { getModelCatalog, getActiveDiscussion, deleteMessage, buzzAgent, interruptAgent, listSections, createSection, switchSection, deleteRole } from "../lib/api";
import type { BoardMessage, DiscussionResponse, SectionInfo } from "../lib/api";
import { useUIStore } from "../lib/stores";
import { DiscussionPanel } from "../components/DiscussionPanel";
import { RoleBriefingModal } from "../components/RoleBriefingModal";
import { FileClaimsPanel } from "../components/FileClaimsPanel";
import { AddRoleModal } from "../components/AddRoleModal";
import { ConfirmDialog } from "../components/ConfirmDialog";

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

function MessageCard({
  msg,
  onDelete,
  onQuickReply,
}: {
  msg: BoardMessage;
  onDelete: (id: number) => void;
  onQuickReply?: (to: string, type: string, subject: string, body: string) => void;
}) {
  const fromRole = msg.from.split(":")[0];
  const isReviewType = msg.type === "review" || msg.type === "handoff";
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
        <span className={`badge badge-${msg.type === "directive" ? "error" : msg.type === "approval" ? "success" : msg.type === "revision" ? "warning" : "accent"}`}>
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
      {/* Quick-action voting for review/handoff messages */}
      {isReviewType && onQuickReply && (
        <div style={{ display: "flex", gap: "var(--space-1)", marginTop: "var(--space-2)" }}>
          <button
            className="btn btn-ghost"
            style={{ fontSize: "var(--text-xs)", color: "var(--success)", padding: "2px var(--space-2)" }}
            onClick={() => onQuickReply(msg.from, "approval", `Re: ${msg.subject || "review"}`, "Approved.")}
            aria-label="Approve"
          >
            {"\u2713"} Approve
          </button>
          <button
            className="btn btn-ghost"
            style={{ fontSize: "var(--text-xs)", color: "var(--warning)", padding: "2px var(--space-2)" }}
            onClick={() => onQuickReply(msg.from, "revision", `Re: ${msg.subject || "review"}`, "Needs revision.")}
            aria-label="Request revision"
          >
            {"\u21BA"} Revise
          </button>
          <button
            className="btn btn-ghost"
            style={{ fontSize: "var(--text-xs)", color: "var(--error)", padding: "2px var(--space-2)" }}
            onClick={() => onQuickReply(msg.from, "revision", `Re: ${msg.subject || "review"}`, "Rejected — see below.")}
            aria-label="Reject"
          >
            {"\u2717"} Reject
          </button>
        </div>
      )}
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
  const stopAgent = useProjectStore((s) => s.stopAgent);
  const projectLoading = useProjectStore((s) => s.loading);

  const messages = useMessageStore((s) => s.messages);
  const loadMessages = useMessageStore((s) => s.loadMessages);
  const connectWs = useMessageStore((s) => s.connectWs);
  const disconnectWs = useMessageStore((s) => s.disconnectWs);
  const connected = useMessageStore((s) => s.connected);
  const sendMsg = useMessageStore((s) => s.sendMessage);

  const [msgTo, setMsgTo] = useState("all");
  const [msgType, setMsgType] = useState("directive");
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
  const [showAddRole, setShowAddRole] = useState(false);
  const [deleteRoleTarget, setDeleteRoleTarget] = useState<{ slug: string; title: string } | null>(null);
  const [msgFilter, setMsgFilter] = useState("");
  const [rosterView, setRosterView] = useState<"cards" | "compact">(() =>
    (localStorage.getItem("vaak_roster_view") as "cards" | "compact") || "cards"
  );

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

  const handleQuickReply = useCallback(async (to: string, type: string, subject: string, body: string) => {
    if (!projectId) return;
    await sendMsg(projectId, to, type, subject, body);
  }, [projectId, sendMsg]);

  const handleDeleteRole = useCallback(async () => {
    if (!projectId || !deleteRoleTarget) return;
    try {
      await deleteRole(projectId, deleteRoleTarget.slug);
      addToast(`Deleted role: ${deleteRoleTarget.title}`, "success");
      setDeleteRoleTarget(null);
      selectProject(projectId);
    } catch {
      addToast("Failed to delete role", "error");
    }
  }, [projectId, deleteRoleTarget, addToast, selectProject]);

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

  // Auto-scroll on new messages + debounced discussion refresh (fixes C2: API spam)
  const discussionRefreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    // Debounce discussion refresh to max once per 2 seconds
    if (discussionRefreshTimer.current) clearTimeout(discussionRefreshTimer.current);
    discussionRefreshTimer.current = setTimeout(() => {
      refreshDiscussion();
    }, 2000);
    return () => {
      if (discussionRefreshTimer.current) clearTimeout(discussionRefreshTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [messages.length]);

  const handleSend = async () => {
    if (!projectId || !msgBody.trim() || sending) return;
    setSending(true);
    await sendMsg(projectId, msgTo, msgType, msgSubject.trim(), msgBody.trim());
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

  const filterLower = msgFilter.toLowerCase();
  const filteredMessages = filterLower
    ? messages.filter((m) =>
        m.from.toLowerCase().includes(filterLower) ||
        m.to.toLowerCase().includes(filterLower) ||
        m.type.toLowerCase().includes(filterLower) ||
        m.subject.toLowerCase().includes(filterLower) ||
        m.body.toLowerCase().includes(filterLower)
      )
    : messages;

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
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: "var(--space-3)" }}>
            <h2 style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)" }}>
              Team
            </h2>
            <div style={{ display: "flex", gap: "var(--space-1)" }}>
              {roles.length > 0 && (
                <>
                  <button
                    className="btn btn-ghost"
                    style={{ fontSize: "var(--text-xs)" }}
                    onClick={async () => {
                      for (const [slug] of roles) {
                        await startAgent(slug);
                      }
                      addToast(`Started ${roles.length} agents`, "success");
                    }}
                    aria-label="Start all agents"
                    title="Start all agents"
                  >
                    Start All
                  </button>
                  <button
                    className="btn btn-ghost"
                    style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}
                    onClick={async () => {
                      for (const [slug] of roles) {
                        await stopAgent(slug);
                      }
                      addToast(`Stopped ${roles.length} agents`, "info");
                    }}
                    aria-label="Stop all agents"
                    title="Stop all agents"
                  >
                    Stop All
                  </button>
                </>
              )}
              <button
                className="btn btn-ghost"
                style={{ fontSize: "var(--text-xs)" }}
                onClick={() => setShowAddRole(true)}
                aria-label="Add role"
              >
                + Role
              </button>
              <button
                className="btn btn-ghost"
                style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-1)" }}
                onClick={() => {
                  const next = rosterView === "cards" ? "compact" : "cards";
                  setRosterView(next);
                  localStorage.setItem("vaak_roster_view", next);
                }}
                aria-label={`Switch to ${rosterView === "cards" ? "compact" : "card"} view`}
                title={rosterView === "cards" ? "Compact view" : "Card view"}
              >
                {rosterView === "cards" ? "\u2630" : "\u2BC1"}
              </button>
            </div>
          </div>

          <FileClaimsPanel projectId={projectId!} />

          {roles.length === 0 ? (
            <div className="empty-state" style={{ padding: "var(--space-6) var(--space-4)" }}>
              <div className="empty-state-title" style={{ fontSize: "var(--text-sm)" }}>No roles configured</div>
            </div>
          ) : rosterView === "compact" ? (
            /* Compact chip view — role names as horizontal chips */
            <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-1)", marginBottom: "var(--space-2)" }}>
              {roles.map(([slug, role]) => (
                <button
                  key={slug}
                  className="badge"
                  style={{
                    cursor: "pointer",
                    borderLeft: `3px solid ${getRoleColor(slug)}`,
                    padding: "var(--space-1) var(--space-2)",
                    background: "var(--bg-secondary)",
                    border: "1px solid var(--border)",
                    borderRadius: "var(--radius-sm)",
                    fontSize: "var(--text-xs)",
                  }}
                  onClick={() => setBriefingRole({ slug, title: role.title })}
                  aria-label={`${role.title} — click for briefing`}
                  title={`${role.title} (${role.provider?.provider || "anthropic"}/${role.provider?.model || "default"})`}
                >
                  <span style={{ color: getRoleColor(slug), marginRight: "var(--space-1)" }}>{"\u25CF"}</span>
                  {role.title}
                </button>
              ))}
            </div>
          ) : (
            /* Full card view — detailed role cards with actions + provider selectors */
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
                        style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-1)", color: "var(--warning)" }}
                        onClick={() => setInterruptTarget({ slug, title: role.title })}
                        aria-label={`Interrupt ${role.title}`}
                        title="Send priority interrupt"
                      >
                        {"\u26A0"}
                      </button>
                      <button
                        className="btn btn-ghost"
                        style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)" }}
                        onClick={() => startAgent(slug)}
                        aria-label={`Start ${role.title} agent`}
                      >
                        Start
                      </button>
                      <button
                        className="btn btn-ghost"
                        style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-1)", color: "var(--text-muted)" }}
                        onClick={() => stopAgent(slug)}
                        aria-label={`Stop ${role.title} agent`}
                        title="Stop agent"
                      >
                        Stop
                      </button>
                      <button
                        className="btn btn-ghost"
                        style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-1)", color: "var(--error)" }}
                        onClick={() => setDeleteRoleTarget({ slug, title: role.title })}
                        aria-label={`Delete ${role.title}`}
                        title="Delete role"
                      >
                        {"\u2715"}
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
            roleSlugs={roles.map(([s]) => s)}
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
            <div style={{ display: "flex", gap: "var(--space-2)", alignItems: "center" }}>
              <input
                className="input"
                style={{ width: 140, fontSize: "var(--text-xs)", padding: "2px var(--space-2)" }}
                value={msgFilter}
                onChange={(e) => setMsgFilter(e.target.value)}
                placeholder="Filter messages..."
                aria-label="Filter messages by role, type, or text"
              />
              <button
                className="btn btn-ghost"
                style={{ fontSize: "var(--text-xs)" }}
                onClick={() => setShowSections(!showSections)}
                aria-label="Toggle sections"
              >
                Sections {showSections ? "\u25B2" : "\u25BC"}
              </button>
            </div>
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
            {filteredMessages.length === 0 ? (
              <div className="empty-state" style={{ padding: "var(--space-8) var(--space-4)" }}>
                <div className="empty-state-icon">{"\uD83D\uDCAC"}</div>
                <div className="empty-state-title">{msgFilter ? "No matching messages" : "No messages yet"}</div>
                <div className="empty-state-desc">
                  {msgFilter
                    ? `No messages match "${msgFilter}". Try a different search.`
                    : "Start the agents and send a directive to kick things off."}
                </div>
              </div>
            ) : (
              <>
                {msgFilter && (
                  <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)", marginBottom: "var(--space-2)" }}>
                    Showing {filteredMessages.length} of {messages.length} messages
                  </div>
                )}
                {filteredMessages.length > visibleMsgCount && (
                  <button
                    className="btn btn-ghost"
                    style={{ width: "100%", fontSize: "var(--text-xs)", marginBottom: "var(--space-2)" }}
                    onClick={() => setVisibleMsgCount((c) => c + 50)}
                    aria-label={`Load more messages (showing ${visibleMsgCount} of ${filteredMessages.length})`}
                  >
                    Load more ({filteredMessages.length - visibleMsgCount} hidden)
                  </button>
                )}
                {filteredMessages.slice(-visibleMsgCount).map((msg) => (
                  <MessageCard key={msg.id} msg={msg} onDelete={handleDeleteMessage} onQuickReply={handleQuickReply} />
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
              <select
                className="input"
                style={{ width: 100, flexShrink: 0, fontSize: "var(--text-xs)" }}
                value={msgType}
                onChange={(e) => setMsgType(e.target.value)}
                aria-label="Message type"
              >
                <option value="directive">Directive</option>
                <option value="question">Question</option>
                <option value="status">Status</option>
                <option value="review">Review</option>
                <option value="approval">Approval</option>
                <option value="revision">Revision</option>
                <option value="broadcast">Broadcast</option>
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
              <textarea
                className="input"
                value={msgBody}
                onChange={(e) => setMsgBody(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
                placeholder="Type a message... (Enter to send, Shift+Enter for new line)"
                disabled={sending}
                aria-label="Message text"
                style={{ flex: 1, minHeight: 40, maxHeight: 120, resize: "vertical" }}
                rows={1}
              />
              <button
                className="btn btn-primary"
                onClick={handleSend}
                disabled={sending || !msgBody.trim()}
                aria-label="Send message"
                style={{ alignSelf: "flex-end" }}
              >
                {sending ? <div className="spinner" style={{ width: 14, height: 14 }} /> : "Send"}
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* Add role modal */}
      {showAddRole && (
        <AddRoleModal
          projectId={projectId!}
          existingSlugs={roles.map(([s]) => s)}
          onClose={() => setShowAddRole(false)}
          onCreated={() => selectProject(projectId!)}
        />
      )}

      {/* Role briefing modal */}
      {briefingRole && (
        <RoleBriefingModal
          projectId={projectId!}
          roleSlug={briefingRole.slug}
          roleTitle={briefingRole.title}
          onClose={() => setBriefingRole(null)}
        />
      )}

      {/* Delete role confirmation */}
      {deleteRoleTarget && (
        <ConfirmDialog
          title={`Delete ${deleteRoleTarget.title}?`}
          message="This will permanently remove this role and its briefing. This cannot be undone."
          confirmLabel="Delete"
          onConfirm={handleDeleteRole}
          onCancel={() => setDeleteRoleTarget(null)}
        />
      )}

      {/* Interrupt modal */}
      {interruptTarget && (
        <div
          className="modal-backdrop"
          onClick={(e) => { if (e.target === e.currentTarget) setInterruptTarget(null); }}
          role="dialog"
          aria-modal="true"
          aria-label={`Interrupt ${interruptTarget.title}`}
        >
          <div className="modal" style={{ maxWidth: 420 }}>
            <div className="modal-header">
              <h2 className="modal-title">Interrupt {interruptTarget.title}</h2>
              <button className="btn btn-ghost" onClick={() => setInterruptTarget(null)} aria-label="Close">
                {"\u2715"}
              </button>
            </div>
            <div style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", marginBottom: "var(--space-3)" }}>
              This sends a priority interrupt that appears at the top of the agent's next prompt.
              Use for urgent course corrections.
            </div>
            <div className="field" style={{ marginBottom: "var(--space-3)" }}>
              <label className="field-label" htmlFor="interrupt-reason">Reason</label>
              <textarea
                id="interrupt-reason"
                className="input"
                value={interruptReason}
                onChange={(e) => setInterruptReason(e.target.value)}
                placeholder="What should the agent change immediately?"
                style={{ minHeight: 80, resize: "vertical" }}
                autoFocus
              />
            </div>
            <div style={{ display: "flex", gap: "var(--space-2)", justifyContent: "flex-end" }}>
              <button className="btn btn-ghost" onClick={() => setInterruptTarget(null)}>Cancel</button>
              <button
                className="btn btn-danger"
                onClick={handleInterrupt}
                disabled={!interruptReason.trim()}
              >
                Send Interrupt
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
