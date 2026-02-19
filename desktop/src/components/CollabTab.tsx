import { useEffect, useState, useRef } from "react";
import type { ParsedProject, BoardMessage, RoleStatus, SessionBinding, QuestionChoice, FileClaim, DiscussionState, Section, RosterSlot, RoleConfig, RoleGroup } from "../lib/collabTypes";
import { BUILTIN_ROLE_GROUPS } from "../utils/roleGroupPresets";
import { RoleBriefingModal } from "./RoleBriefingModal";
import { getAvailableVoices, fetchAvailableVoices, getDefaultVoice } from "../lib/queueStore";
import { CANONICAL_TAGS, ROLE_TEMPLATES, generateBriefing, type PeerRole, type RoleTemplate } from "../utils/briefingGenerator";
import { trimVoiceAssignments } from "../lib/storageManager";
import "../styles/collab.css";

const ROLE_COLORS: Record<string, string> = {
  manager: "#9b59b6",
  architect: "#1da1f2",
  developer: "#17bf63",
  tester: "#f5a623",
  audience: "#e74c3c",
  user: "#e1e8ed",
};

// Palette for dynamically-created roles — deterministic via FNV-1a slug hash
const HASH_PALETTE = [
  "#e91e63", // pink
  "#00bcd4", // cyan
  "#ff7043", // deep orange
  "#8bc34a", // lime green
  "#7e57c2", // deep purple
  "#26a69a", // teal
  "#ec407a", // rose
  "#42a5f5", // sky blue
  "#ffa726", // amber
  "#66bb6a", // medium green
  "#ef5350", // coral
  "#ab47bc", // orchid
];

function hashSlug(slug: string): number {
  let hash = 2166136261;
  for (let i = 0; i < slug.length; i++) {
    hash ^= slug.charCodeAt(i);
    hash = Math.imul(hash, 16777619) & 0xffffffff;
  }
  return hash >>> 0;
}

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
  return HASH_PALETTE[hashSlug(slug) % HASH_PALETTE.length];
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

  // Heartbeat comes every 30s, but agents in project_wait block up to 55s
  // between heartbeats. Use 180s threshold to avoid false "vacant" during waits.
  const goneThreshold = Math.min(timeoutSecs, 180);
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
  if (age > 120) return "vacant";
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
        (s) => s.role === slot.role && s.instance === slot.instance && s.status !== "gone"
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
      (s) => s.status !== "gone" && !coveredSessions.has(`${s.role}:${s.instance}`)
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
  onPlay,
  playingMsgId,
}: {
  msg: BoardMessage;
  answered: { choiceId: string } | null;
  onAnswer: (choiceId: string, choiceLabel: string) => void;
  onDelete?: (id: number) => void;
  onPlay?: (msgId: number, text: string, fromRole: string) => void;
  playingMsgId?: number | null;
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
        {onPlay && <button className="message-play-btn" onClick={(e) => { e.stopPropagation(); onPlay(msg.id, `${msg.subject || ""}. ${msg.body}`, fromRole); }} title={playingMsgId === msg.id ? "Stop" : "Play"}>{playingMsgId === msg.id ? "\u23F9" : "\u25B6"}</button>}
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

function normalizePath(p: string): string {
  return p.replace(/^\\\\\?\\/, "").replace(/[\\/]+$/, "").replace(/\\/g, "/").toLowerCase();
}

function addSavedProject(path: string, name?: string): void {
  try {
    const norm = normalizePath(path);
    const projects = loadSavedProjects().filter(p => normalizePath(p.path) !== norm);
    const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
    const autoName = name || parts[parts.length - 1] || "My Project";
    projects.unshift({ name: autoName, path, addedAt: new Date().toISOString() });
    // Cap saved projects to 20
    const capped = projects.slice(0, 20);
    localStorage.setItem(SAVED_PROJECTS_KEY, JSON.stringify(capped));
  } catch { /* ignore */ }
}

function removeSavedProject(path: string): void {
  try {
    const norm = normalizePath(path);
    const projects = loadSavedProjects().filter(p => normalizePath(p.path) !== norm);
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
  const MAX_DRAFT_LENGTH = 50000; // 50KB cap to prevent unbounded localStorage growth
  const setMsgBody = (v: string) => {
    const capped = v.length > MAX_DRAFT_LENGTH ? v.slice(0, MAX_DRAFT_LENGTH) : v;
    setMsgBodyRaw(capped);
    localStorage.setItem("vaak_compose_draft", capped);
  };
  const [sending, setSending] = useState(false);
  const [workflowDropdownOpen, setWorkflowDropdownOpen] = useState(false);
  const [discussionModeOpen, setDiscussionModeOpen] = useState(false);
  const [discussionState, setDiscussionState] = useState<DiscussionState | null>(null);
  const [closingRound, setClosingRound] = useState(false);
  const [continuousTimeout, setContinuousTimeout] = useState(60);
  const [startDiscussionOpen, setStartDiscussionOpen] = useState(false);
  const [sdFormat, setSdFormat] = useState<"delphi" | "oxford" | "red_team" | "continuous">("delphi");
  const [sdTopic, setSdTopic] = useState("");
  const [sdParticipants, setSdParticipants] = useState<Record<string, boolean>>({});
  const [sdStarting, setSdStarting] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [interruptTarget, setInterruptTarget] = useState<{ slug: string; instance: number; title: string } | null>(null);
  const [interruptReason, setInterruptReason] = useState("");
  const [buzzedKey, setBuzzedKey] = useState<string | null>(null);
  const [claimsCollapsed, setClaimsCollapsed] = useState(true);
  const [_addTeamTab, _setAddTeamTab] = useState<"groups" | "roles">("groups");
  const [expandedGroup, setExpandedGroup] = useState<string | null>(null);
  const [groupRoleChecked, setGroupRoleChecked] = useState<Record<string, boolean>>({});
  const [groupSearch, setGroupSearch] = useState("");
  const [rosterViewMode, setRosterViewMode] = useState<"grid" | "list">("grid");
  const [treeExpanded, setTreeExpanded] = useState<Set<string>>(new Set());
  const [teamSectionOpen, setTeamSectionOpen] = useState(false);
  const [createGroupOpen, setCreateGroupOpen] = useState(false);
  const [newGroupName, setNewGroupName] = useState("");
  const [newGroupIcon, setNewGroupIcon] = useState("\uD83D\uDCE6");
  const [newGroupDesc, setNewGroupDesc] = useState("");
  const [newGroupRoles, setNewGroupRoles] = useState<Record<string, number>>({});
  const [newGroupParent, setNewGroupParent] = useState<string | null>(null);
  const [retentionDays, setRetentionDays] = useState(7);
  const [confirmAction, setConfirmAction] = useState<{ title: string; message: string; onConfirm: () => void; confirmLabel?: string; requireTyped?: string } | null>(null);
  const [confirmTypedInput, setConfirmTypedInput] = useState("");
  const [companionLaunch, setCompanionLaunch] = useState<{ role: string; instance: number; companions: { role: string; enabled: boolean; optional: boolean }[] } | null>(null);
  const [sections, setSections] = useState<Section[]>([]);
  const [activeSection, setActiveSection] = useState<string | null>(null);
  const [newSectionName, setNewSectionName] = useState("");
  const [creatingSectionMode, setCreatingSectionMode] = useState(false);
  const [sectionLoading, setSectionLoading] = useState(false);
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
  const MSG_PAGE_SIZE = 50;
  const [visibleMsgLimit, setVisibleMsgLimit] = useState(MSG_PAGE_SIZE);

  // Team Launcher state
  const [launching, setLaunching] = useState(false);
  const [claudeInstalled, setClaudeInstalled] = useState<boolean | null>(null);
  const [spawnConsented, setSpawnConsented] = useState(false);
  const [launchCooldown, setLaunchCooldown] = useState(false);

  // Role CRUD state
  const AVAILABLE_PERMISSIONS = ["broadcast", "review", "assign_tasks", "status", "question", "handoff", "moderation"];
  const BUILT_IN_ROLES = ["developer", "manager", "architect", "tester", "moderator"];
  const [_globalTemplates, _setGlobalTemplates] = useState<Set<string>>(new Set());

  const PERM_TOOLTIPS: Record<string, string> = {
    broadcast: "Send messages to all team members",
    review: "Review and approve others' work",
    assign_tasks: "Assign tasks to team members",
    status: "Post status updates",
    question: "Ask questions to team members",
    handoff: "Hand off work to other roles",
    moderation: "Moderate structured discussions",
  };

  /** Build PeerRole[] from project config for briefing generation */
  const buildPeers = (excludeSlug?: string): PeerRole[] => {
    if (!project) return [];
    return Object.entries(project.config.roles)
      .filter(([slug]) => slug !== excludeSlug)
      .map(([slug, role]) => ({
        slug,
        title: role.title,
        description: role.description,
        tags: role.tags || [],
        permissions: role.permissions,
      }));
  };

  /** Wrapper to call the utility's generateBriefing with project context */
  const generateBriefingFromForm = (title: string, description: string, tags: string[], permissions: string[]): string => {
    return generateBriefing({ title, description, tags, permissions, peers: buildPeers(roleFormSlug), maxInstances: roleFormMaxInst });
  };

  const [roleFormOpen, setRoleFormOpen] = useState(false);
  const [roleFormEditing, setRoleFormEditing] = useState<string | null>(null);
  const [roleFormTitle, setRoleFormTitle] = useState("");
  const [roleFormDesc, setRoleFormDesc] = useState("");
  const [roleFormPerms, setRoleFormPerms] = useState<string[]>(["status", "question"]);
  const [roleFormMaxInst, setRoleFormMaxInst] = useState(999);
  const [roleFormBriefing, setRoleFormBriefing] = useState("");
  const [roleFormTags, setRoleFormTags] = useState<string[]>([]);
  const [roleFormCustomTag, setRoleFormCustomTag] = useState("");
  const [roleFormSaving, setRoleFormSaving] = useState(false);
  const [roleFormError, setRoleFormError] = useState<string | null>(null);
  const [roleFormStep, setRoleFormStep] = useState(0);
  // Guided description sub-questions
  const [roleFormPurpose, setRoleFormPurpose] = useState("");
  const [roleFormBoundaries, setRoleFormBoundaries] = useState("");
  const [roleFormDifferentiator, setRoleFormDifferentiator] = useState("");
  // Role creation mode: null = show choice screen, "wizard" = manual form, "interview" = AI chat
  const [roleCreationMode, setRoleCreationMode] = useState<"wizard" | "interview" | null>(null);
  // LLM interview chat state
  const [interviewMessages, setInterviewMessages] = useState<{ role: "user" | "assistant"; content: string }[]>([]);
  const [interviewInput, setInterviewInput] = useState("");
  const [interviewLoading, setInterviewLoading] = useState(false);
  const [interviewConfig, setInterviewConfig] = useState<{
    title: string; slug: string; description: string; tags: string[];
    permissions: string[]; max_instances: number; briefing: string;
  } | null>(null);
  const interviewChatRef = useRef<HTMLDivElement>(null);

  const WIZARD_STEPS = roleFormEditing
    ? ["Name", "Description", "Capabilities", "Permissions", "Instances", "Briefing"]
    : ["Template", "Name", "Description", "Capabilities", "Permissions", "Instances", "Briefing"];

  /** Smart permission defaults based on selected tags */
  const smartPermsFromTags = (tags: string[]): string[] => {
    const perms = new Set<string>();
    for (const tag of tags) {
      if (tag === "coordination") { perms.add("assign_tasks"); perms.add("status"); }
      if (tag === "moderation") { perms.add("broadcast"); perms.add("moderation"); }
      if (tag === "code-review" || tag === "architecture") { perms.add("review"); }
      if (tag === "implementation" || tag === "debugging") { perms.add("status"); perms.add("handoff"); }
      if (tag === "testing") { perms.add("status"); perms.add("question"); }
      if (tag === "analysis" || tag === "documentation") { perms.add("status"); }
      if (tag === "security" || tag === "red-team") { perms.add("status"); perms.add("review"); }
    }
    if (perms.size === 0) { perms.add("status"); perms.add("question"); }
    return Array.from(perms);
  };

  /** Smart instance count based on tags */
  const smartInstancesFromTags = (tags: string[]): number => {
    if (tags.includes("implementation")) return 3;
    return 1;
  };

  /** Compose description from guided sub-questions */
  const composeDescription = (): string => {
    const parts: string[] = [];
    if (roleFormPurpose.trim()) parts.push(roleFormPurpose.trim());
    if (roleFormBoundaries.trim()) parts.push(`Boundaries: ${roleFormBoundaries.trim()}`);
    if (roleFormDifferentiator.trim()) parts.push(`Differentiator: ${roleFormDifferentiator.trim()}`);
    return parts.join(". ") || roleFormDesc;
  };

  /** Tag scenario hints — when you'd select each tag */
  const TAG_SCENARIOS: Record<string, string> = {
    implementation: "Select if this role will write or modify source code files.",
    "code-review": "Select if this role should review others' code before it's approved.",
    testing: "Select if this role should write tests or validate that things work.",
    architecture: "Select if this role should design system structure and enforce patterns.",
    moderation: "Select if this role should facilitate debates and enforce discussion rules.",
    security: "Select if this role should look for vulnerabilities and security issues.",
    compliance: "Select if this role should ensure regulatory or policy requirements are met.",
    analysis: "Select if this role should research, investigate, and produce reports.",
    coordination: "Select if this role should assign work and manage priorities.",
    "red-team": "Select if this role should actively try to break or find flaws in designs.",
    documentation: "Select if this role should write docs, specs, or guides.",
    debugging: "Select if this role should diagnose and fix bugs.",
  };

  /** Permission consequence explanations */
  const PERM_CONSEQUENCES: Record<string, string> = {
    broadcast: "Without this, the role can only message specific roles, not the whole team.",
    review: "Without this, the role cannot formally approve or reject others' work.",
    assign_tasks: "Without this, the role cannot delegate work to other team members.",
    status: "Without this, the role cannot post progress updates to the team.",
    question: "Without this, the role cannot ask clarifying questions to others.",
    handoff: "Without this, the role cannot formally pass completed work to the next role.",
    moderation: "Without this, the role cannot control structured discussions or debates.",
  };

  /** Can advance to next step? */
  const canAdvanceStep = (): boolean => {
    const stepLabel = WIZARD_STEPS[roleFormStep];
    if (stepLabel === "Name") return !!roleFormTitle.trim();
    return true;
  };


  const roleFormSlug = roleFormEditing || roleFormTitle.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");

  const resetRoleForm = () => {
    setRoleFormTitle("");
    setRoleFormDesc("");
    setRoleFormPerms(["status", "question"]);
    setRoleFormMaxInst(999);
    setRoleFormBriefing("");
    setRoleFormTags([]);
    setRoleFormCustomTag("");
    setRoleFormEditing(null);
    setRoleFormError(null);
    setRoleFormStep(0);
    setRoleFormPurpose("");
    setRoleFormBoundaries("");
    setRoleFormDifferentiator("");
    setRoleCreationMode(null);
    setInterviewMessages([]);
    setInterviewInput("");
    setInterviewLoading(false);
    setInterviewConfig(null);
  };

  const applyTemplate = (template: RoleTemplate) => {
    setRoleFormTitle(template.title);
    setRoleFormDesc(template.description);
    setRoleFormPerms([...template.permissions]);
    setRoleFormMaxInst(template.maxInstances);
    setRoleFormTags([...template.tags]);
    setRoleFormBriefing(generateBriefingFromForm(template.title, template.description, template.tags, template.permissions));
    // Pre-fill purpose from template description so the guided questions aren't empty
    setRoleFormPurpose(template.description);
    setRoleFormBoundaries("");
    setRoleFormDifferentiator("");
    setRoleFormStep(1); // advance past template step
  };

  const addCustomTag = () => {
    const tag = roleFormCustomTag.toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-|-$/g, "");
    if (tag && !roleFormTags.includes(tag)) {
      setRoleFormTags([...roleFormTags, tag]);
    }
    setRoleFormCustomTag("");
  };

  const openCreateRoleForm = () => {
    resetRoleForm();
    setRoleCreationMode(null); // show choice screen first
    setRoleFormStep(0);
    setRoleFormOpen(true);
  };

  const openEditRoleForm = async (slug: string) => {
    if (!project) return;
    const role = project.config.roles[slug];
    if (!role) return;
    setRoleFormEditing(slug);
    setRoleFormTitle(role.title);
    setRoleFormDesc(role.description);
    setRoleFormPerms([...role.permissions]);
    setRoleFormMaxInst(role.max_instances);
    setRoleFormTags([...(role.tags || [])]);
    setRoleFormError(null);
    setRoleCreationMode("wizard"); // edit always uses wizard mode
    setRoleFormStep(0); // edit mode starts at "Name" (step 0 in edit WIZARD_STEPS)
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        const briefing = await invoke<string>("read_role_briefing", { dir: projectDir, roleSlug: slug });
        setRoleFormBriefing(briefing);
      }
    } catch {
      setRoleFormBriefing("");
    }
    setRoleFormOpen(true);
  };

  const handleRoleFormSave = async () => {
    if (!projectDir) return;
    setRoleFormSaving(true);
    setRoleFormError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      if (roleFormEditing) {
        await invoke("update_role", {
          projectDir,
          slug: roleFormEditing,
          title: roleFormTitle || null,
          description: roleFormDesc || null,
          permissions: roleFormPerms,
          maxInstances: roleFormMaxInst,
          briefing: roleFormBriefing || null,
          tags: roleFormTags,
        });
      } else {
        if (!roleFormSlug) {
          setRoleFormError("Title is required");
          setRoleFormSaving(false);
          return;
        }
        const briefing = roleFormBriefing || generateBriefingFromForm(roleFormTitle, roleFormDesc, roleFormTags, roleFormPerms);
        await invoke("create_role", {
          projectDir,
          slug: roleFormSlug,
          title: roleFormTitle,
          description: roleFormDesc,
          permissions: roleFormPerms,
          maxInstances: roleFormMaxInst,
          briefing,
          tags: roleFormTags,
        });
      }
      setRoleFormOpen(false);
      resetRoleForm();
    } catch (e) {
      setRoleFormError(String(e));
    } finally {
      setRoleFormSaving(false);
    }
  };

  const handleDeleteRole = async (slug: string) => {
    if (BUILT_IN_ROLES.includes(slug)) return;
    const roleTitle = project?.config?.roles?.[slug]?.title || slug;
    setConfirmTypedInput("");
    setConfirmAction({
      title: "Delete Custom Role",
      message: `Permanently delete the "${roleTitle}" role? This removes it from the project, deletes its briefing file, and removes all roster entries. This cannot be undone.\n\nType "${slug}" to confirm:`,
      confirmLabel: "Delete Role",
      requireTyped: slug,
      onConfirm: async () => {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("delete_role", { projectDir, slug });
          setConfirmAction(null);
        } catch (e) {
          setConfirmAction(null);
          setRoleFormError(String(e));
        }
      },
    });
  };

  /** Send a message in the LLM role interview chat */
  const sendInterviewMessage = async () => {
    const msg = interviewInput.trim();
    if (!msg || interviewLoading) return;
    const newMessages = [...interviewMessages, { role: "user" as const, content: msg }];
    setInterviewMessages(newMessages);
    setInterviewLoading(true);
    try {
      const projectContext = project ? {
        roles: Object.fromEntries(
          Object.entries(project.config.roles).map(([slug, role]) => [slug, {
            title: role.title, description: role.description,
            tags: role.tags || [], permissions: role.permissions,
          }])
        ),
      } : { roles: {} };
      const apiUrl = (import.meta as any).env?.VITE_API_URL || "http://127.0.0.1:19836";
      const res = await fetch(`${apiUrl}/api/v1/roles/design`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ messages: newMessages, project_context: projectContext }),
      });
      if (!res.ok) throw new Error(`Server error: ${res.status}`);
      const data = await res.json();
      setInterviewMessages([...newMessages, { role: "assistant", content: data.reply }]);
      if (data.role_config) {
        setInterviewConfig(data.role_config);
      }
      setTimeout(() => interviewChatRef.current?.scrollTo(0, interviewChatRef.current.scrollHeight), 50);
    } catch (e) {
      setRoleFormError(`Interview error: ${e}`);
      setInterviewMessages(interviewMessages);
    } finally {
      setInterviewLoading(false);
    }
  };

  /** Create role from interview-generated config */
  const handleInterviewCreate = async () => {
    if (!projectDir || !interviewConfig) return;
    setRoleFormSaving(true);
    setRoleFormError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const cfg = interviewConfig;
      // Inject multi-instance coordination rules if max_instances > 1
      let briefing = cfg.briefing;
      if (cfg.max_instances > 1) {
        briefing += `\n\n## Multi-Instance Coordination

When multiple instances of this role are active:
1. ALWAYS check \`project_claims\` before starting ANY file work
2. If another instance already claimed the files you need, pick a different task or coordinate via \`project_send\`
3. When a task is addressed to your role generically, the FIRST instance to claim files owns it — others wait
4. NEVER work on the same file as another instance of your role
5. If you see a generic directive, check if another instance already started before beginning
`;
      }
      await invoke("create_role", {
        projectDir,
        slug: cfg.slug,
        title: cfg.title,
        description: cfg.description,
        permissions: cfg.permissions,
        maxInstances: cfg.max_instances,
        briefing,
        tags: cfg.tags,
      });
      setRoleFormOpen(false);
      resetRoleForm();
    } catch (e) {
      setRoleFormError(String(e));
    } finally {
      setRoleFormSaving(false);
    }
  };

  // Audio playback state for message play buttons
  const [audioCache] = useState<Map<number, Blob>>(() => new Map());
  const [playingMsgId, setPlayingMsgId] = useState<number | null>(null);
  const currentAudioRef = useRef<HTMLAudioElement | null>(null);

  // Voice assignment per role for collab messages
  const [collabVoices, setCollabVoices] = useState<Record<string, string>>(() => {
    try {
      const saved = localStorage.getItem("vaak_collab_voice_assignments");
      return saved ? JSON.parse(saved) : {};
    } catch { return {}; }
  });
  const [voiceList, setVoiceList] = useState<{ voice_id: string; name: string }[]>([]);

  // Audience voting state
  interface AudiencePersona {
    name: string;
    background: string;
    values: string;
    style: string;
    provider: string;
    pool: string;
  }
  interface AudienceVoteResult {
    persona: string;
    background: string;
    provider: string;
    pool: string;
    model: string;
    vote: string;
    rationale: string;
    latency_ms: number;
    error: string | null;
  }
  interface AudienceResults {
    topic: string;
    phase: string;
    pool: string;
    total_voters: number;
    tally: Record<string, number>;
    tally_by_provider: Record<string, Record<string, number>>;
    tally_by_pool?: Record<string, Record<string, number>>;
    votes: AudienceVoteResult[];
    total_latency_ms: number;
  }
  interface AudiencePool {
    id: string;
    name: string;
    description: string;
    member_count: number;
    providers: string[];
  }
  const [audiencePanelOpen, setAudiencePanelOpen] = useState(false);
  const [audiencePersonas, setAudiencePersonas] = useState<AudiencePersona[]>([]);
  const [audiencePools, setAudiencePools] = useState<AudiencePool[]>([]);
  const [audienceVoting, setAudienceVoting] = useState(false);
  const [audienceResults, setAudienceResults] = useState<AudienceResults | null>(null);
  const [audienceTopic, setAudienceTopic] = useState("");
  const [audienceArguments, setAudienceArguments] = useState("");
  const [audiencePhase, setAudiencePhase] = useState<"pre" | "post">("pre");
  const [audiencePool, setAudiencePool] = useState<string | "">("");
  const [expandedPersona, setExpandedPersona] = useState<string | null>(null);

  // Fetch available voices on mount
  useEffect(() => {
    fetchAvailableVoices().then(() => setVoiceList(getAvailableVoices()));
  }, []);

  // Load global role templates to show "saved as global" indicators
  useEffect(() => {
    if (!window.__TAURI__) return;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const templates = await invoke("list_global_role_templates") as Record<string, unknown>;
        _setGlobalTemplates(new Set(Object.keys(templates)));
      } catch { /* ignore — non-critical */ }
    })();
  }, [project]); // Re-check when project data changes

  function getCollabVoiceForRole(roleSlug: string): string {
    return collabVoices[roleSlug] || getDefaultVoice();
  }

  function setCollabVoiceAssignment(roleSlug: string, voiceId: string) {
    setCollabVoices(prev => {
      const next = trimVoiceAssignments({ ...prev, [roleSlug]: voiceId });
      localStorage.setItem("vaak_collab_voice_assignments", JSON.stringify(next));
      return next;
    });
  }

  async function playMessage(msgId: number, text: string, fromRole: string) {
    // Skip empty messages
    if (!text || !text.trim() || text.trim() === ".") return;
    // Toggle off if already playing
    if (playingMsgId === msgId) {
      currentAudioRef.current?.pause();
      currentAudioRef.current = null;
      setPlayingMsgId(null);
      return;
    }
    // Stop any currently playing audio
    if (currentAudioRef.current) {
      currentAudioRef.current.pause();
      currentAudioRef.current = null;
    }

    setPlayingMsgId(msgId);

    let blob = audioCache.get(msgId);
    if (!blob) {
      try {
        const voiceId = getCollabVoiceForRole(fromRole);
        const formData = new FormData();
        formData.append("text", text);
        formData.append("voice_id", voiceId);
        const apiUrl = (import.meta as any).env?.VITE_API_URL || "http://127.0.0.1:19836";
        const resp = await fetch(`${apiUrl}/api/v1/tts`, { method: "POST", body: formData });
        if (!resp.ok) { setPlayingMsgId(null); return; }
        blob = await resp.blob();
        audioCache.set(msgId, blob);
      } catch {
        setPlayingMsgId(null);
        return;
      }
    }

    const url = URL.createObjectURL(blob);
    const audio = new Audio(url);
    currentAudioRef.current = audio;
    audio.onended = () => { URL.revokeObjectURL(url); setPlayingMsgId(null); currentAudioRef.current = null; };
    audio.onerror = () => { URL.revokeObjectURL(url); setPlayingMsgId(null); currentAudioRef.current = null; };
    await audio.play();
  }

  // Audience functions
  async function fetchAudiencePersonas(poolFilter?: string) {
    try {
      const apiUrl = (import.meta as any).env?.VITE_API_URL || "http://127.0.0.1:19836";
      const url = poolFilter
        ? `${apiUrl}/api/v1/audience/personas?pool=${poolFilter}`
        : `${apiUrl}/api/v1/audience/personas`;
      const resp = await fetch(url);
      if (resp.ok) {
        const data = await resp.json();
        setAudiencePersonas(data);
      }
    } catch { /* ignore */ }
  }

  async function fetchAudiencePools() {
    try {
      const apiUrl = (import.meta as any).env?.VITE_API_URL || "http://127.0.0.1:19836";
      const resp = await fetch(`${apiUrl}/api/v1/audience/pools`);
      if (resp.ok) {
        const data = await resp.json();
        setAudiencePools(data);
      }
    } catch { /* ignore */ }
  }

  const [audienceError, setAudienceError] = useState<string | null>(null);

  async function triggerAudienceVote() {
    if (!audienceTopic.trim()) return;
    setAudienceVoting(true);
    setAudienceResults(null);
    setAudienceError(null);
    try {
      const apiUrl = (import.meta as any).env?.VITE_API_URL || "http://127.0.0.1:19836";
      const resp = await fetch(`${apiUrl}/api/v1/audience/vote`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          topic: audienceTopic,
          arguments: audienceArguments,
          phase: audiencePhase,
          pool: audiencePool || null,
        }),
      });
      if (resp.ok) {
        const data = await resp.json();
        setAudienceResults(data);
      } else {
        setAudienceError(`Vote failed: ${resp.status} ${resp.statusText}`);
      }
    } catch (e) {
      setAudienceError(`Vote failed: ${e instanceof Error ? e.message : "Network error"}`);
    }
    setAudienceVoting(false);
  }

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
    if (!newSectionName.trim() || !projectDir || sectionLoading) return;
    setSectionLoading(true);
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
    } finally {
      setSectionLoading(false);
    }
  };

  const handleSwitchSection = async (slug: string) => {
    if (!projectDir || slug === activeSection || sectionLoading) return;
    setSectionLoading(true);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("switch_section", { dir: projectDir, slug });
      setActiveSection(slug);
      // Reload project data for the new section (also resets watcher to track new section's board)
      const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
      if (result) setProject(result);
    } catch (e) {
      console.error("[CollabTab] Failed to switch section:", e);
    } finally {
      setSectionLoading(false);
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
          const effectiveDir = result.effective_dir && result.effective_dir !== projectDir
            ? result.effective_dir : projectDir;
          if (result.effective_dir && result.effective_dir !== projectDir) {
            setProjectDir(result.effective_dir);
            persistDir(result.effective_dir);
          }
          setWatching(true);
          setProject(result);
          if (result.config?.settings?.message_retention_days != null) {
            setRetentionDays(result.config.settings.message_retention_days);
          }
          // Re-populate spawned PIDs from disk so kill works across app restarts
          try {
            await invoke("repopulate_spawned", { projectDir: effectiveDir });
          } catch { /* non-critical */ }
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
  const handleLaunchMember = async (role: string, instance: number) => {
    // Check for companion roles before launching
    const roleConfig = project?.config?.roles?.[role];
    const companions = (roleConfig as any)?.companions;
    if (companions && companions.length > 0) {
      setCompanionLaunch({
        role,
        instance,
        companions: companions.map((c: any) => ({
          role: c.role,
          enabled: c.default_enabled !== false,
          optional: c.optional !== false,
        })),
      });
      return;
    }
    if (!spawnConsented) {
      setConfirmAction({
        title: "Launch Claude Code Agent",
        message: `This will launch Claude Code with full autonomous permissions (--dangerously-skip-permissions) in a new terminal window. The agent will join as "${role}" and work autonomously. Continue?`,
        confirmLabel: "Launch",
        onConfirm: async () => {
          setSpawnConsented(true);
          setConfirmAction(null);
          await doLaunchMember(role, instance);
        },
      });
      return;
    }
    await doLaunchMember(role, instance);
  };

  const doLaunchMember = async (role: string, instance: number, skipCompanions?: boolean) => {
    setLaunchCooldown(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("launch_team_member", { projectDir, role, instance, skipCompanions: skipCompanions || false });
      }
    } catch (e) {
      console.error("[CollabTab] Failed to launch team member:", e);
    }
    setTimeout(() => setLaunchCooldown(false), 3000);
  };

  const handleCompanionLaunchConfirm = async () => {
    if (!companionLaunch) return;
    const { role, instance, companions } = companionLaunch;
    setCompanionLaunch(null);
    if (!spawnConsented) {
      setSpawnConsented(true);
    }
    // Launch the primary role — skip backend auto-companion since we handle it here
    await doLaunchMember(role, instance, true);
    // Launch enabled companions with a staggered delay
    for (const comp of companions) {
      if (comp.enabled) {
        await new Promise(r => setTimeout(r, 2000));
        await doLaunchMember(comp.role, 0, true);
      }
    }
  };


  const handleKillMember = async (role: string, instance: number) => {
    const roleConfig = project?.config?.roles?.[role];
    const companions = roleConfig?.companions?.map((c: { role: string }) => c.role) || [];
    const companionNote = companions.length > 0
      ? ` This will also disconnect companion roles: ${companions.join(", ")}.`
      : "";
    setConfirmAction({
      title: "Remove team member",
      message: `Remove ${role}:${instance} from the team? This will close their terminal window.${companionNote}`,
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

  const handleSendInterrupt = async () => {
    if (!interruptTarget || !projectDir) return;
    const to = `${interruptTarget.slug}:${interruptTarget.instance}`;
    const body = interruptReason.trim() || "Stop your current work and check in with the human.";
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("send_team_message", {
          dir: projectDir,
          to,
          subject: "INTERRUPT",
          body,
          msgType: "interrupt",
        });
      }
    } catch (e) {
      console.error("[CollabTab] Failed to send interrupt:", e);
    }
    setInterruptTarget(null);
    setInterruptReason("");
  };

  /** Expand a group card, initializing checkboxes (all pre-checked, disabled if already on roster) */
  const _expandGroup = (group: RoleGroup) => {
    if (expandedGroup === group.slug) {
      setExpandedGroup(null);
      return;
    }
    const roster = project?.config.roster || [];
    const checks: Record<string, boolean> = {};
    for (const entry of group.roles) {
      const existingCount = roster.filter((s: RosterSlot) => s.role === entry.slug).length;
      const alreadyFull = existingCount >= entry.instances;
      // Pre-check all roles by default — users uncheck what they don't want
      checks[entry.slug] = !alreadyFull;
    }
    setGroupRoleChecked(checks);
    setExpandedGroup(group.slug);
  };

  /** Deploy a group — add checked roles to the roster */
  const _handleAddGroup = async (group: RoleGroup) => {
    if (!projectDir || !window.__TAURI__) return;
    const { invoke } = await import("@tauri-apps/api/core");
    const roster = project?.config.roster || [];
    let added = 0;
    let skipped = 0;

    for (const entry of group.roles) {
      if (!groupRoleChecked[entry.slug]) {
        skipped += entry.instances;
        continue;
      }
      const existingCount = roster.filter((s: RosterSlot) => s.role === entry.slug).length;
      const toAdd = Math.max(0, entry.instances - existingCount);
      for (let i = 0; i < toAdd; i++) {
        try {
          await invoke("roster_add_slot", { projectDir, role: entry.slug });
          added++;
        } catch (e) {
          console.error(`[CollabTab] Failed to add ${entry.slug} slot:`, e);
        }
      }
      if (toAdd === 0) skipped += entry.instances;
    }
    setExpandedGroup(null);
    // Refresh project state
    if (added > 0) {
      console.log(`[CollabTab] Group "${group.name}" deployed: ${added} roles added, ${skipped} skipped`);
    }
  };

  // Suppress noUnusedLocals for future-use functions
  void _expandGroup; void _handleAddGroup; void _addTeamTab; void _setAddTeamTab; void _globalTemplates;

  /** Open the create-group modal, optionally pre-filling from current roster */
  const openCreateGroupModal = (fromRoster?: boolean, parentSlug?: string) => {
    setNewGroupName("");
    setNewGroupIcon("\uD83D\uDCE6");
    setNewGroupDesc("");
    setNewGroupParent(parentSlug || null);
    const roles: Record<string, number> = {};
    if (fromRoster && project) {
      const roster = project.config.roster || [];
      for (const slot of roster) {
        roles[slot.role] = (roles[slot.role] || 0) + 1;
      }
    }
    setNewGroupRoles(roles);
    setCreateGroupOpen(true);
  };

  /** Save a custom group to project.json via Tauri */
  const handleSaveCustomGroup = async () => {
    if (!projectDir || !newGroupName.trim() || !window.__TAURI__) return;
    const slug = newGroupName.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    const group: RoleGroup = {
      slug,
      name: newGroupName.trim(),
      icon: newGroupIcon || "\uD83D\uDCE6",
      description: newGroupDesc.trim(),
      builtin: false,
      roles: Object.entries(newGroupRoles)
        .filter(([, count]) => count > 0)
        .map(([roleSlug, count]) => ({ slug: roleSlug, instances: count })),
      parent: newGroupParent,
    };
    if (group.roles.length === 0) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("save_role_group", { projectDir, group });
      setCreateGroupOpen(false);
    } catch (e) {
      // Fallback: if Tauri command doesn't exist yet, store locally
      console.error("[CollabTab] Failed to save group:", e);
      setCreateGroupOpen(false);
    }
  };

  const handleViewAgent = async (slug: string, instance: number) => {
    if (!projectDir) return;
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("focus_agent_window", { role: slug, instance });
      }
    } catch (e) {
      console.error("[CollabTab] Failed to focus agent window:", e);
    }
  };

  const handleBuzz = async (slug: string, instance: number) => {
    if (!projectDir) return;
    const key = `${slug}:${instance}`;
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        // OS-level buzz: sends keystrokes directly to the agent's terminal window.
        // Works even when the agent's MCP connection is dead.
        try {
          await invoke("buzz_agent_terminal", { role: slug, instance });
          setBuzzedKey(key);
        } catch {
          // Terminal buzz failed (PID not found, window gone, etc.) — fall back to board message
          await invoke("send_team_message", {
            dir: projectDir,
            to: key,
            subject: "BUZZ",
            body: "Wake up — you may have lost your session. Rejoin and resume standby.",
            msgType: "buzz",
          });
          setBuzzedKey(key);
        }
        setTimeout(() => setBuzzedKey(prev => prev === key ? null : prev), 1500);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to buzz agent:", e);
    }
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

  const handleOpenStartDiscussion = () => {
    const activeSessions = project?.sessions?.filter(s => s.status === "active") || [];
    const participantMap: Record<string, boolean> = {};
    activeSessions.forEach(s => { participantMap[`${s.role}:${s.instance}`] = true; });
    setSdParticipants(participantMap);
    setSdFormat("delphi");
    setSdTopic("");
    setSdStarting(false);
    setStartDiscussionOpen(true);
  };

  const handleStartDiscussion = async () => {
    if (!sdTopic.trim() && sdFormat !== "continuous") return;
    setSdStarting(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        const participants = Object.entries(sdParticipants).filter(([, v]) => v).map(([k]) => k);
        const topic = sdFormat === "continuous"
          ? "Continuous review — auto-triggered micro-rounds"
          : sdTopic.trim();
        const modSession = project?.sessions?.find(s => s.role === "moderator" && s.status === "active");
        const mgrSession = project?.sessions?.find(s => s.role === "manager" && s.status === "active");
        const moderator = modSession
          ? `moderator:${modSession.instance}`
          : mgrSession
            ? `manager:${mgrSession.instance}`
            : participants[0] || "human:0";
        await invoke("start_discussion", {
          dir: projectDir,
          mode: sdFormat,
          topic,
          moderator,
          participants,
        });
        const state = await invoke<DiscussionState | null>("get_discussion_state", { dir: projectDir });
        if (state) setDiscussionState(state);
        setStartDiscussionOpen(false);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to start discussion:", e);
    } finally {
      setSdStarting(false);
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

        // Try to read existing project; auto-create .vaak/ if missing
        let result: (ParsedProject & { effective_dir?: string }) | null;
        try {
          result = await invoke<(ParsedProject & { effective_dir?: string }) | null>("watch_project_dir", { dir });
        } catch (watchErr) {
          // watch_project_dir throws when .vaak/ doesn't exist — auto-create it
          if (String(watchErr).includes("Not a Vaak project") || String(watchErr).includes("no .vaak")) {
            const config = buildDefaultConfig(dir);
            await invoke("initialize_project", {
              dir,
              config: JSON.stringify(config),
            });
            // Re-read after creation
            result = await invoke<(ParsedProject & { effective_dir?: string }) | null>("watch_project_dir", { dir });
          } else {
            throw watchErr;
          }
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
          {!discussionState?.active && (
            <button
              className="start-discussion-btn"
              onClick={handleOpenStartDiscussion}
              title="Start a structured discussion (Delphi, Oxford, Red Team, or Continuous)"
              aria-label="Start discussion"
            >
              &#9998; Discuss
            </button>
          )}
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
              className={`section-tab${s.slug === activeSection ? " section-tab-active" : ""}${sectionLoading ? " section-tab-loading" : ""}`}
              onClick={() => handleSwitchSection(s.slug)}
              disabled={sectionLoading}
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
              <button className="section-tab-create-ok" onClick={handleCreateSection} disabled={!newSectionName.trim() || sectionLoading}>{sectionLoading ? "\u2026" : "+"}</button>
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

        {/* Discussion Status Panel — rich display when active */}
        {discussionState?.active ? (
          <div className="discussion-status-panel" role="region" aria-label="Active discussion status">
            {/* Row 1: Mode + Phase + Round + Actions */}
            <div className="discussion-status-header">
              <div className="discussion-status-left">
                <span className="discussion-status-mode">
                  {(discussionState.mode || "Discussion").charAt(0).toUpperCase() + (discussionState.mode || "").slice(1)}
                </span>
                <span className="discussion-status-phase-badge" aria-label={`Phase: ${
                  closingRound ? "aggregating" :
                  discussionState.phase === "submitting" ? "accepting submissions" :
                  discussionState.phase === "reviewing" ? "reviewing aggregate" :
                  discussionState.phase === "complete" ? "complete" :
                  discussionState.phase || "unknown"
                }`}>
                  {closingRound ? "Aggregating..." :
                   discussionState.phase === "submitting" ? "Submitting" :
                   discussionState.phase === "aggregating" ? "Aggregating" :
                   discussionState.phase === "reviewing" ? "Reviewing" :
                   discussionState.phase === "paused" ? "Paused" :
                   discussionState.phase === "complete" ? "Complete" :
                   discussionState.phase || ""}
                </span>
                <span className="discussion-status-round-counter" aria-label={`Round ${discussionState.current_round} of ${discussionState.settings?.max_rounds || "?"}`}>
                  Round {discussionState.current_round}
                  {discussionState.settings?.max_rounds ? ` / ${discussionState.settings.max_rounds}` : ""}
                </span>
                {discussionState.moderator && (
                  <span className="discussion-status-mod">
                    Mod: <span style={{ color: getRoleColor(discussionState.moderator.split(":")[0]) }}>{discussionState.moderator}</span>
                  </span>
                )}
                {discussionState.mode === "continuous" && (
                  <select
                    className="discussion-controls-timeout"
                    value={continuousTimeout}
                    onChange={(e) => handleSetContinuousTimeout(Number(e.target.value))}
                    aria-label="Auto-close timeout"
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

            {/* Row 2: Topic */}
            {discussionState.topic && (
              <div className="discussion-status-topic" title={discussionState.topic}>
                {discussionState.topic}
              </div>
            )}

            {/* Row 3: Participant submission tracker */}
            {discussionState.phase === "submitting" && discussionState.rounds.length > 0 && (() => {
              const currentRound = discussionState.rounds[discussionState.rounds.length - 1];
              const submittedBy = new Set((currentRound?.submissions || []).map(s => s.from));
              const eligible = (discussionState.participants || []).filter(p => p !== discussionState.moderator);
              if (eligible.length === 0) return null;
              return (
                <div className="discussion-status-submissions" aria-label={`${submittedBy.size} of ${eligible.length} submitted`}>
                  {eligible.map(pid => {
                    const [role] = pid.split(":");
                    const didSubmit = submittedBy.has(pid);
                    return (
                      <span
                        key={pid}
                        className={`discussion-status-participant${didSubmit ? " ds-submitted" : ""}`}
                        title={`${pid}${didSubmit ? " — submitted" : " — waiting"}`}
                      >
                        <span className="ds-check" aria-hidden="true">{didSubmit ? "\u2713" : "\u2022"}</span>
                        <span style={{ color: getRoleColor(role) }}>{pid}</span>
                      </span>
                    );
                  })}
                </div>
              );
            })()}

            {/* Round history mini-bar */}
            {discussionState.rounds.length > 1 && (
              <div className="discussion-status-rounds-bar" aria-label="Round history">
                {discussionState.rounds.map((round, i) => (
                  <span
                    key={i}
                    className={`ds-round-pip${round.closed_at ? " ds-round-closed" : " ds-round-open"}`}
                    title={`Round ${round.number}: ${round.closed_at ? "closed" : "open"} — ${round.submissions?.length || 0} submissions`}
                  />
                ))}
              </div>
            )}
          </div>
        ) : null}

        {/* Add to Team — collapsible section, collapsed by default to maximize conversation space */}
        {project && (
          <div className={`add-team-section${teamSectionOpen ? " add-team-section-open" : ""}`}>
            {(() => {
              const customGroups = (project.config.role_groups || []).filter(g => !g.builtin);
              const allGroups = [...BUILTIN_ROLE_GROUPS, ...customGroups];
              const allRoleSlugs = Object.keys(project.config.roles);
              const activeGroup = expandedGroup || "all";
              const selectedGroupData = allGroups.find(g => g.slug === activeGroup);
              const visibleSlugs: string[] = activeGroup === "all"
                ? allRoleSlugs
                : (selectedGroupData?.roles.map(r => r.slug).filter(s => allRoleSlugs.includes(s)) || allRoleSlugs);
              const roster = project.config.roster || [];

              // Build tree from flat groups — group by parent
              const topLevel = allGroups.filter(g => !g.parent);
              const childrenOf = (parentSlug: string) => allGroups.filter(g => g.parent === parentSlug);

              // Search filter — matches propagate up (child match keeps parent visible)
              const searchLower = groupSearch.toLowerCase().trim();
              const groupMatchesSearch = (g: RoleGroup): boolean => {
                if (!searchLower) return true;
                if (g.name.toLowerCase().includes(searchLower)) return true;
                if (g.description.toLowerCase().includes(searchLower)) return true;
                return childrenOf(g.slug).some(child => groupMatchesSearch(child));
              };

              // Count roles in a group including children
              const deepRoleCount = (g: RoleGroup): number => {
                let count = g.roles.reduce((sum, r) => sum + r.instances, 0);
                for (const child of childrenOf(g.slug)) count += deepRoleCount(child);
                return count;
              };

              // Breadcrumb path from root to active group
              const breadcrumbs: { slug: string; name: string }[] = [];
              if (activeGroup !== "all") {
                let current = allGroups.find(g => g.slug === activeGroup);
                while (current) {
                  breadcrumbs.unshift({ slug: current.slug, name: current.name });
                  current = current.parent ? allGroups.find(g => g.slug === current!.parent) : undefined;
                }
              }

              // Toggle tree node expanded/collapsed
              const toggleTreeNode = (slug: string) => {
                setTreeExpanded(prev => {
                  const next = new Set(prev);
                  if (next.has(slug)) next.delete(slug); else next.add(slug);
                  return next;
                });
              };

              // Render a single tree node
              const renderTreeNode = (group: RoleGroup, depth: number) => {
                if (!groupMatchesSearch(group)) return null;
                const children = childrenOf(group.slug).sort((a, b) => (a.order ?? 99) - (b.order ?? 99));
                const hasChildren = children.length > 0;
                const isExpanded = treeExpanded.has(group.slug);
                const isActive = activeGroup === group.slug;
                const count = deepRoleCount(group);

                return (
                  <div key={group.slug} className="group-tree-branch">
                    <button
                      className={`group-tree-node${isActive ? " group-tree-node-active" : ""}${!group.builtin ? " group-tree-node-custom" : ""}`}
                      style={{ paddingLeft: `${8 + depth * 16}px` }}
                      onClick={() => setExpandedGroup(group.slug)}
                      title={group.description}
                      aria-expanded={hasChildren ? isExpanded : undefined}
                      aria-label={`${group.name}, ${count} roles${isActive ? ", selected" : ""}`}
                    >
                      {hasChildren && (
                        <span
                          className={`group-tree-toggle${isExpanded ? " group-tree-toggle-open" : ""}`}
                          onClick={(e) => { e.stopPropagation(); toggleTreeNode(group.slug); }}
                          role="button"
                          aria-label={isExpanded ? "Collapse" : "Expand"}
                        />
                      )}
                      {!hasChildren && <span className="group-tree-leaf" />}
                      <span className="group-tree-icon">{group.icon}</span>
                      <span className="group-tree-name">{group.name}</span>
                      <span className="group-tree-count">{count}</span>
                    </button>
                    {hasChildren && isExpanded && (
                      <div className="group-tree-children">
                        {children.map(child => renderTreeNode(child, depth + 1))}
                      </div>
                    )}
                  </div>
                );
              };

              const activeGroupName = activeGroup === "all" ? "All" : (selectedGroupData?.name || activeGroup);
              const activeGroupIcon = activeGroup === "all" ? "\u2B50" : (selectedGroupData?.icon || "");

              return (
                <>
                  {/* Compact header bar — always visible, one line */}
                  <button
                    className="add-team-header"
                    onClick={() => setTeamSectionOpen(prev => !prev)}
                    aria-expanded={teamSectionOpen}
                    aria-label={`Groups & Roles: ${activeGroupName}, ${allRoleSlugs.length} roles. Click to ${teamSectionOpen ? "collapse" : "expand"}`}
                  >
                    <span className={`add-team-header-arrow${teamSectionOpen ? " add-team-header-arrow-open" : ""}`} />
                    <span className="add-team-header-label">Groups & Roles</span>
                    <span className="add-team-header-current">{activeGroupIcon} {activeGroupName}</span>
                    <span className="add-team-header-count">{allRoleSlugs.length}</span>
                    {/* View toggle — always accessible */}
                    <span className="add-team-header-views" onClick={(e) => e.stopPropagation()}>
                      <button
                        className={`roster-view-btn${rosterViewMode === "grid" ? " roster-view-btn-active" : ""}`}
                        onClick={() => setRosterViewMode("grid")}
                        title="Grid view"
                      >&#9638;</button>
                      <button
                        className={`roster-view-btn${rosterViewMode === "list" ? " roster-view-btn-active" : ""}`}
                        onClick={() => setRosterViewMode("list")}
                        title="List view"
                      >&#9776;</button>
                    </span>
                  </button>

                  {/* Expandable content — only visible when open */}
                  {teamSectionOpen && (
                    <div className="add-team-expandable">
                      {/* Search bar */}
                      <input
                        className="group-search-input"
                        type="text"
                        placeholder="Search groups and roles..."
                        value={groupSearch}
                        onChange={(e) => setGroupSearch(e.target.value)}
                        aria-label="Search groups and roles"
                      />

                      {/* Tree navigation */}
                      <div className="group-tree" role="tree" aria-label="Role groups">
                        <button
                          className={`group-tree-node${activeGroup === "all" ? " group-tree-node-active" : ""}`}
                          style={{ paddingLeft: "8px" }}
                          onClick={() => setExpandedGroup(null)}
                          aria-label={`All roles, ${allRoleSlugs.length} total${activeGroup === "all" ? ", selected" : ""}`}
                        >
                          <span className="group-tree-leaf" />
                          <span className="group-tree-icon">{"\u2B50"}</span>
                          <span className="group-tree-name">All</span>
                          <span className="group-tree-count">{allRoleSlugs.length}</span>
                        </button>

                        {topLevel
                          .sort((a, b) => (a.order ?? 99) - (b.order ?? 99))
                          .map(group => renderTreeNode(group, 0))}

                        <button
                          className="group-tree-node group-tree-node-create"
                          style={{ paddingLeft: "8px" }}
                          onClick={() => openCreateGroupModal()}
                          title="Create a custom group"
                          aria-label="Create a new group"
                        >
                          <span className="group-tree-leaf" />
                          <span className="group-tree-icon" style={{ color: "#1da1f2" }}>+</span>
                          <span className="group-tree-name">New Group</span>
                        </button>
                      </div>

                      {/* Breadcrumb path */}
                      {breadcrumbs.length > 0 && (
                        <nav className="group-breadcrumbs" aria-label="Group navigation">
                          <button className="group-breadcrumb" onClick={() => setExpandedGroup(null)}>All</button>
                          {breadcrumbs.map((bc, i) => (
                            <span key={bc.slug}>
                              <span className="group-breadcrumb-sep" aria-hidden="true">&rsaquo;</span>
                              <button
                                className={`group-breadcrumb${i === breadcrumbs.length - 1 ? " group-breadcrumb-active" : ""}`}
                                onClick={() => setExpandedGroup(bc.slug)}
                              >{bc.name}</button>
                            </span>
                          ))}
                        </nav>
                      )}

                      {/* Role add buttons — filtered by selected group + search */}
                      <div className="add-team-toolbar">
                        {visibleSlugs
                          .filter(slug => {
                            if (!searchLower) return true;
                            const role = project.config.roles[slug];
                            if (!role) return false;
                            return role.title.toLowerCase().includes(searchLower) || slug.includes(searchLower);
                          })
                          .sort((a, b) => (ROLE_ORDER[a] ?? 99) - (ROLE_ORDER[b] ?? 99))
                          .map((slug) => {
                            const role = project.config.roles[slug];
                            if (!role) return null;
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
                        <button
                          className="add-team-btn add-team-btn-create"
                          onClick={openCreateRoleForm}
                          title="Create a new custom role"
                          aria-label="Create a new custom role"
                        >
                          <span className="add-team-btn-label">+ New Role</span>
                        </button>
                      </div>
                    </div>
                  )}
                </>
              );
            })()}
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
          // Hide companion roles when their parent role has no active instances
          const companionSlugs = new Set<string>();
          for (const [, roleDef] of Object.entries(project.config.roles)) {
            const rd = roleDef as RoleConfig;
            if (rd.companions) {
              for (const comp of rd.companions) {
                companionSlugs.add((comp as any).role);
              }
            }
          }
          const filteredCards = cards.filter(card => {
            if (!companionSlugs.has(card.slug)) return true;
            // Find parent role that lists this as a companion
            const parentActive = Object.entries(project.config.roles).some(([parentSlug, parentDef]) => {
              const pd = parentDef as RoleConfig;
              if (!pd.companions?.some((c: any) => c.role === card.slug)) return false;
              return project.sessions?.some(s => s.role === parentSlug && s.status === "active");
            });
            return parentActive;
          });
          const hasRoster = project.config.roster && project.config.roster.length > 0;
          const vacantCount = filteredCards.filter(c => c.status === "vacant").length;
          return (
            <>
              {filteredCards.length > 0 && (
                <div className={`project-roles-grid${rosterViewMode === "list" ? " project-roles-list" : ""}`}>
                  {filteredCards.map((card) => {
                    const cardKey = `${card.slug}:${card.instance}`;
                    const matchingRole = project.role_statuses.find((r) => r.slug === card.slug);
                    return (
                      <div
                        key={cardKey}
                        className={`role-card role-card-clickable ${card.status === "working" ? "role-card-working" : ""} ${card.status === "vacant" ? "role-card-vacant" : ""}`}
                        style={{ borderLeftColor: card.roleColor }}
                        role="button"
                        tabIndex={0}
                        aria-label={`${card.title}, status: ${card.status}. Click to view details.`}
                        onClick={() => {
                          if (card.slug === "audience") {
                            setAudiencePanelOpen(true);
                            if (audiencePersonas.length === 0) { fetchAudiencePersonas(); fetchAudiencePools(); }
                          } else {
                            matchingRole && setSelectedRole(matchingRole);
                          }
                        }}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            if (card.slug === "audience") {
                              setAudiencePanelOpen(true);
                              if (audiencePersonas.length === 0) { fetchAudiencePersonas(); fetchAudiencePools(); }
                            } else {
                              matchingRole && setSelectedRole(matchingRole);
                            }
                          }
                        }}
                      >
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
                        {/* Companion badge — show if this role is a companion of another role */}
                        {(() => {
                          const parentRole = Object.entries(project.config.roles).find(([, r]) =>
                            (r as any).companions?.some((c: any) => c.role === card.slug)
                          );
                          if (!parentRole) return null;
                          return <span className="role-companion-badge">paired with {(parentRole[1] as any).title || parentRole[0]}</span>;
                        })()}
                        {/* Action row — always visible, grouped by severity */}
                        <div className="role-card-actions" role="group" aria-label={`Actions for ${card.title}${card.instance >= 0 ? ` instance ${card.instance}` : ""}`} onClick={(e) => e.stopPropagation()}>
                          {/* Safe actions: View, Edit */}
                          {card.status !== "vacant" && (
                            <button
                              className="role-action-btn role-action-view"
                              onClick={() => handleViewAgent(card.slug, card.instance >= 0 ? card.instance : 0)}
                              title={`View ${card.title} — bring terminal to front`}
                              aria-label={`View ${card.title}${card.instance >= 0 ? ` instance ${card.instance}` : ""} terminal`}
                            >&#128065; View</button>
                          )}
                          {card.instance === 0 && (
                            <button
                              className="role-action-btn"
                              onClick={() => openEditRoleForm(card.slug)}
                              title={`Edit ${card.title} role settings`}
                              aria-label={`Edit ${card.title} role settings`}
                            >&#9998; Edit</button>
                          )}
                          {/* Global badge removed — all roles are global, badge was redundant and broke layout consistency */}
                          {/* Gentle nudge: Buzz — wake up stale agents */}
                          {card.status !== "vacant" && (() => {
                            const bk = `${card.slug}:${card.instance >= 0 ? card.instance : 0}`;
                            const isBuzzed = buzzedKey === bk;
                            return (
                              <button
                                className={`role-action-btn role-action-buzz${isBuzzed ? " role-action-buzz-sent" : ""}`}
                                onClick={() => handleBuzz(card.slug, card.instance >= 0 ? card.instance : 0)}
                                disabled={isBuzzed}
                                title={isBuzzed ? "Buzz sent!" : `Buzz ${card.title} — wake up a stale agent`}
                                aria-label={isBuzzed ? `Buzz sent to ${card.title}` : `Buzz ${card.title}${card.instance >= 0 ? ` instance ${card.instance}` : ""}`}
                              >{isBuzzed ? "\u2713 Sent" : "\uD83D\uDD14 Buzz"}</button>
                            );
                          })()}
                          {/* Warning actions: Interrupt */}
                          {card.status !== "vacant" && (
                            <button
                              className="role-action-btn role-action-interrupt"
                              onClick={() => setInterruptTarget({ slug: card.slug, instance: card.instance >= 0 ? card.instance : 0, title: card.title })}
                              title={`Interrupt ${card.title} — stop their current work`}
                              aria-label={`Send interrupt to ${card.title}${card.instance >= 0 ? ` instance ${card.instance}` : ""}`}
                            >&#9889; Interrupt</button>
                          )}
                          {/* Separator between safe and destructive actions */}
                          {(card.status !== "vacant" || (hasRoster && card.status === "vacant")) && (
                            <span className="role-actions-separator" aria-hidden="true" />
                          )}
                          {/* Destructive actions: Disconnect, Remove, Delete */}
                          {card.status !== "vacant" && (
                            <button
                              className="role-action-btn role-action-disconnect"
                              onClick={() => {
                                const inst = card.instance >= 0 ? card.instance : 0;
                                handleKillMember(card.slug, inst);
                              }}
                              title={`Disconnect ${card.title} agent — ends their session`}
                              aria-label={`Disconnect ${card.title}${card.instance >= 0 ? ` instance ${card.instance}` : ""} agent`}
                            >&times; Disconnect</button>
                          )}
                          {hasRoster && card.status === "vacant" && (
                            <button
                              className="role-action-btn role-action-remove"
                              onClick={() => handleRemoveRosterSlot(card.slug, card.instance >= 0 ? card.instance : 0)}
                              title={`Remove ${card.title} slot from roster`}
                              aria-label={`Remove ${card.title} roster slot`}
                            >&#128465; Remove</button>
                          )}
                          {card.instance === 0 && card.status === "vacant" && !BUILT_IN_ROLES.includes(card.slug) && (
                            <button
                              className="role-action-btn role-action-delete"
                              onClick={() => handleDeleteRole(card.slug)}
                              title={`Permanently delete ${card.title} role`}
                              aria-label={`Permanently delete ${card.title} role from project`}
                            >&#128465; Delete</button>
                          )}
                        </div>
                        {voiceList.length > 0 && (
                          <select
                            className="role-voice-select"
                            value={collabVoices[card.slug] || ""}
                            onChange={(e) => { e.stopPropagation(); setCollabVoiceAssignment(card.slug, e.target.value); }}
                            onClick={(e) => e.stopPropagation()}
                            title={`Voice for ${card.title}`}
                          >
                            <option value="">Default voice</option>
                            {voiceList.map(v => (
                              <option key={v.voice_id} value={v.voice_id}>{v.name}</option>
                            ))}
                          </select>
                        )}
                        {/* Launch button for vacant slots */}
                        {card.status === "vacant" && claudeInstalled !== false && (
                          <button
                            className="role-card-launch-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleLaunchMember(card.slug, card.instance);
                            }}
                            disabled={launchCooldown}
                            title={`Launch Claude agent as ${card.title}`}
                            aria-label={`Launch Claude agent as ${card.title}`}
                          >Launch</button>
                        )}
                      </div>
                    );
                  })}

                  {/* Audience card now rendered through normal roster loop above — click opens audience panel */}
                </div>
              )}

              {/* Launch All Vacant button */}
              {vacantCount > 1 && claudeInstalled !== false && (
                <button
                  className="launch-team-btn"
                  onClick={() => {
                    const vacantCards = cards.filter(c => c.status === "vacant");
                    const launchAllVacant = async () => {
                      setLaunching(true);
                      setLaunchCooldown(true);
                      for (let i = 0; i < vacantCards.length; i++) {
                        if (i > 0) await new Promise(r => setTimeout(r, 2000));
                        await doLaunchMember(vacantCards[i].slug, vacantCards[i].instance);
                      }
                      setLaunching(false);
                    };
                    if (!spawnConsented) {
                      setConfirmAction({
                        title: "Launch All Vacant",
                        message: `This will launch ${vacantCards.length} Claude Code agent(s) with full autonomous permissions (--dangerously-skip-permissions) in new terminal windows. Continue?`,
                        confirmLabel: "Launch",
                        onConfirm: async () => {
                          setSpawnConsented(true);
                          setConfirmAction(null);
                          await launchAllVacant();
                        },
                      });
                    } else {
                      launchAllVacant();
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
            <div className="message-timeline-empty" role="status">
              {hasNoSessions
                ? "No team members connected yet. Launch agents from the roster above to get started."
                : "Team connected — no messages yet. Your team's communication will appear here."}
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

              const allMessages = project!.messages;
              const totalCount = allMessages.length;
              const hasHiddenMessages = totalCount > visibleMsgLimit;
              const visibleMessages = hasHiddenMessages
                ? allMessages.slice(totalCount - visibleMsgLimit)
                : allMessages;

              return (<>
              {hasHiddenMessages && (
                <button
                  className="load-earlier-btn"
                  onClick={() => setVisibleMsgLimit(prev => prev + MSG_PAGE_SIZE)}
                >
                  Load {Math.min(MSG_PAGE_SIZE, totalCount - visibleMsgLimit)} earlier messages ({totalCount - visibleMsgLimit} hidden)
                </button>
              )}
              {visibleMessages.map((msg: BoardMessage) => {
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
                      <button className="message-play-btn" onClick={(e) => { e.stopPropagation(); playMessage(msg.id, `${msg.subject || ""}. ${msg.body}`, msg.from.split(":")[0]); }} title={playingMsgId === msg.id ? "Stop" : "Play"}>{playingMsgId === msg.id ? "\u23F9" : "\u25B6"}</button>
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
                      <button className="message-play-btn" onClick={(e) => { e.stopPropagation(); playMessage(msg.id, `${msg.subject || ""}. ${msg.body}`, fromRole); }} title={playingMsgId === msg.id ? "Stop" : "Play"}>{playingMsgId === msg.id ? "\u23F9" : "\u25B6"}</button>
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
                    onPlay={playMessage}
                    playingMsgId={playingMsgId}
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
                    <button className="message-play-btn" onClick={(e) => { e.stopPropagation(); playMessage(msg.id, `${msg.subject || ""}. ${msg.body}`, fromRole); }} title={playingMsgId === msg.id ? "Stop" : "Play"}>{playingMsgId === msg.id ? "\u23F9" : "\u25B6"}</button>
                  </div>
                  {msg.subject && (
                    <div className="message-card-subject">{msg.subject}</div>
                  )}
                  <div className="message-card-body">{msg.body}</div>
                  {/* Expandable audience vote grid for board messages with vote data */}
                  {!!msg.metadata?.audience_vote && Array.isArray(msg.metadata?.votes) && (msg.metadata.votes as any[]).length > 0 && (
                    <div className="audience-board-votes">
                      <button
                        className="audience-board-toggle"
                        onClick={(e) => { e.stopPropagation(); setExpandedPersona(prev => prev === `board-${msg.id}` ? null : `board-${msg.id}`); }}
                        aria-expanded={expandedPersona === `board-${msg.id}`}
                      >
                        {expandedPersona === `board-${msg.id}` ? "Hide" : "Show"} {(msg.metadata.votes as any[]).length} individual votes
                      </button>
                      {expandedPersona === `board-${msg.id}` && (
                        <div className="audience-grid" style={{ marginTop: "8px" }}>
                          {(["groq", "openai", "anthropic"] as const).map(prov => {
                            const provVotes = (msg.metadata.votes as any[]).filter((v: any) => v.provider === prov);
                            if (provVotes.length === 0) return null;
                            const provLabel = prov === "groq" ? "Llama" : prov === "openai" ? "GPT-5m" : "Haiku";
                            return (
                              <div key={prov} className="audience-grid-column">
                                <div className="audience-grid-col-header">
                                  <span className="audience-grid-col-title">{provLabel}</span>
                                </div>
                                {provVotes.map((v: any, i: number) => {
                                  const seatKey = `board-${msg.id}-${v.provider}-${v.persona}`;
                                  const isExp = expandedPersona === seatKey;
                                  const dotColor = v.vote === "FOR" ? "#17bf63" : v.vote === "AGAINST" ? "#e0245e" : v.vote === "ERROR" ? "#f5a623" : "#8899a6";
                                  return (
                                    <div
                                      key={i}
                                      className={`audience-seat${isExp ? " audience-seat-expanded" : ""}`}
                                      onClick={(e) => { e.stopPropagation(); setExpandedPersona(isExp ? `board-${msg.id}` : seatKey); }}
                                      role="button"
                                      tabIndex={0}
                                      onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setExpandedPersona(isExp ? `board-${msg.id}` : seatKey); }}}
                                      aria-expanded={isExp}
                                      aria-label={`${v.persona}: ${v.vote}`}
                                    >
                                      <div className="audience-seat-summary">
                                        <span className="audience-seat-dot" style={{ background: dotColor }} />
                                        <span className="audience-seat-name">{(v.persona || "").split(" ")[0]}</span>
                                        <span className={`audience-seat-badge audience-seat-badge-${(v.vote || "abstain").toLowerCase()}`}>{v.vote}</span>
                                      </div>
                                      {isExp && (
                                        <div className="audience-seat-detail">
                                          <div className="audience-seat-fullname">{v.persona}</div>
                                          <div className="audience-seat-bg">{v.background}</div>
                                          <div className="audience-seat-rationale">{v.rationale}</div>
                                        </div>
                                      )}
                                    </div>
                                  );
                                })}
                              </div>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })}</>);
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
            {sending ? "Sending\u2026" : "Send"}
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

        {/* Audience Panel Modal */}
        {audiencePanelOpen && (
          <div className="confirm-dialog" onClick={() => setAudiencePanelOpen(false)}>
            <div className="audience-panel" onClick={(e) => e.stopPropagation()}>
              <div className="audience-panel-header">
                <span className="audience-panel-title">Audience Panel</span>
                <span className="audience-panel-subtitle">
                  {audiencePools.length > 0
                    ? audiencePools.map(p => `${p.member_count} ${p.name}`).join(" + ")
                    : "36 AI Jurors \u00b7 3 Models"}
                </span>
                <button className="audience-panel-close" onClick={() => setAudiencePanelOpen(false)} aria-label="Close audience panel">&times;</button>
              </div>

              {/* Vote Controls */}
              <div className="audience-vote-controls">
                <input
                  className="audience-topic-input"
                  type="text"
                  value={audienceTopic}
                  onChange={(e) => setAudienceTopic(e.target.value)}
                  placeholder="Debate topic / proposition..."
                />
                <textarea
                  className="audience-args-input"
                  value={audienceArguments}
                  onChange={(e) => setAudienceArguments(e.target.value)}
                  placeholder="Paste debate arguments here (leave empty for pre-vote)..."
                  rows={4}
                />
                <div className="audience-vote-actions">
                  <select
                    className="audience-pool-select"
                    value={audiencePool}
                    onChange={(e) => { setAudiencePool(e.target.value); fetchAudiencePersonas(e.target.value || undefined); }}
                  >
                    <option value="">All Pools</option>
                    {audiencePools.map(p => (
                      <option key={p.id} value={p.id}>{p.name} ({p.member_count})</option>
                    ))}
                  </select>
                  <select
                    className="audience-phase-select"
                    value={audiencePhase}
                    onChange={(e) => setAudiencePhase(e.target.value as "pre" | "post")}
                  >
                    <option value="pre">Pre-Vote (topic only)</option>
                    <option value="post">Post-Vote (with arguments)</option>
                  </select>
                  <button
                    className="audience-vote-btn"
                    onClick={triggerAudienceVote}
                    disabled={audienceVoting || !audienceTopic.trim()}
                  >
                    {audienceVoting ? "Collecting votes..." : "Collect Votes"}
                  </button>
                </div>
              </div>

              {/* Error display */}
              {audienceError && (
                <div style={{ padding: "12px 20px", color: "#e0245e", fontSize: "13px", borderBottom: "1px solid rgba(255,255,255,0.08)" }}>
                  {audienceError}
                </div>
              )}

              {/* Results */}
              {audienceResults && (
                <div className="audience-results">
                  <div className="audience-tally">
                    <div className="audience-tally-header">
                      {audienceResults.phase === "pre" ? "PRE-VOTE" : "POST-VOTE"} TALLY
                      <span className="audience-tally-time">{audienceResults.total_latency_ms}ms</span>
                    </div>
                    <div className="audience-tally-bars">
                      <div className="audience-tally-for">
                        <span className="audience-tally-label">FOR</span>
                        <div className="audience-tally-bar" style={{ width: `${audienceResults.total_voters ? (audienceResults.tally.FOR / audienceResults.total_voters) * 100 : 0}%`, background: "#17bf63" }} />
                        <span className="audience-tally-count">{audienceResults.tally.FOR}</span>
                      </div>
                      <div className="audience-tally-against">
                        <span className="audience-tally-label">AGAINST</span>
                        <div className="audience-tally-bar" style={{ width: `${audienceResults.total_voters ? (audienceResults.tally.AGAINST / audienceResults.total_voters) * 100 : 0}%`, background: "#e0245e" }} />
                        <span className="audience-tally-count">{audienceResults.tally.AGAINST}</span>
                      </div>
                      {(audienceResults.tally.ABSTAIN > 0 || audienceResults.tally.ERROR > 0) && (
                        <div style={{ display: "flex", gap: "12px", marginTop: "4px", fontSize: "11px", color: "#8899a6" }}>
                          {audienceResults.tally.ABSTAIN > 0 && <span>Abstain: {audienceResults.tally.ABSTAIN}</span>}
                          {audienceResults.tally.ERROR > 0 && <span style={{ color: "#f5a623" }}>Errors: {audienceResults.tally.ERROR}</span>}
                        </div>
                      )}
                    </div>
                    {/* By provider breakdown */}
                    <div className="audience-tally-providers">
                      {Object.entries(audienceResults.tally_by_provider).map(([prov, counts]) => (
                        <div key={prov} className="audience-provider-row">
                          <span className="audience-provider-name">{prov === "groq" ? "Llama" : prov === "openai" ? "GPT-5m" : "Haiku"}</span>
                          <span className="audience-provider-tally">
                            <span style={{ color: "#17bf63" }}>{counts.FOR}F</span>
                            {" / "}
                            <span style={{ color: "#e0245e" }}>{counts.AGAINST}A</span>
                          </span>
                        </div>
                      ))}
                    </div>
                    {/* By pool breakdown (when viewing all pools) */}
                    {audienceResults.tally_by_pool && Object.keys(audienceResults.tally_by_pool).length > 1 && (
                      <div className="audience-tally-providers" style={{ marginTop: "8px" }}>
                        {Object.entries(audienceResults.tally_by_pool).map(([poolId, counts]) => (
                          <div key={poolId} className="audience-provider-row">
                            <span className="audience-provider-name">{poolId === "general" ? "General" : poolId === "expert" ? "Expert" : poolId}</span>
                            <span className="audience-provider-tally">
                              <span style={{ color: "#17bf63" }}>{counts.FOR}F</span>
                              {" / "}
                              <span style={{ color: "#e0245e" }}>{counts.AGAINST}A</span>
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>

                  {/* Individual votes — 3-column grid by provider */}
                  <div className="audience-grid">
                    {(["groq", "openai", "anthropic"] as const).map(prov => {
                      const provVotes = audienceResults.votes.filter(v => v.provider === prov);
                      if (provVotes.length === 0) return null;
                      const provLabel = prov === "groq" ? "Llama" : prov === "openai" ? "GPT-5m" : "Haiku";
                      const provTally = audienceResults.tally_by_provider?.[prov];
                      return (
                        <div key={prov} className="audience-grid-column">
                          <div className="audience-grid-col-header">
                            <span className="audience-grid-col-title">{provLabel}</span>
                            {provTally && (
                              <span className="audience-grid-col-tally">
                                <span style={{ color: "#17bf63" }}>{provTally.FOR}</span>
                                {"/"}
                                <span style={{ color: "#e0245e" }}>{provTally.AGAINST}</span>
                                {"/"}
                                <span style={{ color: "#8899a6" }}>{provTally.ABSTAIN || 0}</span>
                              </span>
                            )}
                          </div>
                          {provVotes.map((v, i) => {
                            const seatKey = `${v.provider}-${v.persona}`;
                            const isExpanded = expandedPersona === seatKey;
                            const dotColor = v.vote === "FOR" ? "#17bf63" : v.vote === "AGAINST" ? "#e0245e" : v.vote === "ERROR" ? "#f5a623" : "#8899a6";
                            return (
                              <div
                                key={i}
                                className={`audience-seat${isExpanded ? " audience-seat-expanded" : ""}`}
                                onClick={() => setExpandedPersona(isExpanded ? null : seatKey)}
                                role="button"
                                tabIndex={0}
                                onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setExpandedPersona(isExpanded ? null : seatKey); }}}
                                aria-expanded={isExpanded}
                                aria-label={`${v.persona}: ${v.vote}`}
                              >
                                <div className="audience-seat-summary">
                                  <span className="audience-seat-dot" style={{ background: dotColor }} />
                                  <span className="audience-seat-name">{(v.persona || "").split(" ")[0]}</span>
                                  <span className={`audience-seat-badge audience-seat-badge-${v.vote.toLowerCase()}`}>{v.vote}</span>
                                </div>
                                {isExpanded && (
                                  <div className="audience-seat-detail">
                                    <div className="audience-seat-fullname">{v.persona}</div>
                                    <div className="audience-seat-bg">{v.background}</div>
                                    <div className="audience-seat-rationale">{v.rationale}</div>
                                    {v.latency_ms > 0 && <div className="audience-seat-meta">{v.latency_ms}ms</div>}
                                  </div>
                                )}
                              </div>
                            );
                          })}
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}

              {/* Persona List — grouped by pool, then provider */}
              {!audienceResults && audiencePersonas.length > 0 && (
                <div className="audience-personas">
                  {(audiencePool ? [audiencePool] : [...new Set(audiencePersonas.map(p => p.pool || "general"))]).map(poolId => (
                    <div key={poolId}>
                      {!audiencePool && (
                        <div className="audience-pool-header">
                          {poolId === "general" ? "General Assembly" : poolId === "expert" ? "Expert Panel" : poolId}
                          <span className="audience-pool-count">({audiencePersonas.filter(p => (p.pool || "general") === poolId).length})</span>
                        </div>
                      )}
                      {["groq", "openai", "anthropic"].map(prov => {
                        const personas = audiencePersonas.filter(p => p.provider === prov && (p.pool || "general") === poolId);
                        if (personas.length === 0) return null;
                        return (
                          <div key={prov} className="audience-provider-group">
                            <div className="audience-provider-header">
                              {prov === "groq" ? "Llama 4 Scout (Groq)" : prov === "openai" ? "GPT-5 Mini (OpenAI)" : "Claude Haiku 4.5 (Anthropic)"}
                            </div>
                            {personas.map((p, i) => (
                              <div key={i} className="audience-persona-card">
                                <div className="audience-persona-name">{p.name}</div>
                                <div className="audience-persona-bg">{p.background}</div>
                                <div className="audience-persona-values">Values: {p.values}</div>
                              </div>
                            ))}
                          </div>
                        );
                      })}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}

        {/* Role Create/Edit Form Modal */}
        {roleFormOpen && (
          <div className="briefing-overlay" onClick={() => { setRoleFormOpen(false); resetRoleForm(); }}>
            <div className="role-form-modal" onClick={(e) => e.stopPropagation()}>
              <div className="role-form-header">
                <h2 className="role-form-title">
                  {roleFormEditing ? `Edit Role: ${roleFormEditing}`
                    : roleCreationMode === "interview" ? "Design Role with AI"
                    : roleCreationMode === "wizard" ? "Create New Role"
                    : "Create New Role"}
                </h2>
                <button className="briefing-close-btn" onClick={() => { setRoleFormOpen(false); resetRoleForm(); }}>
                  <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                    <path d="M1 1L13 13M13 1L1 13" stroke="currentColor" strokeWidth="2" strokeLinecap="round"/>
                  </svg>
                </button>
              </div>
              <div className="role-form-body">
                {roleFormError && (
                  <div className="role-form-error">{roleFormError}</div>
                )}

                {/* Mode selection screen — choose wizard or AI interview */}
                {!roleFormEditing && roleCreationMode === null && (
                  <div className="role-mode-selection">
                    <p className="role-mode-hint">How would you like to create this role?</p>
                    <div className="role-mode-cards">
                      <button className="role-mode-card role-mode-card-primary" onClick={() => {
                        setRoleCreationMode("interview");
                        setInterviewMessages([{ role: "assistant", content: "I'll help you design a new team role. What kind of work do you need this agent to do? Describe the problems it should solve or the gaps in your current team." }]);
                      }}>
                        <div className="role-mode-card-icon">
                          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
                          </svg>
                        </div>
                        <div className="role-mode-card-title">Design with AI</div>
                        <div className="role-mode-card-desc">An AI will interview you to design the perfect role.</div>
                      </button>
                      <button className="role-mode-card" onClick={() => { setRoleCreationMode("wizard"); setRoleFormStep(0); }}>
                        <div className="role-mode-card-icon">
                          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                            <path d="M12 3h7a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7"/>
                            <path d="M8 10h8"/><path d="M8 14h8"/><path d="M8 18h4"/>
                          </svg>
                        </div>
                        <div className="role-mode-card-title">Build Manually</div>
                        <div className="role-mode-card-desc">Step through a form to configure every detail. Best for power users who know exactly what they want.</div>
                      </button>
                    </div>
                  </div>
                )}

                {/* AI Interview chat UI — uses backend API (Claude Sonnet 4.5) */}
                {roleCreationMode === "interview" && (
                  <div className="role-interview">
                    <div className="role-interview-chat" ref={interviewChatRef}>
                      {interviewMessages.map((msg, i) => (
                        <div key={i} className={`role-interview-msg role-interview-msg-${msg.role}`}>
                          <div className="role-interview-msg-label">{msg.role === "user" ? "You" : "AI Designer"}</div>
                          <div className="role-interview-msg-content">{msg.content}</div>
                        </div>
                      ))}
                      {interviewLoading && (
                        <div className="role-interview-msg role-interview-msg-assistant">
                          <div className="role-interview-msg-label">AI Designer</div>
                          <div className="role-interview-msg-content role-interview-typing">Thinking...</div>
                        </div>
                      )}
                    </div>
                    {interviewConfig && (
                      <div className="role-interview-config">
                        <div className="role-interview-config-header">Generated Role Configuration</div>
                        <div className="role-interview-config-field"><strong>Title:</strong> {interviewConfig.title}</div>
                        <div className="role-interview-config-field"><strong>Slug:</strong> {interviewConfig.slug}</div>
                        <div className="role-interview-config-field"><strong>Description:</strong> {interviewConfig.description}</div>
                        <div className="role-interview-config-field"><strong>Tags:</strong> {interviewConfig.tags.join(", ")}</div>
                        <div className="role-interview-config-field"><strong>Permissions:</strong> {interviewConfig.permissions.join(", ")}</div>
                        <div className="role-interview-config-field"><strong>Max Instances:</strong> {interviewConfig.max_instances}</div>
                        <details className="role-interview-config-briefing">
                          <summary>Briefing Preview</summary>
                          <pre className="role-interview-config-briefing-text">{interviewConfig.briefing}</pre>
                        </details>
                        <button
                          className="role-form-save"
                          onClick={handleInterviewCreate}
                          disabled={roleFormSaving}
                        >{roleFormSaving ? "Creating..." : "Create Role"}</button>
                      </div>
                    )}
                    <div className="role-interview-input-row">
                      <input
                        className="role-form-input role-interview-input"
                        type="text"
                        value={interviewInput}
                        onChange={(e) => setInterviewInput(e.target.value)}
                        onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendInterviewMessage(); } }}
                        placeholder={interviewConfig ? "Ask for changes, or click Create Role..." : "Describe what you need..."}
                        disabled={interviewLoading}
                        autoFocus
                      />
                      <button
                        className="role-interview-send-btn"
                        onClick={sendInterviewMessage}
                        disabled={interviewLoading || !interviewInput.trim()}
                      >Send</button>
                    </div>
                  </div>
                )}

                {/* Wizard mode — existing step-by-step form */}
                {roleCreationMode === "wizard" && (
                <>
                {/* Step indicator */}
                <div className="wizard-steps">
                  {WIZARD_STEPS.map((label, i) => (
                    <button
                      key={label}
                      className={`wizard-step-dot ${i === roleFormStep ? "wizard-step-active" : ""} ${i < roleFormStep ? "wizard-step-done" : ""}`}
                      onClick={() => { if (i < roleFormStep) setRoleFormStep(i); }}
                      title={label}
                      disabled={i > roleFormStep}
                    >
                      <span className="wizard-step-num">{i + 1}</span>
                      <span className="wizard-step-label">{label}</span>
                    </button>
                  ))}
                </div>

                {/* Step 0 (create only): Template picker */}
                {WIZARD_STEPS[roleFormStep] === "Template" && (
                  <div className="wizard-step-content">
                    <h3 className="wizard-step-heading">Start from a template?</h3>
                    <p className="wizard-step-hint">Templates pre-fill all 7 steps — name, description, tags, permissions, and briefing. Pick the closest match, then customize. Or start from scratch for full control.</p>
                    <div className="role-form-templates">
                      {ROLE_TEMPLATES.map((t) => (
                        <button
                          key={t.id}
                          className="role-template-chip"
                          onClick={() => applyTemplate(t)}
                          title={t.description}
                        >
                          <span className="role-template-chip-title">{t.title}</span>
                          <span className="role-template-chip-desc">{t.description}</span>
                        </button>
                      ))}
                    </div>
                    <button
                      className="wizard-scratch-btn"
                      onClick={() => setRoleFormStep(1)}
                    >Start from scratch</button>
                  </div>
                )}

                {/* Step: Name */}
                {WIZARD_STEPS[roleFormStep] === "Name" && (
                  <div className="wizard-step-content">
                    <h3 className="wizard-step-heading">What should this role be called?</h3>
                    <p className="wizard-step-hint">A good role name describes what this agent IS, not what it does. "Security Auditor" is better than "Code Scanner".</p>
                    <div className="role-form-field">
                      <label className="role-form-label">Title</label>
                      <input
                        className="role-form-input"
                        type="text"
                        value={roleFormTitle}
                        onChange={(e) => setRoleFormTitle(e.target.value)}
                        placeholder="e.g. Security Auditor"
                        disabled={roleFormSaving}
                        autoFocus
                      />
                      {!roleFormEditing && roleFormSlug && (
                        <span className="role-form-slug-preview">Slug: {roleFormSlug}</span>
                      )}
                    </div>
                  </div>
                )}

                {/* Step: Description */}
                {WIZARD_STEPS[roleFormStep] === "Description" && (
                  <div className="wizard-step-content">
                    <h3 className="wizard-step-heading">Define this role's purpose</h3>
                    <p className="wizard-step-hint">Take your time here. These three answers become the foundation of the agent's briefing — they determine how it behaves, what it avoids, and how it coordinates with teammates.</p>
                    <div className="role-form-field">
                      <label className="role-form-label">What specific problems will this role solve?</label>
                      <p className="wizard-field-guidance">This becomes the opening line of the briefing. It tells the agent what to focus on day-to-day. Be specific — "reviews PRs for SQL injection" is better than "does security stuff."</p>
                      <textarea
                        className="role-form-textarea role-form-textarea-sm"
                        value={roleFormPurpose}
                        onChange={(e) => setRoleFormPurpose(e.target.value)}
                        placeholder="e.g. Reviews all pull requests for security vulnerabilities before they can be merged..."
                        rows={3}
                        disabled={roleFormSaving}
                        autoFocus
                      />
                    </div>
                    <div className="role-form-field">
                      <label className="role-form-label">What should this role explicitly NOT do?</label>
                      <p className="wizard-field-guidance">Critical for preventing agents from overstepping. These become hard "NEVER do X" rules in the briefing's anti-patterns section.</p>
                      <textarea
                        className="role-form-textarea role-form-textarea-sm"
                        value={roleFormBoundaries}
                        onChange={(e) => setRoleFormBoundaries(e.target.value)}
                        placeholder="e.g. Should not modify code directly — only flag issues for developers to fix..."
                        rows={3}
                        disabled={roleFormSaving}
                      />
                    </div>
                    <div className="role-form-field">
                      <label className="role-form-label">How does this role differ from existing roles?</label>
                      <p className="wizard-field-guidance">Prevents overlap and confusion between team members. The briefing uses this to define peer relationships — who this role works with and how.</p>
                      <textarea
                        className="role-form-textarea role-form-textarea-sm"
                        value={roleFormDifferentiator}
                        onChange={(e) => setRoleFormDifferentiator(e.target.value)}
                        placeholder="e.g. Unlike the Tester who validates functionality, this role focuses specifically on security..."
                        rows={3}
                        disabled={roleFormSaving}
                      />
                    </div>
                    {roleFormDesc && !roleFormPurpose && (
                      <div className="wizard-prefilled-note">Pre-filled from template. Edit the fields above to customize.</div>
                    )}
                  </div>
                )}

                {/* Step: Capabilities (tags) */}
                {WIZARD_STEPS[roleFormStep] === "Capabilities" && (
                  <div className="wizard-step-content">
                    <h3 className="wizard-step-heading">What capabilities does it need?</h3>
                    <p className="wizard-step-hint">Each capability shapes the agent's behavior, anti-patterns, and peer relationships. Choose carefully.</p>
                    <div className="role-form-tags">
                      {CANONICAL_TAGS.map((tag) => (
                        <button
                          key={tag.id}
                          className={`role-tag-chip ${roleFormTags.includes(tag.id) ? "role-tag-chip-active" : ""}`}
                          onClick={() => {
                            if (roleFormTags.includes(tag.id)) {
                              setRoleFormTags(roleFormTags.filter(t => t !== tag.id));
                            } else {
                              setRoleFormTags([...roleFormTags, tag.id]);
                            }
                          }}
                          disabled={roleFormSaving}
                        >
                          <span className="role-tag-chip-label">{tag.label}</span>
                          <span className="role-tag-chip-desc">{TAG_SCENARIOS[tag.id] || tag.description}</span>
                        </button>
                      ))}
                    </div>
                    {roleFormTags.filter(t => !CANONICAL_TAGS.some(c => c.id === t)).length > 0 && (
                      <div className="role-form-custom-tags">
                        {roleFormTags.filter(t => !CANONICAL_TAGS.some(c => c.id === t)).map(tag => (
                          <span key={tag} className="role-tag-chip role-tag-chip-active role-tag-chip-custom">
                            {tag}
                            <button className="role-tag-remove" onClick={() => setRoleFormTags(roleFormTags.filter(t => t !== tag))}>&times;</button>
                          </span>
                        ))}
                      </div>
                    )}
                    <div className="role-form-custom-tag-input">
                      <input
                        className="role-form-input"
                        type="text"
                        value={roleFormCustomTag}
                        onChange={(e) => setRoleFormCustomTag(e.target.value)}
                        onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); addCustomTag(); } }}
                        placeholder="Add custom tag..."
                        disabled={roleFormSaving}
                      />
                      <button
                        className="role-form-add-tag-btn"
                        onClick={addCustomTag}
                        disabled={roleFormSaving || !roleFormCustomTag.trim()}
                      >Add</button>
                    </div>
                    {/* Live peer connections preview — shows which existing roles share selected tags */}
                    {roleFormTags.length > 0 && project && (() => {
                      const peerConnections: { tag: string; peers: string[] }[] = [];
                      for (const tag of roleFormTags) {
                        const peers = Object.entries(project.config.roles)
                          .filter(([slug]) => slug !== roleFormEditing && (project.config.roles[slug] as any).tags?.includes(tag))
                          .map(([slug]) => (project.config.roles[slug] as any).title || slug);
                        if (peers.length > 0) {
                          peerConnections.push({ tag, peers });
                        }
                      }
                      if (peerConnections.length === 0) return null;
                      return (
                        <div className="role-peer-connections-preview" aria-label="Peer connections based on selected tags">
                          <span className="role-peer-connections-label">Peer connections from these tags:</span>
                          {peerConnections.map(({ tag, peers }) => (
                            <div key={tag} className="role-peer-connection-row">
                              <span className="role-peer-tag">{tag}</span>
                              <span className="role-peer-names">{peers.join(", ")}</span>
                            </div>
                          ))}
                        </div>
                      );
                    })()}
                  </div>
                )}

                {/* Step: Permissions */}
                {WIZARD_STEPS[roleFormStep] === "Permissions" && (
                  <div className="wizard-step-content">
                    <h3 className="wizard-step-heading">What can it do on the team?</h3>
                    <p className="wizard-step-hint">Each permission grants a specific ability. Unchecked permissions are explicitly denied. These were pre-selected based on your capability tags — adjust as needed.</p>
                    <div className="role-form-perms">
                      {AVAILABLE_PERMISSIONS.map((perm) => (
                        <label key={perm} className="role-form-perm-checkbox">
                          <input
                            type="checkbox"
                            checked={roleFormPerms.includes(perm)}
                            onChange={(e) => {
                              if (e.target.checked) {
                                setRoleFormPerms([...roleFormPerms, perm]);
                              } else {
                                setRoleFormPerms(roleFormPerms.filter(p => p !== perm));
                              }
                            }}
                            disabled={roleFormSaving}
                          />
                          <div className="role-form-perm-details">
                            <span className="role-form-perm-label">{perm}</span>
                            <span className="role-form-perm-what">{PERM_TOOLTIPS[perm] || ""}</span>
                            {!roleFormPerms.includes(perm) && (
                              <span className="role-form-perm-consequence">{PERM_CONSEQUENCES[perm] || ""}</span>
                            )}
                          </div>
                        </label>
                      ))}
                    </div>
                  </div>
                )}

                {/* Step: Instances */}
                {WIZARD_STEPS[roleFormStep] === "Instances" && (
                  <div className="wizard-step-content">
                    <h3 className="wizard-step-heading">How many simultaneous instances?</h3>
                    <p className="wizard-step-hint">Multiple instances let you parallelize work. Implementation roles benefit from 2-3 instances working on different files simultaneously. Review and coordination roles work best with 1 instance for consistency.</p>
                    {project && (
                      <div className="wizard-team-context">
                        <span className="wizard-team-context-label">Current team:</span>
                        {Object.entries(project.config.roles).map(([slug, role]) => (
                          <span key={slug} className="wizard-team-context-role">
                            {role.max_instances} {role.title}{role.max_instances > 1 ? "s" : ""}
                          </span>
                        ))}
                      </div>
                    )}
                    <div className="role-form-field">
                      <label className="role-form-label">Max Instances</label>
                      <input
                        className="role-form-input role-form-input-narrow"
                        type="number"
                        min={1}
                        value={roleFormMaxInst}
                        onChange={(e) => setRoleFormMaxInst(parseInt(e.target.value) || 1)}
                        disabled={roleFormSaving}
                        autoFocus
                      />
                    </div>
                  </div>
                )}

                {/* Step: Briefing review */}
                {WIZARD_STEPS[roleFormStep] === "Briefing" && (
                  <div className="wizard-step-content">
                    <h3 className="wizard-step-heading">Review the briefing</h3>
                    <div className="wizard-briefing-note">
                      The briefing is the single most important artifact. An agent's entire behavior is shaped by this document. Take time to customize it.
                    </div>
                    <div className="role-form-field">
                      <div className="role-form-label-row">
                        <label className="role-form-label">Briefing (Markdown)</label>
                        <button
                          className="role-form-generate-btn"
                          onClick={() => setRoleFormBriefing(generateBriefingFromForm(roleFormTitle, roleFormDesc, roleFormTags, roleFormPerms))}
                          disabled={roleFormSaving || !roleFormTitle}
                          title="Auto-generate briefing from role metadata"
                        >Regenerate</button>
                      </div>
                      <div className="wizard-briefing-sections">
                        <div className="wizard-briefing-legend">
                          <span className="wizard-briefing-legend-item wizard-legend-tags">From tags</span>
                          <span className="wizard-briefing-legend-item wizard-legend-perms">From permissions</span>
                          <span className="wizard-briefing-legend-item wizard-legend-peers">From peer roles</span>
                        </div>
                      </div>
                      <textarea
                        className="role-form-textarea role-form-textarea-lg"
                        value={roleFormBriefing}
                        onChange={(e) => setRoleFormBriefing(e.target.value)}
                        placeholder="Click 'Regenerate' to auto-create a briefing from your role settings, or write your own..."
                        rows={18}
                        disabled={roleFormSaving}
                      />
                    </div>
                  </div>
                )}

                {/* Navigation */}
                <div className="wizard-nav">
                  {roleFormStep > 0 ? (
                    <button
                      className="wizard-nav-back"
                      onClick={() => setRoleFormStep(roleFormStep - 1)}
                      disabled={roleFormSaving}
                    >Back</button>
                  ) : <div />}
                  {WIZARD_STEPS[roleFormStep] === "Briefing" ? (
                    <button
                      className="role-form-save"
                      onClick={handleRoleFormSave}
                      disabled={roleFormSaving || (!roleFormEditing && !roleFormSlug)}
                    >
                      {roleFormSaving ? "Saving..." : roleFormEditing ? "Save Changes" : "Create Role"}
                    </button>
                  ) : WIZARD_STEPS[roleFormStep] !== "Template" ? (
                    <button
                      className="wizard-nav-next"
                      onClick={() => {
                        const currentLabel = WIZARD_STEPS[roleFormStep];
                        const nextStep = roleFormStep + 1;
                        const nextLabel = WIZARD_STEPS[nextStep];
                        // Compose description from sub-questions when leaving Description step
                        if (currentLabel === "Description" && (roleFormPurpose || roleFormBoundaries || roleFormDifferentiator)) {
                          setRoleFormDesc(composeDescription());
                        }
                        // Apply smart defaults when entering permissions step
                        if (nextLabel === "Permissions" && !roleFormEditing) {
                          const suggested = smartPermsFromTags(roleFormTags);
                          setRoleFormPerms(suggested);
                        }
                        // Apply smart instance defaults when entering instances step
                        if (nextLabel === "Instances" && !roleFormEditing) {
                          setRoleFormMaxInst(smartInstancesFromTags(roleFormTags));
                        }
                        // Auto-generate briefing when entering briefing step
                        if (nextLabel === "Briefing") {
                          // Use composed description (may not be in state yet due to batching)
                          const desc = (roleFormPurpose || roleFormBoundaries || roleFormDifferentiator)
                            ? composeDescription()
                            : roleFormDesc;
                          setRoleFormBriefing(generateBriefingFromForm(roleFormTitle, desc, roleFormTags, roleFormPerms));
                        }
                        setRoleFormStep(nextStep);
                      }}
                      disabled={roleFormSaving || !canAdvanceStep()}
                    >Next</button>
                  ) : null}
                </div>
                </>
                )}
              </div>
            </div>
          </div>
        )}

        {/* Companion Launch Dialog */}
        {companionLaunch && (
          <div className="confirm-dialog" onClick={() => setCompanionLaunch(null)}>
            <div className="confirm-dialog-box companion-launch-dialog" onClick={(e) => e.stopPropagation()}>
              <div className="confirm-dialog-title">Launch {project?.config?.roles?.[companionLaunch.role]?.title || companionLaunch.role}</div>
              <div className="confirm-dialog-message">This role has companion agents that can launch with it.</div>
              <div className="companion-launch-options">
                {companionLaunch.companions.map((comp) => (
                  <label key={comp.role} className="companion-launch-option">
                    <input
                      type="checkbox"
                      checked={comp.enabled}
                      disabled={!comp.optional}
                      onChange={(e) => {
                        setCompanionLaunch({
                          ...companionLaunch,
                          companions: companionLaunch.companions.map(c =>
                            c.role === comp.role ? { ...c, enabled: e.target.checked } : c
                          ),
                        });
                      }}
                    />
                    <span className="companion-launch-label">
                      Include {project?.config?.roles?.[comp.role]?.title || comp.role}
                    </span>
                    {project?.config?.roles?.[comp.role]?.description && (
                      <span className="companion-launch-desc">{project.config.roles[comp.role].description}</span>
                    )}
                  </label>
                ))}
              </div>
              <div className="confirm-dialog-actions">
                <button className="confirm-dialog-cancel" onClick={() => setCompanionLaunch(null)}>Cancel</button>
                <button className="confirm-dialog-delete" onClick={handleCompanionLaunchConfirm}>Launch</button>
              </div>
            </div>
          </div>
        )}

        {/* Start Discussion Dialog */}
        {startDiscussionOpen && (
          <div className="confirm-dialog" onClick={() => setStartDiscussionOpen(false)}>
            <div className="start-discussion-dialog" onClick={(e) => e.stopPropagation()}>
              <div className="confirm-dialog-title">Start Discussion</div>

              {/* Format selection */}
              <div className="sd-section-label">Format</div>
              <div className="sd-format-grid">
                {([
                  ["delphi", "Delphi", "Blind rounds with anonymized aggregation. Best for reducing groupthink."],
                  ["oxford", "Oxford", "Public adversarial debate with FOR/AGAINST teams. Best for stress-testing ideas."],
                  ["red_team", "Red Team", "All participants attack a proposal. Best for finding weaknesses."],
                  ["continuous", "Continuous", "Auto-triggered micro-reviews from status messages. Best for ongoing code review."],
                ] as const).map(([id, label, desc]) => (
                  <button
                    key={id}
                    className={`sd-format-card${sdFormat === id ? " sd-format-active" : ""}`}
                    onClick={() => setSdFormat(id)}
                    aria-pressed={sdFormat === id}
                  >
                    <span className="sd-format-card-label">{label}</span>
                    <span className="sd-format-card-desc">{desc}</span>
                  </button>
                ))}
              </div>

              {/* Topic input */}
              {sdFormat !== "continuous" && (
                <>
                  <div className="sd-section-label">Topic / Proposition</div>
                  <input
                    className="sd-topic-input"
                    type="text"
                    placeholder={sdFormat === "oxford" ? "Resolved: ..." : "What should we discuss?"}
                    value={sdTopic}
                    onChange={(e) => setSdTopic(e.target.value)}
                    autoFocus
                  />
                </>
              )}

              {/* Participant selection */}
              <div className="sd-section-label">
                Participants ({Object.values(sdParticipants).filter(Boolean).length} selected)
              </div>
              <div className="sd-participant-list">
                {Object.keys(sdParticipants).map((pid) => {
                  const [role, inst] = pid.split(":");
                  const roleConfig = project?.config?.roles?.[role];
                  return (
                    <label key={pid} className="sd-participant-row">
                      <input
                        type="checkbox"
                        checked={sdParticipants[pid] || false}
                        onChange={(e) => setSdParticipants(prev => ({ ...prev, [pid]: e.target.checked }))}
                      />
                      <span className="sd-participant-name" style={{ color: getRoleColor(role) }}>
                        {roleConfig?.title || role}:{inst}
                      </span>
                    </label>
                  );
                })}
                {Object.keys(sdParticipants).length === 0 && (
                  <span className="sd-no-participants">No active team members. Launch agents first.</span>
                )}
              </div>

              {/* Actions */}
              <div className="confirm-dialog-actions">
                <button className="confirm-dialog-cancel" onClick={() => setStartDiscussionOpen(false)}>Cancel</button>
                <button
                  className="sd-start-btn"
                  onClick={handleStartDiscussion}
                  disabled={sdStarting || (sdFormat !== "continuous" && !sdTopic.trim()) || Object.values(sdParticipants).filter(Boolean).length === 0}
                >
                  {sdStarting ? "Starting..." : "Start Discussion"}
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Confirmation Dialog */}
        {confirmAction && (
          <div className="confirm-dialog" onClick={() => { setConfirmAction(null); setConfirmTypedInput(""); }}>
            <div className="confirm-dialog-box" onClick={(e) => e.stopPropagation()}>
              <div className="confirm-dialog-title">{confirmAction.title}</div>
              <div className="confirm-dialog-message">{confirmAction.message}</div>
              {confirmAction.requireTyped && (
                <input
                  className="confirm-dialog-typed-input"
                  type="text"
                  value={confirmTypedInput}
                  onChange={(e) => setConfirmTypedInput(e.target.value)}
                  placeholder={confirmAction.requireTyped}
                  autoFocus
                  aria-label={`Type "${confirmAction.requireTyped}" to confirm deletion`}
                />
              )}
              <div className="confirm-dialog-actions">
                <button className="confirm-dialog-cancel" onClick={() => { setConfirmAction(null); setConfirmTypedInput(""); }}>Cancel</button>
                <button
                  className="confirm-dialog-delete"
                  onClick={confirmAction.onConfirm}
                  disabled={!!confirmAction.requireTyped && confirmTypedInput !== confirmAction.requireTyped}
                >{confirmAction.confirmLabel || "Delete"}</button>
              </div>
            </div>
          </div>
        )}

        {/* Interrupt Dialog */}
        {/* Create Group Modal */}
        {createGroupOpen && (
          <div className="confirm-dialog" onClick={() => setCreateGroupOpen(false)}>
            <div className="confirm-dialog-box" style={{ maxWidth: 460 }} onClick={(e) => e.stopPropagation()}>
              <div className="confirm-dialog-title">Create Role Group</div>
              <div className="create-group-form">
                <div className="create-group-row">
                  <input
                    className="create-group-icon-input"
                    type="text"
                    value={newGroupIcon}
                    onChange={(e) => setNewGroupIcon(e.target.value)}
                    title="Group icon (emoji)"
                    maxLength={2}
                  />
                  <input
                    className="create-group-name-input"
                    type="text"
                    placeholder="Group name"
                    value={newGroupName}
                    onChange={(e) => setNewGroupName(e.target.value)}
                    autoFocus
                  />
                </div>
                <input
                  className="create-group-desc-input"
                  type="text"
                  placeholder="Short description"
                  value={newGroupDesc}
                  onChange={(e) => setNewGroupDesc(e.target.value)}
                />
                <div className="create-group-roles-label">Select roles:</div>
                <div className="create-group-roles">
                  {project && Object.entries(project.config.roles).map(([slug, role]) => (
                    <label key={slug} className="group-role-chip">
                      <input
                        type="checkbox"
                        checked={(newGroupRoles[slug] || 0) > 0}
                        onChange={(e) => setNewGroupRoles(prev => ({ ...prev, [slug]: e.target.checked ? Math.max(prev[slug] || 0, 1) : 0 }))}
                      />
                      <span className="group-role-chip-dot" style={{ background: getRoleColor(slug) }} />
                      <span>{role.title}</span>
                      {(newGroupRoles[slug] || 0) > 0 && (
                        <input
                          type="number"
                          className="create-group-count-input"
                          min={1}
                          max={5}
                          value={newGroupRoles[slug] || 1}
                          onChange={(e) => setNewGroupRoles(prev => ({ ...prev, [slug]: parseInt(e.target.value) || 1 }))}
                          onClick={(e) => e.stopPropagation()}
                        />
                      )}
                    </label>
                  ))}
                </div>
                <div className="confirm-dialog-actions">
                  <button className="confirm-dialog-cancel" onClick={() => setCreateGroupOpen(false)}>Cancel</button>
                  <button
                    className="group-card-deploy-btn"
                    onClick={handleSaveCustomGroup}
                    disabled={!newGroupName.trim() || !Object.values(newGroupRoles).some(v => v > 0)}
                  >Save Group</button>
                </div>
              </div>
            </div>
          </div>
        )}

        {interruptTarget && (
          <div className="confirm-dialog" onClick={() => { setInterruptTarget(null); setInterruptReason(""); }}>
            <div className="interrupt-dialog-box" onClick={(e) => e.stopPropagation()}>
              <div className="interrupt-dialog-title">Interrupt {interruptTarget.title}:{interruptTarget.instance}</div>
              <div className="interrupt-dialog-message">This agent will stop their current work immediately.</div>
              <input
                className="interrupt-dialog-input"
                type="text"
                placeholder="What should they do instead? (optional)"
                value={interruptReason}
                onChange={(e) => setInterruptReason(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleSendInterrupt()}
                autoFocus
              />
              <div className="confirm-dialog-actions">
                <button className="confirm-dialog-cancel" onClick={() => { setInterruptTarget(null); setInterruptReason(""); }}>Cancel</button>
                <button className="interrupt-dialog-send" onClick={handleSendInterrupt}>Send Interrupt</button>
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
                      {loading ? (
                        <>
                          <span className="saved-project-name">Connecting&hellip;</span>
                          <span className="saved-project-path">{proj.path}</span>
                        </>
                      ) : (
                        <>
                          <span className="saved-project-name">{proj.name}</span>
                          <span className="saved-project-path">{proj.path}</span>
                        </>
                      )}
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
