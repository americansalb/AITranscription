import { useEffect, useState, useRef } from "react";
import type { ParsedProject, BoardMessage, RoleStatus, SessionBinding, QuestionChoice, FileClaim, DiscussionState, Section, RosterSlot, RoleConfig } from "../lib/collabTypes";
import { RoleBriefingModal } from "./RoleBriefingModal";
import "../styles/collab.css";

const ROLE_COLORS: Record<string, string> = {
  manager: "#9b59b6",
  architect: "#1da1f2",
  developer: "#17bf63",
  tester: "#f5a623",
  user: "#e1e8ed",
};

// Pipeline order for role cards: Manager → Architect → Developer → Tester
const ROLE_ORDER: Record<string, number> = {
  manager: 0,
  architect: 1,
  developer: 2,
  tester: 3,
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
  if (status === "working") return "project-status-dot working";
  if (status === "active") return "project-status-dot active";
  if (status === "standby") return "project-status-dot standby";
  if (status === "idle") return "project-status-dot idle";
  if (status === "vacant") return "project-status-dot vacant";
  if (status === "gone") return "project-status-dot gone";
  if (status === "stale") return "project-status-dot stale";
  return "project-status-dot";
}

function sortRolesByPipeline(roles: RoleStatus[]): RoleStatus[] {
  return [...roles].sort((a, b) => {
    const orderA = ROLE_ORDER[a.slug] ?? 99;
    const orderB = ROLE_ORDER[b.slug] ?? 99;
    return orderA - orderB;
  });
}

interface InstanceCard {
  slug: string;
  title: string;
  instance: number;
  status: "working" | "standby" | "vacant";
  roleColor: string;
}

function computeInstanceStatus(
  session: SessionBinding,
  timeoutSecs: number,
  nowSecs: number
): InstanceCard["status"] {
  const hbEpoch = new Date(session.last_heartbeat).getTime() / 1000;
  const age = nowSecs - hbEpoch;

  // Immediate detection: process wrote "disconnected" on exit
  if (session.activity === "disconnected") return "vacant";

  // Heartbeat comes every 30s. If older than 90s, session is dead → vacant
  const goneThreshold = Math.min(timeoutSecs, 90);
  if (age > goneThreshold) return "vacant";

  // Use activity field if available (set by vaak-mcp.rs)
  if (session.activity === "working") return "working";
  if (session.activity === "standby") {
    // Minimum display duration: if the session was working within the last 30 seconds,
    // show "working" instead of "standby" so the human can actually see the transition
    const lwAt = session.last_working_at;
    if (lwAt) {
      const workAge = nowSecs - new Date(lwAt).getTime() / 1000;
      if (workAge < 30) return "working";
    }
    return "standby";
  }

  // Fallback: heartbeat-based classification for sessions without activity field
  if (age > 60) return "vacant";
  return "standby";
}

/** Build roster-based instance cards. Uses roster slots if available, falls back to sessions. */
function buildRosterCards(
  roster: RosterSlot[] | undefined,
  roles: Record<string, RoleConfig>,
  roleStatuses: RoleStatus[],
  sessions: SessionBinding[],
  timeoutSecs: number
): InstanceCard[] {
  const nowSecs = Date.now() / 1000;
  const cards: InstanceCard[] = [];

  if (roster && roster.length > 0) {
    // Roster-based: one card per roster slot
    const sorted = [...roster].sort((a, b) => {
      const orderA = ROLE_ORDER[a.role] ?? 99;
      const orderB = ROLE_ORDER[b.role] ?? 99;
      if (orderA !== orderB) return orderA - orderB;
      return a.instance - b.instance;
    });

    // Track which sessions are covered by roster slots
    const coveredSessions = new Set<string>();

    for (const slot of sorted) {
      const roleDef = roles[slot.role];
      const title = roleDef?.title || slot.role;
      // Find matching session for this slot
      const session = sessions.find(
        (s) => s.role === slot.role && s.instance === slot.instance && s.status === "active"
      );
      if (session) coveredSessions.add(`${session.role}:${session.instance}`);
      const slotsForRole = sorted.filter(s => s.role === slot.role);
      const displayTitle = slotsForRole.length > 1 ? `${title} :${slot.instance}` : title;

      cards.push({
        slug: slot.role,
        title: displayTitle,
        instance: slot.instance,
        status: session ? computeInstanceStatus(session, timeoutSecs, nowSecs) : "vacant",
        roleColor: getRoleColor(slot.role),
      });
    }

    // Also show active sessions not covered by the roster (legacy/pre-roster agents)
    const uncoveredSessions = sessions.filter(
      (s) => s.status === "active" && !coveredSessions.has(`${s.role}:${s.instance}`)
    ).sort((a, b) => {
      const orderA = ROLE_ORDER[a.role] ?? 99;
      const orderB = ROLE_ORDER[b.role] ?? 99;
      if (orderA !== orderB) return orderA - orderB;
      return a.instance - b.instance;
    });
    for (const s of uncoveredSessions) {
      const roleDef = roles[s.role];
      const title = roleDef?.title || s.role;
      const sameRole = uncoveredSessions.filter(u => u.role === s.role);
      const rosterSameRole = sorted.filter(r => r.role === s.role);
      const needsSuffix = sameRole.length > 1 || rosterSameRole.length > 0;
      const displayTitle = needsSuffix ? `${title} :${s.instance}` : title;
      cards.push({
        slug: s.role,
        title: displayTitle,
        instance: s.instance,
        status: computeInstanceStatus(s, timeoutSecs, nowSecs),
        roleColor: getRoleColor(s.role),
      });
    }
  } else {
    // Fallback: legacy behavior from role_statuses + sessions
    for (const role of sortRolesByPipeline(roleStatuses)) {
      const roleSessions = sessions
        .filter((s) => s.role === role.slug && s.status === "active")
        .sort((a, b) => a.instance - b.instance);

      if (roleSessions.length === 0) {
        cards.push({
          slug: role.slug,
          title: role.title,
          instance: 0,
          status: "vacant",
          roleColor: getRoleColor(role.slug),
        });
      } else if (roleSessions.length === 1) {
        const s = roleSessions[0];
        cards.push({
          slug: role.slug,
          title: role.title,
          instance: s.instance,
          status: computeInstanceStatus(s, timeoutSecs, nowSecs),
          roleColor: getRoleColor(role.slug),
        });
      } else {
        for (const s of roleSessions) {
          cards.push({
            slug: role.slug,
            title: `${role.title} :${s.instance}`,
            instance: s.instance,
            status: computeInstanceStatus(s, timeoutSecs, nowSecs),
            roleColor: getRoleColor(role.slug),
          });
        }
      }
    }
  }

  return cards;
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

const COLLAB_STORAGE_KEY = "vaak_collab_project_dir";
const SAVED_PROJECTS_KEY = "vaak_projects";

function loadPersistedDir(): string {
  try {
    const stored = localStorage.getItem(COLLAB_STORAGE_KEY);
    return stored ? JSON.parse(stored) : "";
  } catch { return ""; }
}

function persistDir(dir: string): void {
  try {
    if (dir) {
      localStorage.setItem(COLLAB_STORAGE_KEY, JSON.stringify(dir));
    } else {
      localStorage.removeItem(COLLAB_STORAGE_KEY);
    }
  } catch { /* ignore */ }
}

interface SavedProject {
  name: string;
  path: string;
  addedAt: string;
}

function loadSavedProjects(): SavedProject[] {
  try {
    const stored = localStorage.getItem(SAVED_PROJECTS_KEY);
    return stored ? JSON.parse(stored) : [];
  } catch { return []; }
}

function addSavedProject(path: string, name?: string): void {
  try {
    const projects = loadSavedProjects().filter(p => p.path !== path);
    const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
    const autoName = name || parts[parts.length - 1] || "My Project";
    projects.unshift({ name: autoName, path, addedAt: new Date().toISOString() });
    localStorage.setItem(SAVED_PROJECTS_KEY, JSON.stringify(projects));
  } catch { /* ignore */ }
}

function removeSavedProject(path: string): void {
  try {
    const projects = loadSavedProjects().filter(p => p.path !== path);
    localStorage.setItem(SAVED_PROJECTS_KEY, JSON.stringify(projects));
  } catch { /* ignore */ }
}

export function CollabTab() {
  const [project, setProject] = useState<ParsedProject | null>(null);
  const [projectDir, setProjectDir] = useState(() => loadPersistedDir());
  const [watching, setWatching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [autoCollab, setAutoCollab] = useState(false);
  const [humanInLoop, setHumanInLoop] = useState(false);
  const [selectedRole, setSelectedRole] = useState<RoleStatus | null>(null);
  const [msgTo, setMsgTo] = useState("all");
  const [msgBody, setMsgBodyRaw] = useState(() => localStorage.getItem("vaak_compose_draft") || "");
  const setMsgBody = (v: string) => { setMsgBodyRaw(v); localStorage.setItem("vaak_compose_draft", v); };
  const [sending, setSending] = useState(false);
  const [workflowDropdownOpen, setWorkflowDropdownOpen] = useState(false);
  const [discussionModeOpen, setDiscussionModeOpen] = useState(false);
  const [discussionState, setDiscussionState] = useState<DiscussionState | null>(null);
  const [closingRound, setClosingRound] = useState(false);
  const [continuousTimeout, setContinuousTimeout] = useState(60);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [claimsCollapsed, setClaimsCollapsed] = useState(true);
  const [retentionDays, setRetentionDays] = useState(7);
  const [confirmAction, setConfirmAction] = useState<{ title: string; message: string; onConfirm: () => void; confirmLabel?: string } | null>(null);
  const [sections, setSections] = useState<Section[]>([]);
  const [activeSection, setActiveSection] = useState<string | null>(null);
  const [newSectionName, setNewSectionName] = useState("");
  const [creatingSectionMode, setCreatingSectionMode] = useState(false);
  const [savedProjects, setSavedProjects] = useState(() => loadSavedProjects());
  const [expandedProject, setExpandedProject] = useState<string | null>(null);
  const [projectSections, setProjectSections] = useState<Record<string, Section[]>>({});
  const workflowDropdownRef = useRef<HTMLDivElement>(null);
  const discussionModeRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messageTimelineRef = useRef<HTMLDivElement>(null);
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [newMsgCount, setNewMsgCount] = useState(0);
  const prevMsgCountRef = useRef(0);

  // Team Launcher state
  const [launching, setLaunching] = useState(false);
  const [claudeInstalled, setClaudeInstalled] = useState<boolean | null>(null);
  const [spawnConsented, setSpawnConsented] = useState(false);
  const [launchCooldown, setLaunchCooldown] = useState(false);

  // Add Roles panel state

  // Close workflow dropdown on click outside
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

  // Close discussion mode dropdown on click outside
  useEffect(() => {
    if (!discussionModeOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (discussionModeRef.current && !discussionModeRef.current.contains(e.target as Node)) {
        setDiscussionModeOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [discussionModeOpen]);

  // Poll sections list
  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    const pollSections = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<Section[]>("list_sections", { dir: projectDir });
        if (!cancelled) {
          setSections(result);
          const active = result.find(s => s.is_active);
          if (active) setActiveSection(active.slug);
        }
      } catch { /* command may not exist yet */ }
    };
    pollSections();
    const interval = setInterval(pollSections, 10000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [projectDir]);

  const handleCreateSection = async () => {
    if (!newSectionName.trim() || !projectDir) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const created = await invoke<Section>("create_section", {
        dir: projectDir,
        name: newSectionName.trim(),
      });
      // Switch to the new section
      await invoke("switch_section", { dir: projectDir, slug: created.slug });
      setActiveSection(created.slug);
      setNewSectionName("");
      setCreatingSectionMode(false);
      // Refresh sections list and messages
      const updated = await invoke<Section[]>("list_sections", { dir: projectDir });
      setSections(updated);
      // Reload project data for the new section
      const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
      if (result) setProject(result);
    } catch (e) {
      console.error("[CollabTab] Failed to create section:", e);
    }
  };

  const handleSwitchSection = async (slug: string) => {
    if (!projectDir || slug === activeSection) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("switch_section", { dir: projectDir, slug });
      setActiveSection(slug);
      // Reload project data for the new section (also resets watcher to track new section's board)
      const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
      if (result) setProject(result);
    } catch (e) {
      console.error("[CollabTab] Failed to switch section:", e);
    }
  };

  // Poll discussion state (independent of communication mode)
  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    const pollDiscussion = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const state = await invoke<DiscussionState | null>("get_discussion_state", { dir: projectDir });
        if (!cancelled) {
          setDiscussionState(state);
          // Sync continuous timeout from server state
          if (state?.settings?.auto_close_timeout_seconds != null) {
            setContinuousTimeout(state.settings.auto_close_timeout_seconds);
          }
        }
      } catch { /* command may not exist yet */ }
    };
    pollDiscussion();
    const interval = setInterval(pollDiscussion, 5000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [projectDir]);

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

  // Auto-reconnect: if we have a persisted projectDir, start watching automatically
  const autoReconnectRef = useRef(false);
  useEffect(() => {
    if (autoReconnectRef.current) return;
    if (!window.__TAURI__ || !projectDir || watching) return;
    autoReconnectRef.current = true;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<(ParsedProject & { effective_dir?: string }) | null>("watch_project_dir", { dir: projectDir });
        if (result) {
          if (result.effective_dir && result.effective_dir !== projectDir) {
            setProjectDir(result.effective_dir);
            persistDir(result.effective_dir);
          }
          setWatching(true);
          setProject(result);
          if (result.config?.settings?.message_retention_days != null) {
            setRetentionDays(result.config.settings.message_retention_days);
          }
        }
      } catch { /* project dir may no longer exist — show setup screen */ }
    })();
  }, []);

  // Check if Claude CLI is installed
  useEffect(() => {
    if (!window.__TAURI__) return;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const installed = await invoke<boolean>("check_claude_installed");
        setClaudeInstalled(installed);
      } catch { setClaudeInstalled(false); }
    })();
  }, []);

  // Team launcher handlers
  const handleLaunchMember = async (role: string) => {
    if (!spawnConsented) {
      setConfirmAction({
        title: "Launch Claude Code Agent",
        message: `This will launch Claude Code with full autonomous permissions (--dangerously-skip-permissions) in a new terminal window. The agent will join as "${role}" and work autonomously. Continue?`,
        confirmLabel: "Launch",
        onConfirm: async () => {
          setSpawnConsented(true);
          setConfirmAction(null);
          await doLaunchMember(role);
        },
      });
      return;
    }
    await doLaunchMember(role);
  };

  const doLaunchMember = async (role: string) => {
    setLaunchCooldown(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("launch_team_member", { projectDir, role });
      }
    } catch (e) {
      console.error("[CollabTab] Failed to launch team member:", e);
    }
    setTimeout(() => setLaunchCooldown(false), 3000);
  };

  const doLaunchTeam = async (roles: string[]) => {
    setLaunching(true);
    setLaunchCooldown(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("launch_team", { projectDir, roles });
      }
    } catch (e) {
      console.error("[CollabTab] Failed to launch team:", e);
    }
    setLaunching(false);
    setTimeout(() => setLaunchCooldown(false), 3000);
  };

  const handleKillMember = async (role: string, instance: number) => {
    setConfirmAction({
      title: "Remove team member",
      message: `Remove ${role}:${instance} from the team? This will close their terminal window.`,
      confirmLabel: "Remove",
      onConfirm: async () => {
        try {
          if (window.__TAURI__) {
            const { invoke } = await import("@tauri-apps/api/core");
            await invoke("kill_team_member", { role, instance });
          }
        } catch (e) {
          console.error("[CollabTab] Failed to kill team member:", e);
        }
        setConfirmAction(null);
      },
    });
  };


  // Roster management handlers
  const handleAddRosterSlot = async (role: string) => {
    if (!projectDir) return;
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("roster_add_slot", { projectDir, role });
        // Refresh project state so the new slot appears in the UI
        const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
        if (result) setProject(result);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to add roster slot:", e);
    }
  };

  const handleRemoveRosterSlot = async (role: string, instance: number) => {
    setConfirmAction({
      title: "Remove role from roster",
      message: `Remove ${role}:${instance} from the project roster? This will also disconnect any active agent in this slot.`,
      confirmLabel: "Remove",
      onConfirm: async () => {
        try {
          if (window.__TAURI__) {
            const { invoke } = await import("@tauri-apps/api/core");
            await invoke("roster_remove_slot", { projectDir, role, instance });
            // Refresh project state so the removed slot disappears from the UI
            const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
            if (result) setProject(result);
          }
        } catch (e) {
          console.error("[CollabTab] Failed to remove roster slot:", e);
        }
        setConfirmAction(null);
      },
    });
  };

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

  const handleSetDiscussionMode = async (mode: string) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_discussion_mode", {
          dir: projectDir,
          discussionMode: mode,
        });
        setDiscussionModeOpen(false);
        // Force re-read project to update UI immediately
        const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
        if (result) setProject(result);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to set discussion mode:", e);
    }
  };

  const handleCloseRound = async () => {
    setClosingRound(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("close_discussion_round", { dir: projectDir });
        const state = await invoke<DiscussionState | null>("get_discussion_state", { dir: projectDir });
        if (state) setDiscussionState(state);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to close round:", e);
    } finally {
      setClosingRound(false);
    }
  };

  const handleEndDiscussion = async () => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("end_discussion", { dir: projectDir });
        setDiscussionState(null);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to end discussion:", e);
    }
  };

  const handleSetContinuousTimeout = async (seconds: number) => {
    setContinuousTimeout(seconds);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_continuous_timeout", { dir: projectDir, timeoutSeconds: seconds });
      }
    } catch (e) {
      console.error("[CollabTab] Failed to set continuous timeout:", e);
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

  // Smart scroll: only auto-scroll if user is at bottom, otherwise show indicator
  useEffect(() => {
    const currentCount = project?.messages?.length || 0;
    const prevCount = prevMsgCountRef.current;
    const added = currentCount - prevCount;
    prevMsgCountRef.current = currentCount;

    if (added > 0) {
      if (isAtBottom) {
        messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
        setNewMsgCount(0);
      } else {
        setNewMsgCount((prev) => prev + added);
      }
    }
  }, [project?.messages?.length, isAtBottom]);

  // Track scroll position in message timeline
  useEffect(() => {
    const el = messageTimelineRef.current;
    if (!el) return;
    const handleScroll = () => {
      const threshold = 40;
      const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
      setIsAtBottom(atBottom);
      if (atBottom) setNewMsgCount(0);
    };
    el.addEventListener("scroll", handleScroll);
    return () => el.removeEventListener("scroll", handleScroll);
  }, [watching]);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    setNewMsgCount(0);
  };

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

  const fetchProjectSections = async (path: string) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<Section[]>("list_sections", { dir: path });
        setProjectSections(prev => ({ ...prev, [path]: result }));
      }
    } catch {
      setProjectSections(prev => ({ ...prev, [path]: [] }));
    }
  };

  const toggleProjectExpand = (path: string) => {
    if (expandedProject === path) {
      setExpandedProject(null);
    } else {
      setExpandedProject(path);
      fetchProjectSections(path);
    }
  };

  const startWatching = async (overrideDir?: string, sectionSlug?: string) => {
    const dir = overrideDir || projectDir;
    if (!dir.trim()) {
      setError("Please enter a project directory");
      return;
    }
    if (overrideDir) setProjectDir(overrideDir);
    setError(null);
    setLoading(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");

        // Try to read existing project
        let result = await invoke<(ParsedProject & { effective_dir?: string }) | null>("watch_project_dir", { dir });

        // No .vaak/ found — auto-create it
        if (!result) {
          const config = buildDefaultConfig(dir);
          await invoke("initialize_project", {
            dir,
            config: JSON.stringify(config),
          });
          // Re-read after creation
          result = await invoke<(ParsedProject & { effective_dir?: string }) | null>("watch_project_dir", { dir });
        }

        // Update projectDir if the backend found a better subdirectory
        const finalDir = (result?.effective_dir && result.effective_dir !== dir) ? result.effective_dir : dir;
        if (finalDir !== dir) {
          setProjectDir(finalDir);
        }
        persistDir(finalDir);
        addSavedProject(finalDir);
        setSavedProjects(loadSavedProjects());

        // Switch to specific section if requested
        if (sectionSlug) {
          await invoke("switch_section", { dir: finalDir, slug: sectionSlug });
          setActiveSection(sectionSlug);
          result = await invoke<(ParsedProject & { effective_dir?: string }) | null>("watch_project_dir", { dir: finalDir });
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
    persistDir("");
  };

  const sendMessage = async () => {
    if (!msgBody.trim() || !projectDir) return;
    const trimmed = msgBody.trim();

    // Slash command parsing for discussion control
    if (trimmed.startsWith("/")) {
      const parts = trimmed.split(/\s+/);
      const cmd = parts[0].toLowerCase();

      if (cmd === "/debate" || cmd === "/discuss") {
        // /debate [format] [@moderator] [topic...]
        // /debate delphi What should we do about X?
        // /debate oxford @tester How to restructure the UI?
        // /debate continuous
        const validFormats = ["delphi", "oxford", "continuous", "red_team"];
        const format = parts[1]?.toLowerCase();
        if (!format || !validFormats.includes(format)) {
          console.error("[CollabTab] Usage: /debate <delphi|oxford|continuous> [topic]");
          setMsgBody("");
          return;
        }

        let moderatorOverride: string | undefined;
        let topicStart = 2;

        // Check for @moderator
        if (parts[2]?.startsWith("@")) {
          const modRole = parts[2].slice(1); // remove @
          const activeSessions = project?.sessions?.filter(s => s.status === "active") || [];
          const modSession = activeSessions.find(s => s.role === modRole);
          if (modSession) {
            moderatorOverride = `${modSession.role}:${modSession.instance}`;
          } else {
            // Try exact match like @tester:1
            moderatorOverride = modRole;
          }
          topicStart = 3;
        }

        const topic = format === "continuous"
          ? "Continuous review — auto-triggered micro-rounds"
          : parts.slice(topicStart).join(" ") || "Open discussion";

        setSending(true);
        try {
          if (window.__TAURI__) {
            const { invoke } = await import("@tauri-apps/api/core");
            const activeSessions = project?.sessions?.filter(s => s.status === "active") || [];
            const defaultMod = activeSessions.find(s => s.role === "manager")
              ? `manager:${activeSessions.find(s => s.role === "manager")!.instance}`
              : activeSessions.length > 0
                ? `${activeSessions[0].role}:${activeSessions[0].instance}`
                : "human:0";
            const participants = activeSessions.map(s => `${s.role}:${s.instance}`);
            await invoke("start_discussion", {
              dir: projectDir,
              mode: format,
              topic,
              moderator: moderatorOverride || defaultMod,
              participants,
            });
            setMsgBody("");
            const state = await invoke<DiscussionState | null>("get_discussion_state", { dir: projectDir });
            if (state) setDiscussionState(state);
          }
        } catch (e) {
          console.error("[CollabTab] Failed to start discussion:", e);
        } finally {
          setSending(false);
        }
        return;
      }

      if (cmd === "/end-debate" || cmd === "/end-discussion") {
        setSending(true);
        try {
          if (window.__TAURI__) {
            const { invoke } = await import("@tauri-apps/api/core");
            await invoke("end_discussion", { dir: projectDir });
            setMsgBody("");
            const state = await invoke<DiscussionState | null>("get_discussion_state", { dir: projectDir });
            if (state) setDiscussionState(state);
          }
        } catch (e) {
          console.error("[CollabTab] Failed to end discussion:", e);
        } finally {
          setSending(false);
        }
        return;
      }

      if (cmd === "/close-round") {
        setSending(true);
        try {
          if (window.__TAURI__) {
            const { invoke } = await import("@tauri-apps/api/core");
            await invoke("close_discussion_round", { dir: projectDir });
            setMsgBody("");
            const state = await invoke<DiscussionState | null>("get_discussion_state", { dir: projectDir });
            if (state) setDiscussionState(state);
          }
        } catch (e) {
          console.error("[CollabTab] Failed to close round:", e);
        } finally {
          setSending(false);
        }
        return;
      }

      // Unknown slash command — fall through to regular send
    }

    // Regular message send
    setSending(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("send_team_message", {
          dir: projectDir,
          to: msgTo,
          subject: "",
          body: trimmed,
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
          <button className="project-back-btn" onClick={stopWatching} title="Back to projects">&larr;</button>
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
          {/* Visibility Mode Selector */}
          {(() => {
            const currentMode = project?.config?.settings?.discussion_mode || "directed";
            const modes: Record<string, { label: string; color: string; desc: string }> = {
              directed: { label: "Directed", color: "#1da1f2", desc: "Agents only see messages addressed to them" },
              open: { label: "Open", color: "#f5a623", desc: "All agents see all messages" },
            };
            const active = modes[currentMode] || modes.directed;
            return (
              <div className="discussion-mode-wrapper" ref={discussionModeRef}>
                <span
                  className="discussion-mode-badge"
                  style={{
                    background: `${active.color}22`,
                    color: active.color,
                    borderColor: `${active.color}55`,
                  }}
                  onClick={() => setDiscussionModeOpen(!discussionModeOpen)}
                  title="Visibility — controls whether agents see all messages or only ones addressed to them"
                >
                  {active.label}
                </span>
                {discussionModeOpen && (
                  <div className="discussion-mode-dropdown">
                    {Object.entries(modes).map(([id, m]) => (
                      <div
                        key={id}
                        className={`discussion-mode-dropdown-item${currentMode === id ? " discussion-mode-active" : ""}`}
                        onClick={() => handleSetDiscussionMode(id)}
                      >
                        <span className="discussion-mode-dropdown-dot" style={{ background: m.color }} />
                        <div className="discussion-mode-dropdown-info">
                          <span className="discussion-mode-dropdown-label">{m.label}</span>
                          <span className="discussion-mode-dropdown-desc">{m.desc}</span>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })()}
          <button
            className="project-settings-btn"
            onClick={() => setSettingsOpen(!settingsOpen)}
            title="Board settings"
          >
            &#9881;
          </button>
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

        {/* Section Tabs */}
        <div className="section-tabs">
          {sections.map(s => (
            <button
              key={s.slug}
              className={`section-tab${s.slug === activeSection ? " section-tab-active" : ""}`}
              onClick={() => handleSwitchSection(s.slug)}
            >
              <span className="section-tab-hash">#</span>
              <span className="section-tab-name">{s.name}</span>
              {s.message_count > 0 && (
                <span className="section-tab-count">{s.message_count}</span>
              )}
            </button>
          ))}
          {creatingSectionMode ? (
            <div className="section-tab-create">
              <input
                className="section-tab-create-input"
                type="text"
                placeholder="Name..."
                value={newSectionName}
                onChange={e => setNewSectionName(e.target.value)}
                onKeyDown={e => {
                  if (e.key === "Enter") handleCreateSection();
                  if (e.key === "Escape") { setCreatingSectionMode(false); setNewSectionName(""); }
                }}
                autoFocus
              />
              <button className="section-tab-create-ok" onClick={handleCreateSection} disabled={!newSectionName.trim()}>+</button>
              <button className="section-tab-create-cancel" onClick={() => { setCreatingSectionMode(false); setNewSectionName(""); }}>&times;</button>
            </div>
          ) : (
            <button className="section-tab section-tab-new" onClick={() => setCreatingSectionMode(true)}>
              + New
            </button>
          )}
        </div>

        {/* Section join hint for discoverability */}
        {sections.length > 1 && activeSection && (
          <div className="section-join-hint">
            Tell agents: <span className="section-join-hint-cmd">join as [role], section {activeSection}</span>
          </div>
        )}

        {/* Discussion Controls — compact bar when active, setup form when toggled */}
        {discussionState?.active ? (
          <div className="discussion-controls-bar">
            <div className="discussion-controls-info">
              <span className="discussion-controls-mode">
                {(discussionState.mode || "Discussion").charAt(0).toUpperCase() + (discussionState.mode || "").slice(1)}
              </span>
              <span className="discussion-controls-round">
                R{discussionState.current_round}
              </span>
              <span className="discussion-controls-phase">
                {closingRound ? "Aggregating..." :
                 discussionState.phase === "submitting" ? "Open" :
                 discussionState.phase === "reviewing" ? "Reviewing" :
                 discussionState.phase === "complete" ? "Done" :
                 discussionState.phase || ""}
              </span>
              {discussionState.moderator && (
                <span className="discussion-controls-moderator">
                  Mod: <span style={{ color: getRoleColor(discussionState.moderator.split(":")[0]) }}>{discussionState.moderator}</span>
                </span>
              )}
              {discussionState.phase === "submitting" && discussionState.rounds.length > 0 && (() => {
                const currentRound = discussionState.rounds[discussionState.rounds.length - 1];
                const submitted = currentRound?.submissions?.length || 0;
                const total = (discussionState.participants || []).filter(p => p !== discussionState.moderator).length;
                return <span className="discussion-controls-count">{submitted}/{total}</span>;
              })()}
              {discussionState.mode === "continuous" && (
                <select
                  className="discussion-controls-timeout"
                  value={continuousTimeout}
                  onChange={(e) => handleSetContinuousTimeout(Number(e.target.value))}
                >
                  <option value={30}>30s</option>
                  <option value={60}>60s</option>
                  <option value={120}>2m</option>
                  <option value={300}>5m</option>
                </select>
              )}
            </div>
            <div className="discussion-controls-actions">
              {discussionState.phase === "submitting" && discussionState.mode !== "continuous" && (
                <button className="discussion-controls-btn" onClick={handleCloseRound} disabled={closingRound}>
                  {closingRound ? "Closing..." : "Close Round"}
                </button>
              )}
              <button className="discussion-controls-btn discussion-controls-end" onClick={handleEndDiscussion}>
                End
              </button>
            </div>
          </div>
        ) : null}

        {/* Add to Team — compact toolbar row */}
        {project && (
          <div className="add-team-toolbar">
            {Object.entries(project.config.roles)
              .sort(([a], [b]) => (ROLE_ORDER[a] ?? 99) - (ROLE_ORDER[b] ?? 99))
              .map(([slug, role]) => {
                const roster = project.config.roster || [];
                const slotsForRole = roster.filter((s: RosterSlot) => s.role === slug).length;
                return (
                  <button
                    key={slug}
                    className="add-team-btn"
                    onClick={() => handleAddRosterSlot(slug)}
                    title={`Add ${role.title} to team — ${role.description}`}
                    aria-label={`Add ${role.title}, ${slotsForRole} currently on team`}
                  >
                    <span className="add-team-btn-dot" style={{ background: getRoleColor(slug) }} />
                    <span className="add-team-btn-label">+ {role.title}</span>
                    {slotsForRole > 0 && <span className="add-team-btn-count">{slotsForRole}</span>}
                  </button>
                );
              })}
          </div>
        )}

        {/* Team Roster — shows all roster slots with status */}
        {project && (() => {
          const timeoutSecs = project.config?.settings?.heartbeat_timeout_seconds || 120;
          // Use ALL sessions for roster status (team is project-wide, not section-specific)
          const cards = buildRosterCards(
            project.config.roster,
            project.config.roles,
            project.role_statuses,
            project.sessions,
            timeoutSecs
          );
          const hasRoster = project.config.roster && project.config.roster.length > 0;
          const vacantCount = cards.filter(c => c.status === "vacant").length;
          return (
            <>
              {cards.length > 0 && (
                <div className="project-roles-grid">
                  {cards.map((card) => {
                    const cardKey = `${card.slug}:${card.instance}`;
                    const matchingRole = project.role_statuses.find((r) => r.slug === card.slug);
                    return (
                      <div
                        key={cardKey}
                        className={`role-card role-card-clickable ${card.status === "working" ? "role-card-working" : ""} ${card.status === "vacant" ? "role-card-vacant" : ""}`}
                        style={{ borderLeftColor: card.roleColor }}
                        onClick={() => matchingRole && setSelectedRole(matchingRole)}
                      >
                        {/* Remove agent button (x) — disconnect agent, slot stays vacant */}
                        {card.status !== "vacant" && (
                          <button
                            className="role-remove-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              const inst = card.instance >= 0 ? card.instance : 0;
                              handleKillMember(card.slug, inst);
                            }}
                            title={`Disconnect ${card.title} agent`}
                            aria-label={`Disconnect ${card.title} agent`}
                          >&times;</button>
                        )}
                        {/* Remove slot button (trash) — removes from roster entirely */}
                        {hasRoster && card.status === "vacant" && (
                          <button
                            className="role-slot-remove-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleRemoveRosterSlot(card.slug, card.instance >= 0 ? card.instance : 0);
                            }}
                            title={`Remove ${card.title} slot from roster`}
                            aria-label={`Remove ${card.title} slot from roster`}
                          >&#128465;</button>
                        )}
                        <div className="role-card-header">
                          <span className={getStatusDotClass(card.status)} />
                          <span className="role-card-title" style={{ color: card.roleColor }}>
                            {card.title}
                          </span>
                        </div>
                        <div className="role-card-meta">
                          <span className="role-card-slug">{card.slug}</span>
                          <span className={`role-card-status role-card-status-${card.status}`}>
                            {card.status}
                          </span>
                        </div>
                        {/* Launch button for vacant slots */}
                        {card.status === "vacant" && claudeInstalled !== false && (
                          <button
                            className="role-card-launch-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleLaunchMember(card.slug);
                            }}
                            disabled={launchCooldown}
                            title={`Launch Claude agent as ${card.title}`}
                            aria-label={`Launch Claude agent as ${card.title}`}
                          >Launch</button>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}

              {/* Launch All Vacant button */}
              {vacantCount > 1 && claudeInstalled !== false && (
                <button
                  className="launch-team-btn"
                  onClick={() => {
                    const vacantRoles = cards.filter(c => c.status === "vacant").map(c => c.slug);
                    if (!spawnConsented) {
                      setConfirmAction({
                        title: "Launch All Vacant",
                        message: `This will launch ${vacantRoles.length} Claude Code agent(s) with full autonomous permissions (--dangerously-skip-permissions) in new terminal windows. Continue?`,
                        confirmLabel: "Launch",
                        onConfirm: async () => {
                          setSpawnConsented(true);
                          setConfirmAction(null);
                          await doLaunchTeam(vacantRoles);
                        },
                      });
                    } else {
                      doLaunchTeam(vacantRoles);
                    }
                  }}
                  disabled={launching || launchCooldown}
                >
                  {launching && <span className="launch-team-spinner" />}
                  {launching ? "Launching..." : `Launch All Vacant (${vacantCount})`}
                </button>
              )}
            </>
          );
        })()}

        {/* Active Claims Section — collapsible */}
        {project && project.claims && project.claims.length > 0 && (
          <div className={`claims-section${claimsCollapsed ? " claims-collapsed" : ""}`}>
            <div className="claims-section-title" onClick={() => setClaimsCollapsed(!claimsCollapsed)}>
              <span className="claims-section-toggle">&#9660;</span>
              Active Claims <span className="claims-section-count">({project.claims.length})</span>
            </div>
            <div className="claims-section-body">
              {project.claims.map((claim: FileClaim) => {
                const roleSlug = claim.role_instance.split(":")[0] || "";
                const filesDisplay = claim.files.length > 2
                  ? `${claim.files[0]} (+${claim.files.length - 1} more)`
                  : claim.files.join(", ");
                const claimAgeSec = (Date.now() - new Date(claim.claimed_at).getTime()) / 1000;
                const isStale = claimAgeSec > 900; // >15 minutes
                return (
                  <div key={claim.role_instance} className={`claim-card${isStale ? " claim-stale" : ""}`}>
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
        <div className="message-timeline" ref={messageTimelineRef}>
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

              // Discussion events render as distinct inline cards
              if (msg.type === "moderation" && msg.metadata?.discussion_action) {
                const action = msg.metadata.discussion_action as string;
                const isAggregate = action === "aggregate" || action === "auto_aggregate";
                const isStart = action === "start";
                const isEnd = action === "end";
                return (
                  <div key={msg.id} className={`discussion-event-card${isAggregate ? " event-aggregate" : isStart ? " event-start" : isEnd ? " event-end" : ""}`}>
                    <div className="discussion-event-header">
                      <span className="discussion-event-icon">
                        {isStart ? "\uD83D\uDDE3\uFE0F" : isEnd ? "\uD83C\uDFC1" : isAggregate ? "\uD83D\uDCCA" : "\u2139\uFE0F"}
                      </span>
                      <span className="discussion-event-label">
                        {isStart ? "Discussion Started" : isEnd ? "Discussion Ended" : isAggregate ? `Round ${msg.metadata.round || "?"} Aggregate` : msg.subject}
                      </span>
                      <span className="message-card-time" title={msg.timestamp}>{formatRelativeTime(msg.timestamp)}</span>
                    </div>
                    <div className="discussion-event-body">{msg.body}</div>
                  </div>
                );
              }

              // Submissions render with a distinct visual style
              if (msg.type === "submission") {
                const fromRole = msg.from.split(":")[0];
                return (
                  <div key={msg.id} className="submission-card" style={{ borderLeftColor: getRoleColor(fromRole) }}>
                    <div className="message-card-header">
                      <span className="message-card-id">#{msg.id}</span>
                      <span className="message-card-from" style={{ color: getRoleColor(fromRole) }}>{msg.from}</span>
                      <span className="message-card-arrow">&rarr;</span>
                      <span className="message-card-to" style={{ color: getRoleColor(msg.to) }}>{msg.to}</span>
                      <span className="message-type-badge badge-submission">submission</span>
                      <span className="message-card-time" title={msg.timestamp}>{formatRelativeTime(msg.timestamp)}</span>
                    </div>
                    {msg.subject && <div className="message-card-subject">{msg.subject}</div>}
                    <div className="message-card-body">{msg.body}</div>
                  </div>
                );
              }

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

        {/* New messages indicator */}
        {newMsgCount > 0 && (
          <button className="new-messages-indicator" onClick={scrollToBottom}>
            {newMsgCount} new message{newMsgCount !== 1 ? "s" : ""} &darr;
          </button>
        )}

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
            placeholder={`Message${activeSection ? ` in #${sections.find(s => s.slug === activeSection)?.name || activeSection}` : ""}... (/debate delphi [topic])`}
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
                <button className="confirm-dialog-delete" onClick={confirmAction.onConfirm}>{confirmAction.confirmLabel || "Delete"}</button>
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
          Add a project to see AI agent roles and their messages in real time.
        </div>

        <div className="saved-projects">
          <div className="saved-projects-title">My Projects</div>
          {savedProjects.length > 0 ? (
            savedProjects.map((proj) => {
              const isExpanded = expandedProject === proj.path;
              const secs = projectSections[proj.path];
              return (
                <div key={proj.path} className="saved-project-entry">
                  <div className="saved-project-item">
                    <button
                      className="saved-project-expand"
                      onClick={() => toggleProjectExpand(proj.path)}
                      title={isExpanded ? "Collapse" : "Show sections"}
                    >
                      {isExpanded ? "\u25BC" : "\u25B6"}
                    </button>
                    <button
                      className="saved-project-btn"
                      onClick={() => startWatching(proj.path)}
                      disabled={loading}
                    >
                      <span className="saved-project-name">{proj.name}</span>
                      <span className="saved-project-path">{proj.path}</span>
                    </button>
                    <button
                      className="saved-project-remove"
                      onClick={(e) => {
                        e.stopPropagation();
                        removeSavedProject(proj.path);
                        setSavedProjects(loadSavedProjects());
                      }}
                      title="Remove project"
                    >&times;</button>
                  </div>
                  {isExpanded && (
                    <div className="saved-project-sections">
                      {!secs ? (
                        <div className="saved-project-sections-loading">Loading sections...</div>
                      ) : secs.length === 0 ? (
                        <div className="saved-project-sections-empty">Click project name to set up</div>
                      ) : (
                        secs.map(s => (
                          <button
                            key={s.slug}
                            className={`saved-project-section-btn${s.is_active ? " section-active" : ""}`}
                            onClick={() => startWatching(proj.path, s.slug)}
                            disabled={loading}
                          >
                            <span className="saved-project-section-hash">#</span>
                            <span className="saved-project-section-name">{s.name}</span>
                            {s.message_count > 0 && (
                              <span className="saved-project-section-count">{s.message_count}</span>
                            )}
                          </button>
                        ))
                      )}
                    </div>
                  )}
                </div>
              );
            })
          ) : (
            <div className="saved-projects-empty">No projects added yet. Click below to get started.</div>
          )}
          <button
            className="saved-project-add-btn"
            onClick={async () => {
              try {
                if (window.__TAURI__) {
                  const { open } = await import("@tauri-apps/plugin-dialog");
                  const selected = await open({ directory: true, multiple: false });
                  if (selected) {
                    addSavedProject(selected as string);
                    setSavedProjects(loadSavedProjects());
                    startWatching(selected as string);
                  }
                }
              } catch { /* user cancelled */ }
            }}
            disabled={loading}
          >
            + Add Project
          </button>
          {error && <div className="project-watch-error" style={{ marginTop: 8 }}>{error}</div>}
        </div>
      </div>
    </div>
  );
}
