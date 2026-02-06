import { useEffect, useState, useRef } from "react";
import type { ParsedProject, BoardMessage, RoleStatus, QuestionChoice, FileClaim } from "../lib/collabTypes";
import { RoleBriefingModal } from "./RoleBriefingModal";
import "../styles/collab.css";

const ROLE_COLORS: Record<string, string> = {
  manager: "#9b59b6",
  architect: "#1da1f2",
  developer: "#17bf63",
  tester: "#f5a623",
  user: "#e1e8ed",
};

function getRoleColor(slug: string): string {
  if (ROLE_COLORS[slug]) return ROLE_COLORS[slug];
  for (const [prefix, color] of Object.entries(ROLE_COLORS)) {
    if (slug.startsWith(prefix)) return color;
  }
  return "#657786";
}

const WORKFLOW_TYPES: Record<string, { label: string; color: string; desc: string }> = {
  full: { label: "Full Review", color: "#9b59b6", desc: "Complete onboarding + planning + full review pipeline" },
  quick: { label: "Quick Feature", color: "#17bf63", desc: "Skip onboarding, abbreviated review cycle" },
  bugfix: { label: "Bug Fix", color: "#f5a623", desc: "Focused diagnosis and fix, minimal review" },
};

function getWorkflowDisplay(type?: string, customColors?: Record<string, string>): { label: string; color: string } {
  if (type && WORKFLOW_TYPES[type]) {
    const color = customColors?.[type] || WORKFLOW_TYPES[type].color;
    return { label: WORKFLOW_TYPES[type].label, color };
  }
  return { label: "No Workflow", color: "#657786" };
}

interface VoteTally {
  proposalId: number;
  proposedValue: string;
  proposer: string;
  reason: string;
  yesVotes: string[];
  noVotes: string[];
  resolved: boolean;
}

function getActiveVotes(messages: BoardMessage[], activeCount: number): VoteTally[] {
  const required = Math.floor((activeCount + 1) / 2) + 1; // +1 for human
  const proposals = messages.filter(
    (m) => m.type === "vote" && m.metadata?.vote_type === "workflow_change" && !m.metadata?.in_reply_to
  );

  return proposals.map((proposal) => {
    const yesVotes: string[] = [];
    const noVotes: string[] = [];

    // Proposer's vote
    if (proposal.metadata?.vote === "yes") {
      yesVotes.push(proposal.from);
    } else if (proposal.metadata?.vote === "no") {
      noVotes.push(proposal.from);
    }

    // Response votes (deduplicate by role)
    const seenRoles = new Set<string>(yesVotes.concat(noVotes));
    messages
      .filter(
        (m) =>
          m.type === "vote" &&
          m.metadata?.vote_type === "workflow_change" &&
          m.metadata?.in_reply_to === proposal.id
      )
      .forEach((response) => {
        const role = response.from;
        if (seenRoles.has(role)) return;
        seenRoles.add(role);
        if (response.metadata?.vote === "yes") {
          yesVotes.push(role);
        } else {
          noVotes.push(role);
        }
      });

    const resolved = yesVotes.length >= required || noVotes.length >= required;

    return {
      proposalId: proposal.id,
      proposedValue: (proposal.metadata?.proposed_value as string) || "",
      proposer: proposal.from,
      reason: proposal.body,
      yesVotes,
      noVotes,
      resolved,
    };
  });
}

function getStatusDotClass(status: string): string {
  if (status === "active") return "project-status-dot active";
  if (status === "idle") return "project-status-dot idle";
  if (status === "gone") return "project-status-dot gone";
  if (status === "stale") return "project-status-dot stale";
  return "project-status-dot";
}

function MessageTypeBadge({ type: msgType }: { type: string }) {
  return <span className={`message-type-badge badge-${msgType}`}>{msgType}</span>;
}

function getAnswerForQuestion(questionId: number, messages: BoardMessage[]): { choiceId: string } | null {
  const answer = messages.find(
    (m) => m.metadata?.in_reply_to === questionId && m.type === "answer"
  );
  if (answer?.metadata?.choice_id) {
    return { choiceId: answer.metadata.choice_id as string };
  }
  return null;
}

function QuestionCard({
  msg,
  answered,
  onAnswer,
  onDelete,
}: {
  msg: BoardMessage;
  answered: { choiceId: string } | null;
  onAnswer: (choiceId: string, choiceLabel: string) => void;
  onDelete?: (id: number) => void;
}) {
  const fromRole = msg.from.split(":")[0];
  const choices = (msg.metadata?.choices || []) as QuestionChoice[];

  return (
    <div
      className={`message-card question-card ${answered ? "question-answered" : ""}`}
      style={{ borderLeftColor: getRoleColor(fromRole) }}
    >
      <div className="message-card-header">
        <span className="message-card-id">#{msg.id}</span>
        <span className="message-card-from" style={{ color: getRoleColor(fromRole) }}>
          {msg.from}
        </span>
        <span className="message-card-arrow">&rarr;</span>
        <span className="message-card-to" style={{ color: "#e1e8ed" }}>you</span>
        <MessageTypeBadge type={msg.type} />
        <span className="message-card-time" title={msg.timestamp}>
          {formatRelativeTime(msg.timestamp)}
        </span>
        {onDelete && <button className="message-delete-btn" onClick={(e) => { e.stopPropagation(); onDelete(msg.id); }} title="Delete message">&times;</button>}
      </div>
      {msg.subject && <div className="message-card-subject">{msg.subject}</div>}
      <div className="message-card-body">{msg.body}</div>
      <div className="question-choices">
        {choices.map((choice) => (
          <button
            key={choice.id}
            className={`question-choice-btn ${answered?.choiceId === choice.id ? "selected" : ""}`}
            onClick={() => !answered && onAnswer(choice.id, choice.label)}
            disabled={!!answered}
          >
            <span className="question-choice-label">{choice.label}</span>
            {choice.desc && <span className="question-choice-desc">{choice.desc}</span>}
          </button>
        ))}
      </div>
    </div>
  );
}

function VoteCard({
  tally,
  activeCount,
  onVote,
  customColors,
  onDelete,
}: {
  tally: VoteTally;
  activeCount: number;
  onVote: (proposalId: number, vote: string, proposedValue: string) => void;
  customColors?: Record<string, string>;
  onDelete?: (id: number) => void;
}) {
  const required = Math.floor((activeCount + 1) / 2) + 1;
  const total = tally.yesVotes.length + tally.noVotes.length;
  const yesPercent = total > 0 ? (tally.yesVotes.length / total) * 100 : 0;
  const noPercent = total > 0 ? (tally.noVotes.length / total) * 100 : 0;
  const workflowDisplay = getWorkflowDisplay(tally.proposedValue, customColors);
  const passed = tally.yesVotes.length >= required;

  return (
    <div className={`message-card vote-card ${tally.resolved ? "vote-resolved" : ""}`}
      style={{ borderLeftColor: "#9b59b6" }}
    >
      <div className="message-card-header">
        <span className="message-card-id">#{tally.proposalId}</span>
        <span className="message-card-from" style={{ color: "#9b59b6" }}>
          {tally.proposer}
        </span>
        <MessageTypeBadge type="vote" />
        {onDelete && <button className="message-delete-btn" onClick={(e) => { e.stopPropagation(); onDelete(tally.proposalId); }} title="Delete message">&times;</button>}
      </div>
      <div className="message-card-subject">
        Workflow change: <span style={{ color: workflowDisplay.color }}>{workflowDisplay.label}</span>
      </div>
      {tally.reason && <div className="message-card-body">{tally.reason}</div>}
      <div className="vote-tally">
        <div className="vote-tally-info">
          <span>{tally.yesVotes.length} yes / {tally.noVotes.length} no</span>
          <span>Need <span className="vote-threshold-marker">{required}</span> to pass</span>
        </div>
        <div className="vote-tally-bar">
          <div className="vote-tally-yes" style={{ width: `${yesPercent}%` }} />
          <div className="vote-tally-no" style={{ width: `${noPercent}%` }} />
        </div>
      </div>
      {!tally.resolved && (
        <div className="vote-actions">
          <button className="vote-btn-yes" onClick={() => onVote(tally.proposalId, "yes", tally.proposedValue)}>
            Vote Yes
          </button>
          <button className="vote-btn-no" onClick={() => onVote(tally.proposalId, "no", tally.proposedValue)}>
            Vote No
          </button>
        </div>
      )}
      {tally.resolved && (
        <div style={{ marginTop: 8, fontSize: 12, color: passed ? "#17bf63" : "#e0245e", fontWeight: 600 }}>
          {passed ? "Passed — workflow changed" : "Rejected — not enough votes"}
        </div>
      )}
    </div>
  );
}

function formatRelativeTime(isoTimestamp: string): string {
  try {
    const msgTime = new Date(isoTimestamp).getTime();
    const now = Date.now();
    const diffSec = Math.floor((now - msgTime) / 1000);
    if (diffSec < 0) return "just now";
    if (diffSec < 60) return `${diffSec}s ago`;
    const diffMin = Math.floor(diffSec / 60);
    if (diffMin < 60) return `${diffMin}m ago`;
    const diffHr = Math.floor(diffMin / 60);
    if (diffHr < 24) return `${diffHr}h ${diffMin % 60}m ago`;
    const diffDay = Math.floor(diffHr / 24);
    return `${diffDay}d ago`;
  } catch {
    return isoTimestamp;
  }
}

function buildDefaultConfig(dirPath: string) {
  const now = new Date().toISOString();
  const id = `proj-${Date.now().toString(36)}`;
  // Derive project name from folder name
  const parts = dirPath.replace(/\\/g, "/").split("/").filter(Boolean);
  const folderName = parts[parts.length - 1] || "My Project";
  return {
    project_id: id,
    name: folderName,
    description: "AI-assisted development project",
    created_at: now,
    updated_at: now,
    roles: {
      architect: {
        title: "Architect",
        description: "Owns the technical vision, reviews architecture decisions, ensures consistency across the codebase",
        max_instances: 1,
        permissions: ["broadcast", "review", "assign_tasks"],
        created_at: now,
      },
      manager: {
        title: "Project Manager",
        description: "Coordinates tasks, reviews work, manages priorities",
        max_instances: 1,
        permissions: ["assign_tasks", "broadcast", "review"],
        created_at: now,
      },
      developer: {
        title: "Developer",
        description: "Implements features, fixes bugs, writes code",
        max_instances: 3,
        permissions: ["status", "handoff", "question"],
        created_at: now,
      },
      tester: {
        title: "Tester",
        description: "Writes and runs tests, validates implementations, reports bugs",
        max_instances: 1,
        permissions: ["status", "question"],
        created_at: now,
      },
    },
    settings: {
      heartbeat_timeout_seconds: 120,
      message_retention_days: 7,
    },
  };
}

export function CollabTab() {
  const [project, setProject] = useState<ParsedProject | null>(null);
  const [projectDir, setProjectDir] = useState("");
  const [watching, setWatching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [autoCollab, setAutoCollab] = useState(false);
  const [humanInLoop, setHumanInLoop] = useState(false);
  const [selectedRole, setSelectedRole] = useState<RoleStatus | null>(null);
  const [msgTo, setMsgTo] = useState("all");
  const [msgBody, setMsgBody] = useState("");
  const [sending, setSending] = useState(false);
  const [workflowDropdownOpen, setWorkflowDropdownOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [retentionDays, setRetentionDays] = useState(7);
  const [confirmAction, setConfirmAction] = useState<{ title: string; message: string; onConfirm: () => void } | null>(null);
  const workflowDropdownRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Close dropdown on click outside
  useEffect(() => {
    if (!workflowDropdownOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (workflowDropdownRef.current && !workflowDropdownRef.current.contains(e.target as Node)) {
        setWorkflowDropdownOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [workflowDropdownOpen]);

  // Load settings on mount
  useEffect(() => {
    if (!window.__TAURI__) return;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const val = await invoke<boolean>("get_auto_collab");
        setAutoCollab(val);
        const hil = await invoke<boolean>("get_human_in_loop");
        setHumanInLoop(hil);
      } catch { /* ignore */ }
    })();
  }, []);

  const toggleAutoCollab = async () => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        const newVal = !autoCollab;
        await invoke("set_auto_collab", { enabled: newVal });
        setAutoCollab(newVal);
      }
    } catch { /* ignore */ }
  };

  const toggleHumanInLoop = async () => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        const newVal = !humanInLoop;
        await invoke("set_human_in_loop", { enabled: newVal });
        setHumanInLoop(newVal);
      }
    } catch { /* ignore */ }
  };

  const handleSetWorkflow = async (type: string | null) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_workflow_type", {
          dir: projectDir,
          workflowType: type,
        });
        setWorkflowDropdownOpen(false);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to set workflow type:", e);
    }
  };

  const handleVote = async (proposalId: number, vote: string, proposedValue: string) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("send_team_message", {
          dir: projectDir,
          to: "all",
          subject: "Re: Workflow change",
          body: vote === "yes" ? "Approved" : "Rejected",
          msgType: "vote",
          metadata: {
            vote_type: "workflow_change",
            in_reply_to: proposalId,
            proposed_value: proposedValue,
            vote,
          },
        });
      }
    } catch (e) {
      console.error("[CollabTab] Failed to send vote:", e);
    }
  };

  const handleDeleteMessage = (id: number) => {
    setConfirmAction({
      title: "Delete message",
      message: `Delete message #${id}? This cannot be undone.`,
      onConfirm: async () => {
        try {
          if (window.__TAURI__) {
            const { invoke } = await import("@tauri-apps/api/core");
            await invoke("delete_message", { dir: projectDir, messageId: id });
          }
        } catch (e) {
          console.error("[CollabTab] Failed to delete message:", e);
        }
        setConfirmAction(null);
      },
    });
  };

  const handleClearAllMessages = () => {
    setConfirmAction({
      title: "Clear all messages",
      message: `Clear all ${project?.messages?.length || 0} messages? This cannot be undone.`,
      onConfirm: async () => {
        try {
          if (window.__TAURI__) {
            const { invoke } = await import("@tauri-apps/api/core");
            await invoke("clear_all_messages", { dir: projectDir });
          }
        } catch (e) {
          console.error("[CollabTab] Failed to clear messages:", e);
        }
        setConfirmAction(null);
      },
    });
  };

  const handleSetRetention = async (days: number) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_message_retention", { dir: projectDir, days });
        setRetentionDays(days);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to set retention:", e);
    }
  };

  // Auto-apply workflow when majority reached
  useEffect(() => {
    if (!project?.messages || !project?.sessions) return;
    const activeCount = project.sessions.filter(
      (s) => s.status === "active" || s.status === "idle"
    ).length;
    const votes = getActiveVotes(project.messages, activeCount);
    const required = Math.floor((activeCount + 1) / 2) + 1;

    for (const tally of votes) {
      if (tally.yesVotes.length >= required && !tally.resolved) {
        // Check if workflow isn't already set to this value
        if (project.config?.settings?.workflow_type !== tally.proposedValue) {
          handleSetWorkflow(tally.proposedValue);
        }
      }
    }
  }, [project?.messages?.length]);

  const handleAnswerQuestion = async (msg: BoardMessage, choiceId: string, choiceLabel: string) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("send_team_message", {
          dir: projectDir,
          to: msg.from.split(":")[0],
          subject: `Re: ${msg.subject}`,
          body: choiceLabel,
          msgType: "answer",
          metadata: { in_reply_to: msg.id, choice_id: choiceId },
        });
      }
    } catch (e) {
      console.error("[CollabTab] Failed to send answer:", e);
    }
  };

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [project?.messages?.length]);

  // Listen for project file change events from backend
  useEffect(() => {
    if (!window.__TAURI__) return;

    let unlistenUpdate: (() => void) | undefined;
    let unlistenFileChanged: (() => void) | undefined;

    const setupListeners = async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");

        unlistenUpdate = await listen<ParsedProject>(
          "project-update",
          (event) => {
            setProject(event.payload);
            if (event.payload?.config?.settings?.message_retention_days != null) {
              setRetentionDays(event.payload.config.settings.message_retention_days);
            }
          }
        );

        unlistenFileChanged = await listen(
          "project-file-changed",
          async () => {
            if (!watching) return;
            try {
              const { invoke } = await import("@tauri-apps/api/core");
              const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
              if (result) setProject(result);
            } catch { /* ignore */ }
          }
        );
      } catch (err) {
        console.error("[CollabTab] Failed to setup listeners:", err);
      }
    };

    setupListeners();
    return () => {
      unlistenUpdate?.();
      unlistenFileChanged?.();
    };
  }, [watching, projectDir]);

  const pickFolder = async () => {
    try {
      if (window.__TAURI__) {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const selected = await open({ directory: true, multiple: false });
        if (selected) setProjectDir(selected as string);
      }
    } catch { /* user types manually */ }
  };

  const startWatching = async () => {
    if (!projectDir.trim()) {
      setError("Please enter a project directory");
      return;
    }
    setError(null);
    setLoading(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");

        // Try to read existing project
        let result = await invoke<(ParsedProject & { effective_dir?: string }) | null>("watch_project_dir", { dir: projectDir });

        // No .vaak/ found — auto-create it
        if (!result) {
          const config = buildDefaultConfig(projectDir);
          await invoke("initialize_project", {
            dir: projectDir,
            config: JSON.stringify(config),
          });
          // Re-read after creation
          result = await invoke<(ParsedProject & { effective_dir?: string }) | null>("watch_project_dir", { dir: projectDir });
        }

        // Update projectDir if the backend found a better subdirectory
        if (result?.effective_dir && result.effective_dir !== projectDir) {
          setProjectDir(result.effective_dir);
        }

        setWatching(true);
        setProject(result);
        if (result?.config?.settings?.message_retention_days != null) {
          setRetentionDays(result.config.settings.message_retention_days);
        }
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const stopWatching = async () => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("stop_watching_project");
      }
    } catch { /* ignore */ }
    setWatching(false);
    setProject(null);
  };

  const sendMessage = async () => {
    if (!msgBody.trim() || !projectDir) return;
    setSending(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("send_team_message", {
          dir: projectDir,
          to: msgTo,
          subject: "",
          body: msgBody.trim(),
        });
        setMsgBody("");
      }
    } catch (e) {
      console.error("[CollabTab] Failed to send message:", e);
    } finally {
      setSending(false);
    }
  };

  // ===== WATCHING STATE: Project Dashboard =====
  if (watching) {
    const hasNoSessions = !project || project.sessions.length === 0;
    const hasNoMessages = !project || project.messages.length === 0;
    const pendingQuestionCount = project ? project.messages.filter(
      (m) => m.to === "human" && m.type === "question" && m.metadata?.choices?.length &&
        !getAnswerForQuestion(m.id, project.messages)
    ).length : 0;

    return (
      <div className="project-tab">
        {/* Header */}
        <div className="project-header">
          <div className="project-header-info">
            <span className={`project-status-dot ${!hasNoSessions ? "active" : ""}`} />
            <span className="project-header-name">
              {project?.config?.name || "Project"}
            </span>
            {pendingQuestionCount > 0 && (
              <span
                className="pending-questions-badge"
                title={`${pendingQuestionCount} question${pendingQuestionCount > 1 ? "s" : ""} awaiting your answer`}
              >
                {pendingQuestionCount}
              </span>
            )}
            {(() => {
              const customColors = project?.config?.settings?.workflow_colors;
              const wf = getWorkflowDisplay(project?.config?.settings?.workflow_type, customColors);
              return (
                <div className="workflow-badge-wrapper" ref={workflowDropdownRef}>
                  <span
                    className="workflow-badge"
                    style={{
                      background: `${wf.color}22`,
                      color: wf.color,
                      borderColor: `${wf.color}55`,
                    }}
                    onClick={() => setWorkflowDropdownOpen(!workflowDropdownOpen)}
                    title="Click to change workflow type"
                  >
                    {wf.label}
                  </span>
                  {workflowDropdownOpen && (
                    <div className="workflow-dropdown">
                      {Object.entries(WORKFLOW_TYPES).map(([id, wt]) => {
                        const itemColor = customColors?.[id] || wt.color;
                        return (
                          <div
                            key={id}
                            className="workflow-dropdown-item"
                            onClick={() => handleSetWorkflow(id)}
                          >
                            <span className="workflow-dropdown-dot" style={{ background: itemColor }} />
                            <div className="workflow-dropdown-info">
                              <span className="workflow-dropdown-label">{wt.label}</span>
                              <span className="workflow-dropdown-desc">{wt.desc}</span>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })()}
            {project?.config?.description && (
              <span className="project-header-desc">{project.config.description}</span>
            )}
          </div>
          <label className="auto-collab-toggle" title="When enabled, agents autonomously check messages, act on directives, and communicate without manual prompting">
            <input
              type="checkbox"
              checked={autoCollab}
              onChange={toggleAutoCollab}
            />
            <span className="auto-collab-label">Auto</span>
          </label>
          <label className="auto-collab-toggle human-in-loop-toggle" title="When enabled, you become a checkpoint in the review chain — agents ask for your approval at key stages">
            <input
              type="checkbox"
              checked={humanInLoop}
              onChange={toggleHumanInLoop}
            />
            <span className="auto-collab-label">Review</span>
          </label>
          <button
            className="project-settings-btn"
            onClick={() => setSettingsOpen(!settingsOpen)}
            title="Board settings"
          >
            &#9881;
          </button>
          <button className="project-stop-btn" onClick={stopWatching}>Stop</button>
        </div>

        {/* Settings Panel */}
        {settingsOpen && (
          <div className="project-settings-panel">
            <div className="settings-row">
              <div>
                <div className="settings-label-text">Message retention</div>
                <div className="settings-label-hint">Auto-delete messages older than this</div>
              </div>
              <select
                className="settings-select"
                value={retentionDays}
                onChange={(e) => handleSetRetention(Number(e.target.value))}
              >
                <option value={1}>1 day</option>
                <option value={3}>3 days</option>
                <option value={7}>7 days</option>
                <option value={14}>14 days</option>
                <option value={30}>30 days</option>
                <option value={0}>Never</option>
              </select>
            </div>
            <div className="settings-row">
              <div>
                <div className="settings-label-text">
                  {project?.messages?.length || 0} message{(project?.messages?.length || 0) !== 1 ? "s" : ""} on board
                </div>
                <div className="settings-label-hint">Remove all messages from the board</div>
              </div>
              <button
                className="settings-danger-btn"
                onClick={handleClearAllMessages}
                disabled={!project?.messages?.length}
              >
                Clear All
              </button>
            </div>
          </div>
        )}

        {/* File path */}
        <div className="project-filepath">
          <span className="project-filepath-label">Watching:</span>
          <code>{projectDir}/.vaak/</code>
        </div>

        {/* Role Cards Grid */}
        {project && project.role_statuses.length > 0 && (
          <div className="project-roles-grid">
            {project.role_statuses.map((role: RoleStatus) => (
              <div
                key={role.slug}
                className="role-card role-card-clickable"
                style={{ borderLeftColor: getRoleColor(role.slug) }}
                onClick={() => setSelectedRole(role)}
              >
                <div className="role-card-header">
                  <span className={getStatusDotClass(role.status)} />
                  <span className="role-card-title" style={{ color: getRoleColor(role.slug) }}>
                    {role.title}
                  </span>
                </div>
                <div className="role-card-meta">
                  <span className="role-card-slug">{role.slug}</span>
                  <span className="role-card-instances">
                    {role.active_instances}/{role.max_instances}{" "}
                    {role.status === "active" ? "active" : role.status === "idle" ? "idle" : role.status === "gone" ? "gone" : role.status === "vacant" ? "vacant" : role.status}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Active Claims Section */}
        {project && project.claims && project.claims.length > 0 && (
          <div className="claims-section">
            <div className="claims-section-title">Active Claims</div>
            {project.claims.map((claim: FileClaim) => {
              const roleSlug = claim.role_instance.split(":")[0] || "";
              const filesDisplay = claim.files.length > 2
                ? `${claim.files[0]} (+${claim.files.length - 1} more)`
                : claim.files.join(", ");
              return (
                <div key={claim.role_instance} className="claim-card">
                  <div className="claim-role-dot" style={{ background: getRoleColor(roleSlug) }} />
                  <span className="claim-role-label" style={{ color: getRoleColor(roleSlug) }}>
                    {claim.role_instance}
                  </span>
                  <div className="claim-info">
                    <div className="claim-files">{filesDisplay}</div>
                    <div className="claim-desc">{claim.description}</div>
                  </div>
                  <span className="claim-time">{formatRelativeTime(claim.claimed_at)}</span>
                </div>
              );
            })}
          </div>
        )}

        {/* Contextual hint when no sessions */}
        {hasNoSessions && (
          <div className="project-hint-banner">
            <div className="project-hint-title">Ready for team members</div>
            <div className="project-hint-body">
              Any Claude Code session running in this folder will automatically detect the project
              and join a role. Just start chatting — Claude will see the team and join on its own.
            </div>
          </div>
        )}

        {/* Message Timeline */}
        <div className="message-timeline">
          {hasNoMessages ? (
            <div className="message-timeline-empty">
              {hasNoSessions
                ? "No team members connected yet"
                : "Team connected — waiting for first message..."}
            </div>
          ) : (
            (() => {
              const activeCount = project!.sessions.filter(
                (s) => s.status === "active" || s.status === "idle"
              ).length;
              const voteTallies = getActiveVotes(project!.messages, activeCount);
              const voteProposalIds = new Set(voteTallies.map((t) => t.proposalId));
              // IDs of response votes — hide from timeline
              const voteResponseIds = new Set(
                project!.messages
                  .filter((m) => m.type === "vote" && m.metadata?.in_reply_to)
                  .map((m) => m.id)
              );

              return project!.messages.map((msg: BoardMessage) => {
              // Vote proposal → render as VoteCard
              if (voteProposalIds.has(msg.id)) {
                const tally = voteTallies.find((t) => t.proposalId === msg.id);
                if (tally) {
                  return (
                    <VoteCard
                      key={msg.id}
                      tally={tally}
                      activeCount={activeCount}
                      onVote={handleVote}
                      customColors={project!.config?.settings?.workflow_colors}
                      onDelete={handleDeleteMessage}
                    />
                  );
                }
              }

              // Vote responses are hidden (tallied in VoteCard)
              if (voteResponseIds.has(msg.id)) return null;

              // Interactive question card for human-targeted questions with choices
              if (msg.to === "human" && msg.type === "question" && msg.metadata?.choices?.length) {
                const answered = getAnswerForQuestion(msg.id, project!.messages);
                return (
                  <QuestionCard
                    key={msg.id}
                    msg={msg}
                    answered={answered}
                    onAnswer={(choiceId, choiceLabel) => handleAnswerQuestion(msg, choiceId, choiceLabel)}
                    onDelete={handleDeleteMessage}
                  />
                );
              }

              const fromRole = msg.from.split(":")[0];
              const borderColor = getRoleColor(fromRole);
              return (
                <div
                  key={msg.id}
                  className="message-card"
                  style={{ borderLeftColor: borderColor }}
                >
                  <div className="message-card-header">
                    <span className="message-card-id">#{msg.id}</span>
                    <span className="message-card-from" style={{ color: getRoleColor(fromRole) }}>
                      {msg.from}
                    </span>
                    <span className="message-card-arrow">&rarr;</span>
                    <span className="message-card-to" style={{ color: getRoleColor(msg.to) }}>
                      {msg.to}
                    </span>
                    <MessageTypeBadge type={msg.type} />
                    <span className="message-card-time" title={msg.timestamp}>{formatRelativeTime(msg.timestamp)}</span>
                    <button className="message-delete-btn" onClick={(e) => { e.stopPropagation(); handleDeleteMessage(msg.id); }} title="Delete message">&times;</button>
                  </div>
                  {msg.subject && (
                    <div className="message-card-subject">{msg.subject}</div>
                  )}
                  <div className="message-card-body">{msg.body}</div>
                </div>
              );
            });
            })()
          )}
          <div ref={messagesEndRef} />
        </div>

        {/* Compose Bar */}
        <div className="compose-bar">
          <select
            className="compose-target"
            value={msgTo}
            onChange={(e) => setMsgTo(e.target.value)}
          >
            <option value="all">@ Everyone</option>
            {project?.role_statuses.map((role: RoleStatus) => (
              <option key={role.slug} value={role.slug}>
                @ {role.title}{role.active_instances > 1 ? " (all)" : ""}
              </option>
            ))}
            {/* Instance-specific options for roles with multiple active instances */}
            {project?.role_statuses
              .filter((role: RoleStatus) => role.active_instances > 1)
              .flatMap((role: RoleStatus) => {
                const instances: JSX.Element[] = [];
                for (let i = 0; i < role.active_instances; i++) {
                  instances.push(
                    <option key={`${role.slug}:${i}`} value={`${role.slug}:${i}`}>
                      @ {role.title} :{i}
                    </option>
                  );
                }
                return instances;
              })}
          </select>
          <input
            className="compose-input"
            type="text"
            value={msgBody}
            onChange={(e) => setMsgBody(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && sendMessage()}
            placeholder="Send a message to your team..."
            disabled={sending}
          />
          <button
            className="compose-send-btn"
            onClick={sendMessage}
            disabled={!msgBody.trim() || sending}
          >
            Send
          </button>
        </div>

        {/* Role Briefing Modal */}
        {selectedRole && (
          <RoleBriefingModal
            projectDir={projectDir}
            roleSlug={selectedRole.slug}
            roleTitle={selectedRole.title}
            roleColor={getRoleColor(selectedRole.slug)}
            onClose={() => setSelectedRole(null)}
          />
        )}

        {/* Confirmation Dialog */}
        {confirmAction && (
          <div className="confirm-dialog" onClick={() => setConfirmAction(null)}>
            <div className="confirm-dialog-box" onClick={(e) => e.stopPropagation()}>
              <div className="confirm-dialog-title">{confirmAction.title}</div>
              <div className="confirm-dialog-message">{confirmAction.message}</div>
              <div className="confirm-dialog-actions">
                <button className="confirm-dialog-cancel" onClick={() => setConfirmAction(null)}>Cancel</button>
                <button className="confirm-dialog-delete" onClick={confirmAction.onConfirm}>Delete</button>
              </div>
            </div>
          </div>
        )}
      </div>
    );
  }

  // ===== SETUP STATE: Directory Picker =====
  return (
    <div className="project-tab">
      <div className="project-setup">
        <div className="project-setup-title">Project Team Dashboard</div>
        <div className="project-setup-hint">
          Watch a project to see AI agent roles and their messages in real time.
        </div>

        <div className="project-watch-input">
          <div className="project-folder-row">
            <input
              type="text"
              value={projectDir}
              onChange={(e) => setProjectDir(e.target.value)}
              placeholder="Project directory path"
              onKeyDown={(e) => e.key === "Enter" && startWatching()}
            />
            <button className="project-browse-btn" onClick={pickFolder}>Browse</button>
          </div>
          {error && <div className="project-watch-error">{error}</div>}
          <button
            className="project-start-btn"
            onClick={startWatching}
            disabled={!projectDir.trim() || loading}
          >
            {loading ? "Setting up..." : "Watch Project"}
          </button>
        </div>
      </div>
    </div>
  );
}
