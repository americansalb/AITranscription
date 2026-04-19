import { useEffect, useLayoutEffect, useState, useRef } from "react";
import { createPortal } from "react-dom";
import type { ParsedProject, BoardMessage, RoleStatus, SessionBinding, QuestionChoice, FileClaim, DiscussionState, Section, RosterSlot, RoleConfig, RoleGroup } from "../lib/collabTypes";
import { parseModeratorError } from "../lib/collabTypes";
import { BUILTIN_ROLE_GROUPS } from "../utils/roleGroupPresets";
import { RoleBriefingModal } from "./RoleBriefingModal";
import { getAvailableVoices, fetchAvailableVoices, getDefaultVoice } from "../lib/queueStore";
import { CANONICAL_TAGS, ROLE_TEMPLATES, generateBriefing, type PeerRole, type RoleTemplate } from "../utils/briefingGenerator";
import { trimVoiceAssignments } from "../lib/storageManager";
import { DiscussionPanel } from "./DiscussionPanel";
import { EndSessionConfirmModal } from "./EndSessionConfirmModal";
import { QuickLaunchBar } from "./QuickLaunchBar";
import { BuildIdentityFooter } from "./BuildIdentityFooter";
import PreviousTeamBanner from "./PreviousTeamBanner";
import { type SequenceTurnState } from "./SequenceBanner";
import { type ModeratorSequencePanelRosterEntry } from "./ModeratorSequencePanel";
import SequenceSessionCard from "./SequenceSessionCard";
import StartSequenceModal, { type StartSequenceCandidate } from "./StartSequenceModal";
import "../styles/collab.css";

// pr-reason-params + pr-reason-relax: shared contract with the Rust
// `normalize_action_reason` helper in main.rs. The number reflects the
// backend's "use caller value if ≥N chars after trim, else fall back to
// the action default." Backend never rejects on reason — this is the
// threshold at which a caller-supplied reason is considered informative
// enough to override the default.
export const MODERATOR_REASON_MIN_CHARS = 3;

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

const ETHEREAL_ROLES: readonly { slug: string; label: string }[] = [];

// Discussion-bound agents: auto-start when a discussion begins, auto-stop when it ends
const DISCUSSION_BOUND_AGENTS = ["moderator", "audience"] as const;

function getWorkflowDisplay(type?: string, customColors?: Record<string, string>): { label: string; color: string } {
  if (type && WORKFLOW_TYPES[type]) {
    const color = customColors?.[type] || WORKFLOW_TYPES[type].color;
    return { label: WORKFLOW_TYPES[type].label, color };
  }
  return { label: "No Workflow", color: "#657786" };
}

/** Format a message sender, showing persona name for audience messages */
function formatSender(msg: { from: string; metadata?: Record<string, any> }): string {
  const persona = msg.metadata?.persona;
  if (persona && msg.from.startsWith("audience:")) {
    // Capitalize first letter of persona name
    const label = typeof persona === "string" ? persona.charAt(0).toUpperCase() + persona.slice(1) : persona;
    return `${msg.from} (${label})`;
  }
  return msg.from;
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
  if (status === "ready") return "project-status-dot ready";
  if (status === "active") return "project-status-dot working"; // Rust role-level → treat as working
  if (status === "vacant") return "project-status-dot vacant";
  return "project-status-dot vacant"; // unknown → vacant
}

/** Human-friendly status label for display */
function getStatusLabel(status: string): string {
  if (status === "working") return "Working";
  if (status === "ready") return "Ready";
  if (status === "active") return "Working"; // Rust role-level
  if (status === "vacant") return "Not started";
  return "Offline";
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
  status: "working" | "ready" | "vacant";
  roleColor: string;
}

function computeInstanceStatus(
  session: SessionBinding,
  _timeoutSecs: number,
  nowSecs: number
): InstanceCard["status"] {
  const hbEpoch = new Date(session.last_heartbeat).getTime() / 1000;
  const age = nowSecs - hbEpoch;

  // Immediate detection: process wrote "disconnected" on exit
  if (session.activity === "disconnected") return "vacant";

  // No contact for 10 minutes → disconnected (vacant)
  const disconnectThreshold = 600; // 10 min
  if (age > disconnectThreshold) return "vacant";

  // Working: agent is actively calling tools / doing work
  if (session.activity === "working") {
    return "working";
  }

  // Standby → "ready": agent is in project_wait, available for tasks
  if (session.activity === "standby") {
    // Smooth transition: if agent was working within last 30s, keep showing "working"
    const lwAt = session.last_working_at;
    if (lwAt) {
      const workAge = nowSecs - new Date(lwAt).getTime() / 1000;
      if (workAge < 30) return "working";
    }
    return "ready";
  }

  // Fallback: no activity field — if heartbeat is recent, assume ready
  if (age > disconnectThreshold) return "vacant";
  return "ready";
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
          {formatSender(msg)}
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
      heartbeat_timeout_seconds: 300,
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

  /** Click a sender name to auto-fill compose bar as a DM reply */
  const handleReplyTo = (senderIdentity: string) => {
    if (senderIdentity === "system" || senderIdentity === "system:0") return;
    const slug = senderIdentity.split(":")[0];
    // Check if this role has multiple instances — if so use full identity, else just slug
    const roleStatus = project?.role_statuses.find((r: RoleStatus) => r.slug === slug);
    const target = roleStatus && roleStatus.active_instances > 1 ? senderIdentity : slug;
    setMsgTo(target);
    composeInputRef.current?.focus();
  };

  const [workflowDropdownOpen, setWorkflowDropdownOpen] = useState(false);
  const [discussionModeOpen, setDiscussionModeOpen] = useState(false);
  const [workModeOpen, setWorkModeOpen] = useState(false);
  const [turnState, setTurnState] = useState<{
    relevance_order?: string[];
    current_index?: number;
    responded?: string[];
    passed?: string[];
    completed?: boolean;
  } | null>(null);
  const [discussionState, setDiscussionState] = useState<DiscussionState | null>(null);
  const [closingRound, setClosingRound] = useState(false);
  const [continuousTimeout, setContinuousTimeout] = useState(60);
  const [startDiscussionOpen, setStartDiscussionOpen] = useState(false);
  const [sdFormat, setSdFormat] = useState<"delphi" | "oxford" | "red_team" | "continuous">("delphi");
  const [sdTopic, setSdTopic] = useState("");
  const [sdParticipants, setSdParticipants] = useState<Record<string, boolean>>({});
  const [sdStarting, setSdStarting] = useState(false);
  const [sdModeratorEnabled, setSdModeratorEnabled] = useState(true);
  const [sdAudienceEnabled, setSdAudienceEnabled] = useState(false);
  const [sdAudiencePool, setSdAudiencePool] = useState("");
  const [sdAudienceSize, setSdAudienceSize] = useState(5);
  const [tauriPools, setTauriPools] = useState<Array<{ id: string; name: string; persona_count: number; providers: string[] }>>([]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [interruptTarget, setInterruptTarget] = useState<{ slug: string; instance: number; title: string } | null>(null);
  const [interruptReason, setInterruptReason] = useState("");
  const [buzzedKey, setBuzzedKey] = useState<string | null>(null);
  // Auto-buzz watchdog: tracks which agents have been auto-buzzed in the current staleness episode
  const [openCardMenu, setOpenCardMenu] = useState<string | null>(null); // "slug:instance" key
  const [menuPos, setMenuPos] = useState<{ top: number; right: number; left: number } | null>(null);
  const [claimsCollapsed, setClaimsCollapsed] = useState(true);
  const [_addTeamTab, _setAddTeamTab] = useState<"groups" | "roles">("groups");
  const [expandedGroup, setExpandedGroup] = useState<string | null>(null);
  const [groupRoleChecked, setGroupRoleChecked] = useState<Record<string, boolean>>({});
  const [groupSearch, setGroupSearch] = useState("");
  const [rosterViewMode, setRosterViewMode] = useState<"grid" | "list" | "chip">(() => {
    try {
      const saved = localStorage.getItem("vaak_roster_view_mode");
      if (saved === "grid" || saved === "list" || saved === "chip") return saved;
    } catch { /* ignore */ }
    return "grid";
  });
  const updateRosterViewMode = (mode: "grid" | "list" | "chip") => {
    setRosterViewMode(mode);
    try { localStorage.setItem("vaak_roster_view_mode", mode); } catch { /* ignore */ }
  };
  const [treeExpanded, setTreeExpanded] = useState<Set<string>>(new Set());
  const [teamSectionOpen, setTeamSectionOpen] = useState(false);
  const [createGroupOpen, setCreateGroupOpen] = useState(false);
  const [editingGroupSlug, setEditingGroupSlug] = useState<string | null>(null);
  const [importRolesStatus, setImportRolesStatus] = useState<string | null>(null);
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
  const sectionDropdownRef = useRef<HTMLDivElement>(null);
  const [sectionDropdownOpen, setSectionDropdownOpen] = useState(false);
  const [supervisionOpen, setSupervisionOpen] = useState(false);
  const [etherealSettings, setEtherealSettings] = useState<Record<string, boolean>>(() => {
    const saved: Record<string, boolean> = {};
    for (const role of ETHEREAL_ROLES) {
      const val = localStorage.getItem(`vaak_ethereal_${role.slug}`);
      if (val !== null) saved[role.slug] = val === "true";
    }
    return saved;
  });
  const [audienceCount, setAudienceCount] = useState<number>(() => {
    const saved = localStorage.getItem("vaak_audience_count");
    return saved ? parseInt(saved, 10) || 3 : 3;
  });
  const [audienceProviders, setAudienceProviders] = useState<string[]>(() => {
    try {
      const saved = localStorage.getItem("vaak_audience_providers");
      return saved ? JSON.parse(saved) : ["Claude", "GPT-4o"];
    } catch { return ["Claude", "GPT-4o"]; }
  });
  const [etherealApiKey, setEtherealApiKey] = useState(() =>
    localStorage.getItem("vaak_anthropic_key") || ""
  );
  const [hasEnvKey, setHasEnvKey] = useState(false);
  const [etherealStatuses, setEtherealStatuses] = useState<Record<string, { running: boolean; last_error: string | null; messages_sent: number; last_poll: string | null }>>({});
  const discussionModeRef = useRef<HTMLDivElement>(null);
  const workModeRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messageTimelineRef = useRef<HTMLDivElement>(null);
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [newMsgCount, setNewMsgCount] = useState(0);
  const prevMsgCountRef = useRef(0);
  const savedScrollRef = useRef<number | null>(null);
  const scrollingToBottomRef = useRef(false);
  const MSG_PAGE_SIZE = 50;
  const [visibleMsgLimit, setVisibleMsgLimit] = useState(MSG_PAGE_SIZE);

  // PR H: Human-channel separation tabs
  // Why: the board mixes team-to-team chatter with human-directed messages; users asked
  // for a clean separated view so they can see "what's for me" without scrolling past
  // dozens of role-to-role turns. Filter is client-side only — no backend changes.
  type InboxTab = "human" | "pipeline" | "all";
  const [inboxTab, setInboxTab] = useState<InboxTab>(() => {
    try {
      const saved = localStorage.getItem("vaak_inbox_tab");
      if (saved === "human" || saved === "pipeline" || saved === "all") return saved;
    } catch { /* ignore */ }
    return "all";
  });
  // PR H3: moderator-action error toast state.
  // Why: when handleEndDiscussion / handleTogglePause fail (e.g. non-moderator
  // clicks the button, wrong format, missing reason), the backend returns a
  // tagged error string parseable by parseModeratorError. Previously these
  // failures only logged to console — invisible to the user. Now they surface
  // as a dismissible toast with a human-friendly rendering of each ModeratorErrorCode.
  const [modErrorToast, setModErrorToast] = useState<string | null>(null);
  // PR H3 v2: End Session confirmation modal state.
  // Why: the red End button on DiscussionPanel previously called the destructive
  // end_discussion command on a single click with no friction. Per spec
  // .vaak/specs/pr-h3-moderator-toolbar.md § "End session", a typed-confirm with
  // mandatory reason is the guardrail. Reason is broadcast to the board before
  // the end call so the moderator's rationale is durably recorded even though
  // the Tauri command signature has not yet gained a `reason` parameter
  // (developer msg 374 — deferred item).
  const [endConfirmOpen, setEndConfirmOpen] = useState(false);
  const [endSessionAnnouncement, setEndSessionAnnouncement] = useState<string | null>(null);
  // Clear the polite-region announcement after screen readers have had time
  // to read it (≈3s). Leaving the text in the DOM would prevent re-announcing
  // the same message on a subsequent end.
  useEffect(() => {
    if (!endSessionAnnouncement) return;
    const t = window.setTimeout(() => setEndSessionAnnouncement(null), 3000);
    return () => window.clearTimeout(t);
  }, [endSessionAnnouncement]);
  const showModeratorError = (raw: string) => {
    const parsed = parseModeratorError(raw);
    if (!parsed) {
      setModErrorToast(raw);
      return;
    }
    if (parsed.code === "CAPABILITY_NOT_SUPPORTED_FOR_FORMAT") {
      setModErrorToast(`${parsed.capability ?? "That action"} is not available in ${parsed.format ?? "this"} mode.`);
    } else if (parsed.code === "HUMAN_BYPASS_YIELDS_TO_MODERATOR") {
      const mod = parsed.moderator ?? "the moderator";
      setModErrorToast(`${mod} is the current moderator — route through them or wait for their stage.`);
    } else {
      setModErrorToast(parsed.rawMessage);
    }
  };

  // PR H2: last-seen timestamps per tab drive unread-count badges. Persisted so
  // the badges survive reloads — otherwise every reopen would show zero unread.
  // Why: tab badges are the only at-a-glance signal of "something new for me" —
  // without persistence they reset on every app restart and lose their value.
  const [tabLastSeen, setTabLastSeen] = useState<Record<InboxTab, number>>(() => {
    try {
      const saved = localStorage.getItem("vaak_inbox_last_seen");
      if (saved) return JSON.parse(saved);
    } catch { /* ignore */ }
    return { human: 0, pipeline: 0, all: 0 };
  });
  const changeInboxTab = (tab: InboxTab) => {
    setInboxTab(tab);
    try { localStorage.setItem("vaak_inbox_tab", tab); } catch { /* ignore */ }
    // Mark this tab as seen — clears its unread badge.
    setTabLastSeen(prev => {
      const next = { ...prev, [tab]: Date.now() };
      try { localStorage.setItem("vaak_inbox_last_seen", JSON.stringify(next)); } catch { /* ignore */ }
      return next;
    });
  };

  // PR H2: Ctrl+1 / Ctrl+2 / Ctrl+3 switch tabs. Cmd+digit on macOS.
  // Why: power users asked for keyboard access during active pipelines where
  // the mouse pulls focus away from urgent work — a single keystroke should
  // surface "what's for me" without losing keyboard context.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey) return;
      if (e.key === "1") { e.preventDefault(); changeInboxTab("human"); }
      else if (e.key === "2") { e.preventDefault(); changeInboxTab("pipeline"); }
      else if (e.key === "3") { e.preventDefault(); changeInboxTab("all"); }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  // Team Launcher state
  const [launching, setLaunching] = useState(false);
  const [npmInstalled, setNpmInstalled] = useState<boolean | null>(null);
  const [sequenceTurn, setSequenceTurn] = useState<SequenceTurnState | null>(null);
  const [startSequenceOpen, setStartSequenceOpen] = useState(false);
  const [claudeInstalled, setClaudeInstalled] = useState<boolean | null>(null);
  const [installingCli, setInstallingCli] = useState(false);
  const [installingNode, setInstallingNode] = useState(false);
  const [spawnConsented, setSpawnConsented] = useState(false);
  const [launchCooldown, setLaunchCooldown] = useState(false);
  const [macPermissions, setMacPermissions] = useState<{ automation: boolean; accessibility: boolean; screen_recording: boolean; platform: string } | null>(null);
  const [apiKeyStatus, setApiKeyStatus] = useState<{ has_key: boolean; key_source: string | null } | null>(null);
  const [setupDismissed, setSetupDismissed] = useState(() => localStorage.getItem("vaak_setup_dismissed") === "true");
  const [autoSetupRunning, setAutoSetupRunning] = useState(false);
  const [autoSetupStatus, setAutoSetupStatus] = useState("");

  // Role CRUD state
  const AVAILABLE_PERMISSIONS = ["broadcast", "review", "assign_tasks", "status", "question", "handoff", "moderation"];
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
  const composeInputRef = useRef<HTMLInputElement>(null);

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
    perms.add("broadcast"); // All roles can broadcast by default
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
    // Only allow deleting user-created custom roles
    if (project?.config?.roles?.[slug]?.custom !== true) return;
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
      const { invoke } = await import("@tauri-apps/api/core");
      const projectContext = project ? {
        roles: Object.fromEntries(
          Object.entries(project.config.roles).map(([slug, role]) => [slug, {
            title: role.title, description: role.description,
            tags: role.tags || [], permissions: role.permissions,
          }])
        ),
      } : { roles: {} };
      const apiKey = localStorage.getItem("vaak_anthropic_key") || localStorage.getItem("vaak_api_key") || "";
      const data = await invoke<{ reply: string; role_config: any }>("design_role_turn", {
        dir: projectDir,
        messages: newMessages,
        apiKey,
      });
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

  // Close section dropdown on click outside
  useEffect(() => {
    if (!sectionDropdownOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (sectionDropdownRef.current && !sectionDropdownRef.current.contains(e.target as Node)) {
        setSectionDropdownOpen(false);
      }
    };
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") setSectionDropdownOpen(false);
    };
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [sectionDropdownOpen]);

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

  // Close work mode dropdown on click outside
  useEffect(() => {
    if (!workModeOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (workModeRef.current && !workModeRef.current.contains(e.target as Node)) {
        setWorkModeOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [workModeOpen]);

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

  // pr-watchdog-opt-in (2026-04-18): the dead-agent watchdog is now
  // disabled by default and gated on `settings.watchdog_respawn_dead_agents`.
  // The human reported auto-spawn of new PowerShells every ~1-2 minutes —
  // this effect was the cause. Interval still runs (cheap) but the invoke is
  // skipped when the setting is off, and the Rust side also early-returns
  // as a defense-in-depth. To re-enable, set the flag true in project.json.
  const watchdogEnabled = project?.config?.settings?.watchdog_respawn_dead_agents === true;
  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    if (!watchdogEnabled) return;
    let cancelled = false;
    const tickRespawn = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const respawned = await invoke<number>("check_and_respawn_dead_agents", {
          projectDir,
          staleThresholdSecs: 90,
        });
        if (!cancelled && respawned > 0) {
          console.log(`[respawn-watchdog] respawned ${respawned} dead agent(s)`);
        }
      } catch (e) {
        if (!cancelled) console.error("[respawn-watchdog] tick failed:", e);
      }
    };
    tickRespawn();
    const interval = setInterval(tickRespawn, 60_000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [projectDir, watchdogEnabled]);

  useEffect(() => {
    const discussionTurn = (project?.discussion as unknown as { turn?: SequenceTurnState })?.turn ?? null;
    setSequenceTurn(discussionTurn);
  }, [project?.discussion]);

  useEffect(() => {
    if (!window.__TAURI__) return;
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<SequenceTurnState | null>("sequence-state-changed", (evt) => {
          if (cancelled) return;
          setSequenceTurn(evt.payload ?? null);
        });
      } catch (e) {
        console.error("[sequence] event listen failed:", e);
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

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

  const handleEtherealToggle = async (slug: string) => {
    const newValue = !etherealSettings[slug];
    setEtherealSettings(prev => ({ ...prev, [slug]: newValue }));
    localStorage.setItem(`vaak_ethereal_${slug}`, String(newValue));

    if (newValue && projectDir) {
      // Starting agent — pass manual key or empty string (backend falls back to env var)
      const key = etherealApiKey || "";
      const groqKey = localStorage.getItem("vaak_groq_key") || undefined;
      const openaiKey = localStorage.getItem("vaak_openai_key") || undefined;
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("start_ethereal_agent", {
          dir: projectDir,
          slug,
          apiKey: key,
          groqKey: slug === "audience" ? groqKey : undefined,
          openaiKey: slug === "audience" ? openaiKey : undefined,
        });
      } catch (e: any) {
        const errMsg = typeof e === "string" ? e : e?.message || JSON.stringify(e);
        console.error("[CollabTab] Failed to start ethereal agent:", errMsg);
        alert(`Failed to start ${slug}: ${errMsg}`);
        setEtherealSettings(prev => ({ ...prev, [slug]: false }));
        localStorage.setItem(`vaak_ethereal_${slug}`, "false");
      }
    } else if (!newValue) {
      // Stopping agent
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("stop_ethereal_agent", { slug });
      } catch (e) {
        console.error("[CollabTab] Failed to stop ethereal agent:", e);
      }
    }
  };

  const handleApiKeyChange = (key: string) => {
    setEtherealApiKey(key);
    localStorage.setItem("vaak_anthropic_key", key);
  };

  const etherealActiveCount = ETHEREAL_ROLES.filter(r => etherealSettings[r.slug]).length;

  // Poll ethereal agent statuses when any are enabled
  useEffect(() => {
    if (etherealActiveCount === 0) return;
    let cancelled = false;
    const poll = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const statuses = await invoke<Array<{ slug: string; running: boolean; last_error: string | null; messages_sent: number; last_poll: string | null }>>("get_ethereal_statuses");
        if (!cancelled) {
          const map: Record<string, { running: boolean; last_error: string | null; messages_sent: number; last_poll: string | null }> = {};
          for (const s of statuses) {
            map[s.slug] = { running: s.running, last_error: s.last_error, messages_sent: s.messages_sent, last_poll: s.last_poll };
          }
          setEtherealStatuses(map);
        }
      } catch (e) {
        console.error("[CollabTab] Failed to poll ethereal statuses:", e);
      }
    };
    poll();
    const interval = setInterval(poll, 3000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [etherealActiveCount]);

  // Poll turn state when in consecutive mode
  useEffect(() => {
    const currentWorkMode = project?.config?.settings?.work_mode || "simultaneous";
    if (currentWorkMode !== "consecutive" || !projectDir) {
      setTurnState(null);
      return;
    }
    let cancelled = false;
    const pollTurn = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const state = await invoke<Record<string, unknown>>("get_turn_state", { dir: projectDir });
        if (!cancelled) setTurnState(state as typeof turnState);
      } catch {
        if (!cancelled) setTurnState(null);
      }
    };
    pollTurn();
    const interval = setInterval(pollTurn, 2000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [project?.config?.settings?.work_mode, projectDir]);

  // Check if ANTHROPIC_API_KEY is set in environment
  useEffect(() => {
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const hasKey = await invoke<boolean>("check_anthropic_env_key");
        setHasEnvKey(hasKey);
      } catch { /* ignore */ }
    })();
  }, []);

  const handleAudienceCountChange = (count: number) => {
    setAudienceCount(count);
    localStorage.setItem("vaak_audience_count", String(count));
  };

  const handleAudienceProviderToggle = (provider: string) => {
    setAudienceProviders(prev => {
      const next = prev.includes(provider) ? prev.filter(p => p !== provider) : [...prev, provider];
      localStorage.setItem("vaak_audience_providers", JSON.stringify(next));
      return next;
    });
  };

  // Poll discussion state (independent of communication mode)
  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    const pollDiscussion = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
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

  // Check if Node.js/npm is installed
  useEffect(() => {
    if (!window.__TAURI__) return;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const installed = await invoke<boolean>("check_npm_installed");
        setNpmInstalled(installed);
      } catch { setNpmInstalled(false); }
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

  // Check if ANTHROPIC_API_KEY is set
  useEffect(() => {
    if (!window.__TAURI__) return;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const status = await invoke<{ has_key: boolean; key_source: string | null }>("check_anthropic_key");
        setApiKeyStatus(status);
      } catch { setApiKeyStatus({ has_key: false, key_source: null }); }
    })();
  }, []);

  // MCP sidecar is auto-configured on app startup (setup_claude_code_integration in main.rs).
  // No user action needed — the setup runs before the window opens.

  // Check macOS permissions on connect (not deferred — users need to see permission
  // issues immediately, especially since the launch button depends on permissions)
  const macPermsCheckedRef = useRef(false);
  useEffect(() => {
    if (macPermsCheckedRef.current || !window.__TAURI__) return;
    macPermsCheckedRef.current = true;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const perms = await invoke<{ automation: boolean; accessibility: boolean; screen_recording: boolean; platform: string }>("check_macos_permissions");
        if (perms.platform === "macos") {
          setMacPermissions(perms);
        }
      } catch { /* non-critical */ }
    })();
  }, []);
  const checkMacPermissions = async () => {
    // Re-check on launch attempt in case permissions changed
    if (!window.__TAURI__) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const perms = await invoke<{ automation: boolean; accessibility: boolean; screen_recording: boolean; platform: string }>("check_macos_permissions");
      if (perms.platform === "macos") {
        setMacPermissions(perms);
      }
    } catch { /* non-critical */ }
  };

  // Team launcher handlers
  const handleLaunchMember = async (role: string, instance: number) => {
    // Check macOS permissions on first launch attempt
    await checkMacPermissions();
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

  const handleRequestPreviousTeamLaunch = (count: number, execute: () => Promise<number>) => {
    if (spawnConsented) {
      void execute();
      return;
    }
    setConfirmAction({
      title: "Relaunch Previous Team",
      message: `This will launch ${count} Claude Code agent${count === 1 ? "" : "s"} with full autonomous permissions (--dangerously-skip-permissions) in new terminal windows. Continue?`,
      confirmLabel: "Relaunch",
      onConfirm: async () => {
        setSpawnConsented(true);
        setConfirmAction(null);
        await execute();
      },
    });
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
    setEditingGroupSlug(null);
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

  /** Open the group modal pre-filled for editing an existing group */
  const openEditGroupModal = (group: RoleGroup) => {
    setEditingGroupSlug(group.slug);
    setNewGroupName(group.name);
    setNewGroupIcon(group.icon || "\uD83D\uDCE6");
    setNewGroupDesc(group.description || "");
    setNewGroupParent(group.parent || null);
    const roles: Record<string, number> = {};
    for (const r of group.roles) {
      roles[r.slug] = r.instances;
    }
    setNewGroupRoles(roles);
    setCreateGroupOpen(true);
  };

  /** Save a custom group (create new or update existing) to project.json via Tauri */
  const handleSaveCustomGroup = async () => {
    if (!projectDir || !newGroupName.trim() || !window.__TAURI__) return;
    const slug = editingGroupSlug || newGroupName.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
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

  /** Delete a custom group from project.json via Tauri */
  const handleDeleteGroup = async (slug: string) => {
    if (!projectDir || !window.__TAURI__) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("delete_role_group", { projectDir, slug });
    } catch (e) {
      console.error("[CollabTab] Failed to delete group:", e);
    }
  };

  const handleImportRoles = async () => {
    if (!projectDir || !window.__TAURI__) return;
    const savedProjects = loadSavedProjects();
    const otherProjects = savedProjects.filter(p => normalizePath(p.path) !== normalizePath(projectDir));
    if (otherProjects.length === 0) {
      setImportRolesStatus("No other projects to import from");
      setTimeout(() => setImportRolesStatus(null), 3000);
      return;
    }
    try {
      setImportRolesStatus("Importing...");
      const { invoke } = await import("@tauri-apps/api/core");
      // Try each saved project until one succeeds
      let imported = 0;
      for (const source of otherProjects) {
        try {
          const count = await invoke<number>("copy_project_roles", {
            sourceDir: source.path,
            destDir: projectDir,
          });
          imported += count;
        } catch {
          // Source project may not exist or have no roles — try next
        }
      }
      if (imported > 0) {
        setImportRolesStatus(`Imported ${imported} role${imported !== 1 ? "s" : ""}`);
      } else {
        setImportRolesStatus("No new roles to import");
      }
      setTimeout(() => setImportRolesStatus(null), 4000);
    } catch (e) {
      console.error("[CollabTab] Failed to import roles:", e);
      setImportRolesStatus("Import failed");
      setTimeout(() => setImportRolesStatus(null), 3000);
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
    const roleTitle = project?.config?.roles?.[role]?.title || role;
    setConfirmAction({
      title: "Remove from roster",
      message: `Remove ${roleTitle} :${instance} from the roster? This hides the card and disconnects any active agent.`,
      confirmLabel: "Remove",
      onConfirm: async () => {
        try {
          if (window.__TAURI__) {
            const { invoke } = await import("@tauri-apps/api/core");
            // If no explicit roster exists, create one from all defined roles first
            const hasExplicitRoster = project?.config?.roster && project.config.roster.length > 0;
            if (!hasExplicitRoster && project) {
              const allSlugs = Object.keys(project.config.roles);
              for (const slug of allSlugs) {
                try {
                  await invoke("roster_add_slot", { projectDir, role: slug, metadata: null });
                } catch { /* slot may already exist from migration */ }
              }
            }
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
        await invoke("set_session_mode", {
          dir: projectDir,
          sessionMode: mode,
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

  const handleSetWorkMode = async (mode: string) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_work_mode", {
          dir: projectDir,
          workMode: mode,
        });
        setWorkModeOpen(false);
        const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
        if (result) setProject(result);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to set work mode:", e);
    }
  };

  const handleCloseRound = async () => {
    setClosingRound(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("close_session_round", { dir: projectDir });
        const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
        if (state) setDiscussionState(state);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to close round:", e);
    } finally {
      setClosingRound(false);
    }
  };

  // PR H3 v2 / pr-reason-params / pr-reason-relax: backend `end_discussion`
  // accepts an optional reason and applies its own "Ended by user" default
  // when missing or under MODERATOR_REASON_MIN_CHARS. Frontend just passes
  // through whatever the caller gave (modal value, /end-discussion slash, etc.).
  const doEndDiscussion = async (reason?: string) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("end_session", { dir: projectDir, reason: reason?.trim() || null });
        setDiscussionState(null);
        setEndSessionAnnouncement("Session ended.");

        // Auto-stop discussion-bound agents (moderator + audience)
        for (const slug of DISCUSSION_BOUND_AGENTS) {
          if (etherealSettings[slug]) {
            try {
              await invoke("stop_ethereal_agent", { slug });
              setEtherealSettings(prev => ({ ...prev, [slug]: false }));
              localStorage.setItem(`vaak_ethereal_${slug}`, "false");
            } catch (e) {
              console.error(`[CollabTab] Auto-stop ${slug} failed (non-blocking):`, e);
            }
          }
        }
      }
    } catch (e) {
      // PR H3: surface moderator-action errors to the user (previously silent).
      console.error("[CollabTab] Failed to end discussion:", e);
      showModeratorError(e instanceof Error ? e.message : String(e));
    }
  };

  // Red End button on DiscussionPanel. Opens the typed-confirm modal instead
  // of calling end_discussion directly; the modal's onConfirm is what actually
  // invokes doEndDiscussion.
  const handleEndDiscussion = () => {
    setEndConfirmOpen(true);
  };

  const handleConfirmEndSession = async (reason: string) => {
    setEndConfirmOpen(false);
    await doEndDiscussion(reason);
  };

  // pr-h3-moderator-picker: designate a different moderator mid-session.
  // Wires to invoke("set_session_moderator", { dir, role, instance }) per
  // developer msg 562 / architect msg 524. Errors surface through the
  // existing pr-h3-v1 showModeratorError toast channel.
  const handleSetModerator = async (role: string, instance: number) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_session_moderator", { dir: projectDir, role, instance });
        const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
        if (state) setDiscussionState(state);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to set moderator:", e);
      showModeratorError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleTogglePause = async () => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        // pr-reason-relax: backend applies "Paused by user" / "Resumed by user"
        // defaults when reason is null. No reason-prompt UI yet on this toggle —
        // future PR can add one (architect msg 449 option 2: contextual default
        // with moderator override) without changing this call site.
        if (discussionState?.paused_at) {
          await invoke("resume_session", { dir: projectDir, reason: null });
        } else {
          await invoke("pause_session", { dir: projectDir, reason: null });
        }
        const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
        if (state) setDiscussionState(state);
      }
    } catch (e) {
      // PR H3: surface moderator-action errors to the user (previously silent).
      console.error("[CollabTab] Failed to toggle pause:", e);
      showModeratorError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleSetMaxRounds = async (rounds: number | null) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("update_session_settings", {
          dir: projectDir,
          maxRounds: rounds ?? 999,
        });
        const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
        if (state) setDiscussionState(state);
      }
    } catch (e) {
      console.error("[CollabTab] Failed to set max rounds:", e);
    }
  };

  const handleOpenStartDiscussion = async () => {
    const activeSessions = project?.sessions?.filter(s => s.status === "active") || [];
    const participantMap: Record<string, boolean> = {};
    activeSessions.forEach(s => { participantMap[`${s.role}:${s.instance}`] = true; });
    setSdParticipants(participantMap);
    setSdFormat("delphi");
    setSdTopic("");
    setSdStarting(false);
    setSdModeratorEnabled(true);
    setSdAudienceEnabled(false);
    setSdAudiencePool("");
    setSdAudienceSize(5);
    setStartDiscussionOpen(true);
    // Load audience pools from Tauri
    if (window.__TAURI__) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const pools = await invoke("list_audience_pools", {}) as Array<{ id: string; name: string; persona_count: number; providers: string[] }>;
        setTauriPools(pools);
        if (pools.length > 0 && !sdAudiencePool) setSdAudiencePool(pools[0].id);
      } catch (e) { console.error("Failed to load audience pools:", e); }
    }
  };

  const handleStartDiscussion = async () => {
    console.log("[handleStartDiscussion] Called. sdFormat:", sdFormat, "sdTopic:", sdTopic, "window.__TAURI__:", !!window.__TAURI__);
    if (!sdTopic.trim() && sdFormat !== "continuous") {
      console.warn("[handleStartDiscussion] Aborted: empty topic for non-continuous mode");
      return;
    }
    setSdStarting(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        const participants = Object.entries(sdParticipants).filter(([, v]) => v).map(([k]) => k);
        const topic = sdFormat === "continuous"
          ? "Continuous review — auto-triggered micro-rounds"
          : sdTopic.trim();
        const modSession = sdModeratorEnabled
          ? project?.sessions?.find(s => s.role === "moderator" && s.status === "active")
          : null;
        const moderator = modSession
          ? `moderator:${modSession.instance}`
          : undefined;  // let backend auto-detect from roster (or no moderator if disabled)
        console.log("[handleStartDiscussion] Invoking start_session:", { dir: projectDir, mode: sdFormat, topic, moderator, participants });
        let startDiscussionError: string | null = null;
        try {
          await invoke("start_session", {
            dir: projectDir,
            mode: sdFormat,
            topic,
            moderator,
            participants,
            rounds: null,
            pipelineMode: null,
          });
          console.log("[handleStartDiscussion] invoke succeeded");
        } catch (sdErr) {
          // start_discussion may partially succeed (writes discussion.json but fails on board announcement)
          // We continue to post the announcement via send_team_message as a fallback
          startDiscussionError = String(sdErr);
          console.warn("[handleStartDiscussion] start_session returned error (will post announcement via fallback):", sdErr);
        }

        // === OPTIMISTIC UI UPDATE ===
        // Immediately show the announcement in the board, regardless of whether
        // the file write succeeds. This fixes the UX bug where the user clicks
        // "Start Discussion" and sees nothing happen due to board lock contention.
        const annBody = sdFormat === "continuous"
          ? `Continuous Review mode activated.\n\nTopic: ${topic}\nModerator: ${moderator || "auto"}\nParticipants: ${participants.join(", ")}`
          : `A ${sdFormat} discussion has been started.\n\nTopic: ${topic}\nModerator: ${moderator || "auto"}\nParticipants: ${participants.join(", ")}\nRound: 1`;

        const optimisticId = (project?.messages?.length ? Math.max(...project.messages.map(m => m.id)) : 0) + 1;
        const optimisticMsg: BoardMessage = {
          id: optimisticId,
          from: "system",
          to: "all",
          type: "moderation",
          timestamp: new Date().toISOString(),
          subject: `${sdFormat} discussion started: ${topic}`,
          body: annBody,
          metadata: { discussion_action: "start", mode: sdFormat, round: 1 },
        };
        setProject(prev => prev ? { ...prev, messages: [...prev.messages, optimisticMsg] } : prev);

        // Write board announcement via send_team_message (the Rust-side board write in
        // start_discussion is unreliable). This goes through the working MCP sidecar path.
        try {
          await invoke("send_team_message", {
            dir: projectDir,
            to: "all",
            subject: `${sdFormat} discussion started: ${topic}`,
            body: annBody,
            msgType: "moderation",
            metadata: { discussion_action: "start", mode: sdFormat, round: 1 },
          });
        } catch (boardErr) {
          console.warn("[CollabTab] Board announcement write failed (optimistic UI already shown):", boardErr);
        }
        const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
        if (state) setDiscussionState(state);
        setStartDiscussionOpen(false);

        // Auto-start discussion-bound agents (moderator + audience) — skip if disabled
        const key = etherealApiKey || "";
        const groqKey = localStorage.getItem("vaak_groq_key") || undefined;
        const openaiKey = localStorage.getItem("vaak_openai_key") || undefined;
        for (const slug of DISCUSSION_BOUND_AGENTS) {
          if (slug === "moderator" && !sdModeratorEnabled) continue;
          if (slug === "audience" && !sdAudienceEnabled) continue;
          if (!etherealSettings[slug]) {
            try {
              await invoke("start_ethereal_agent", {
                dir: projectDir,
                slug,
                apiKey: key,
                groqKey: slug === "audience" ? groqKey : undefined,
                openaiKey: slug === "audience" ? openaiKey : undefined,
              });
              setEtherealSettings(prev => ({ ...prev, [slug]: true }));
              localStorage.setItem(`vaak_ethereal_${slug}`, "true");
            } catch (e) {
              console.error(`[CollabTab] Auto-start ${slug} failed (non-blocking):`, e);
            }
          }
        }
      } else {
        console.error("[handleStartDiscussion] window.__TAURI__ is falsy — Tauri runtime not available. Running in browser-only mode?");
        alert("Cannot start session: Tauri runtime not available. Make sure you're running the desktop app (npm run tauri dev), not just the web server.");
      }
    } catch (e) {
      console.error("[CollabTab] Failed to start discussion:", e);
      const errMsg = `Failed to start discussion: ${e}`;
      setError(errMsg);
      // Write error to board so we can see it even in screen reader mode
      try {
        const { invoke: inv } = await import("@tauri-apps/api/core");
        await inv("send_team_message", {
          dir: projectDir,
          to: "all",
          subject: "DEBUG: start_session FAILED",
          body: `Error: ${String(e)}\n\nThis means invoke("start_session") threw an exception. The Tauri command either didn't run or returned an error.`,
          msgType: "status",
          metadata: { debug: true },
        });
      } catch { /* ignore send failure */ }
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

  const handleSetWatchdogEnabled = async (enabled: boolean) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_watchdog_respawn_enabled", { dir: projectDir, enabled });
      }
    } catch (e) {
      console.error("[CollabTab] Failed to set watchdog:", e);
    }
  };

  // Auto-apply workflow when majority reached
  useEffect(() => {
    if (!project?.messages || !project?.sessions) return;
    const activeCount = project.sessions.filter(
      (s) => s.status === "active"
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

  // Smart scroll: only auto-scroll if user is at bottom or they just sent a message
  useEffect(() => {
    const messages = project?.messages;
    const currentCount = messages?.length || 0;
    const prevCount = prevMsgCountRef.current;
    const added = currentCount - prevCount;
    prevMsgCountRef.current = currentCount;

    if (added > 0) {
      // Grow visible limit so new messages don't shift the slice window
      setVisibleMsgLimit((prev) => prev + added);

      // Always scroll to bottom if the newest message is from the human (they just sent it)
      const newestMsg = messages?.[currentCount - 1];
      const isOwnMessage = newestMsg?.from?.startsWith("human:");
      if (isAtBottom || isOwnMessage) {
        // Suppress scroll-position saves until scroll completes
        // (race: project-update saves scrollTop before smooth scroll finishes,
        //  then useLayoutEffect restores mid-conversation position)
        scrollingToBottomRef.current = true;
        savedScrollRef.current = null;
        messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
        setNewMsgCount(0);
        setTimeout(() => { scrollingToBottomRef.current = false; }, 500);
      } else {
        setNewMsgCount((prev) => prev + added);
      }
    }
  }, [project?.messages?.length, isAtBottom]);

  // Restore scroll position after React commits DOM updates from project refreshes
  useLayoutEffect(() => {
    if (savedScrollRef.current !== null && messageTimelineRef.current) {
      messageTimelineRef.current.scrollTop = savedScrollRef.current;
      savedScrollRef.current = null;
    }
  }, [project]);

  // Track scroll position in message timeline
  useEffect(() => {
    const el = messageTimelineRef.current;
    if (!el) return;
    const handleScroll = () => {
      const threshold = 150;
      const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
      setIsAtBottom(atBottom);
      if (atBottom) setNewMsgCount(0);
    };
    el.addEventListener("scroll", handleScroll);
    return () => el.removeEventListener("scroll", handleScroll);
  }, [watching]);

  const scrollToBottom = () => {
    scrollingToBottomRef.current = true;
    savedScrollRef.current = null;
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    setNewMsgCount(0);
    setTimeout(() => { scrollingToBottomRef.current = false; }, 500);
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
            if (!scrollingToBottomRef.current) {
              savedScrollRef.current = messageTimelineRef.current?.scrollTop ?? null;
            }
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
              if (!scrollingToBottomRef.current) {
                savedScrollRef.current = messageTimelineRef.current?.scrollTop ?? null;
              }
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
            // Copy roles from an existing project if available
            const savedProjects = loadSavedProjects();
            const otherProjects = savedProjects.filter(p => normalizePath(p.path) !== normalizePath(dir));
            if (otherProjects.length > 0) {
              try {
                // Auto-copy from the most recently added project
                const sourceProject = otherProjects[0];
                await invoke("copy_project_roles", {
                  sourceDir: sourceProject.path,
                  destDir: dir,
                });
              } catch (copyErr) {
                // Non-fatal — project still initializes with defaults
                console.warn("[CollabTab] Could not copy roles from existing project:", copyErr);
              }
            }
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
          : parts.slice(topicStart).join(" ") || "Open session";

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
            await invoke("start_session", {
              dir: projectDir,
              mode: format,
              topic,
              moderator: moderatorOverride || defaultMod,
              participants,
            });
            setMsgBody("");
            const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
            if (state) setDiscussionState(state);
          }
        } catch (e) {
          console.error("[CollabTab] Failed to start discussion:", e);
          setError(`Failed to start discussion: ${e}`);
        } finally {
          setSending(false);
        }
        return;
      }

      if (cmd === "/end-debate" || cmd === "/end-discussion" || cmd === "/end-session") {
        // pr-r2-slash-cmds: `/end-session` is canonical; `/end-debate` and
        // `/end-discussion` kept as synonyms so muscle-memory keeps working.
        setSending(true);
        try {
          if (window.__TAURI__) {
            const { invoke } = await import("@tauri-apps/api/core");
            await invoke("end_session", { dir: projectDir, reason: `Ended via ${cmd} command` });
            // Inline reason — conveys exact invocation command (so audit
            // distinguishes /end-session vs /end-discussion vs /end-debate).
            setMsgBody("");
            const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
            if (state) setDiscussionState(state);

            // Auto-stop discussion-bound agents (moderator + audience)
            for (const slug of DISCUSSION_BOUND_AGENTS) {
              if (etherealSettings[slug]) {
                try {
                  await invoke("stop_ethereal_agent", { slug });
                  setEtherealSettings(prev => ({ ...prev, [slug]: false }));
                  localStorage.setItem(`vaak_ethereal_${slug}`, "false");
                } catch (e) {
                  console.error(`[CollabTab] Auto-stop ${slug} failed (non-blocking):`, e);
                }
              }
            }
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
            await invoke("close_session_round", { dir: projectDir });
            setMsgBody("");
            const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
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
          <label className="auto-collab-toggle" title="When enabled, agents autonomously check messages, act on directives, and communicate without manual prompting" aria-label="Auto mode: agents act autonomously on messages and directives">
            <input
              type="checkbox"
              checked={autoCollab}
              onChange={toggleAutoCollab}
              aria-describedby="auto-collab-desc"
            />
            <span className="auto-collab-label">Auto</span>
            <span className="auto-collab-desc" id="auto-collab-desc">Agents act on their own</span>
          </label>
          <label className="auto-collab-toggle human-in-loop-toggle" title="When enabled, you become a checkpoint in the review chain — agents ask for your approval at key stages" aria-label="Review mode: agents ask for your approval at key stages">
            <input
              type="checkbox"
              checked={humanInLoop}
              onChange={toggleHumanInLoop}
              aria-describedby="review-collab-desc"
            />
            <span className="auto-collab-label">Review</span>
            <span className="auto-collab-desc" id="review-collab-desc">You approve key decisions</span>
          </label>
          {/* Visibility Mode + Work Mode Selectors — hidden during active discussions to avoid contradictory labels */}
          {!discussionState?.active && (() => {
            // pr-r2-data-fields dual-read: prefer session_mode (new name),
            // fall back to discussion_mode for projects last written before
            // 0477758. Backend serde aliases on read, but the raw JSON may
            // still have either key until the next write rewrites it.
            const currentMode = project?.config?.settings?.session_mode
              ?? project?.config?.settings?.discussion_mode
              ?? "directed";
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
                  role="button"
                  aria-label={`Message visibility: ${active.label} — ${active.desc}. Click to change.`}
                >
                  {active.label}
                  <span className="discussion-mode-desc">{active.desc}</span>
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
          {/* Work Mode Selector — hidden during any active discussion or sequence */}
          {!discussionState?.active && !sequenceTurn && (() => {
            const currentWorkMode = project?.config?.settings?.work_mode || "simultaneous";
            const workModes: Record<string, { label: string; color: string; desc: string }> = {
              simultaneous: { label: "Simultaneous", color: "#17bf63", desc: "All agents work in parallel" },
              consecutive: { label: "Consecutive", color: "#e74c3c", desc: "Agents take turns in relevance order" },
            };
            const activeWM = workModes[currentWorkMode] || workModes.simultaneous;
            return (
              <div className="discussion-mode-wrapper" ref={workModeRef}>
                <span
                  className="discussion-mode-badge"
                  style={{
                    background: `${activeWM.color}22`,
                    color: activeWM.color,
                    borderColor: `${activeWM.color}55`,
                  }}
                  onClick={() => setWorkModeOpen(!workModeOpen)}
                  title="Work mode — controls whether agents work in parallel or take turns"
                  role="button"
                  aria-label={`Work mode: ${activeWM.label} — ${activeWM.desc}. Click to change.`}
                >
                  {activeWM.label}
                  <span className="discussion-mode-desc">{activeWM.desc}</span>
                </span>
                {workModeOpen && (
                  <div className="discussion-mode-dropdown">
                    {Object.entries(workModes).map(([id, m]) => (
                      <div
                        key={id}
                        className={`discussion-mode-dropdown-item${currentWorkMode === id ? " discussion-mode-active" : ""}`}
                        onClick={() => handleSetWorkMode(id)}
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
          {/* Turn Indicator — visible only in consecutive mode */}
          {turnState && !turnState.completed && turnState.relevance_order && turnState.current_index != null && (
            <span
              className="turn-indicator"
              title={`Turn order: ${turnState.relevance_order.join(" → ")}`}
              aria-label={`Current turn: ${turnState.relevance_order[turnState.current_index] || "unknown"}. Position ${turnState.current_index + 1} of ${turnState.relevance_order.length}. ${(turnState.responded?.length || 0) + (turnState.passed?.length || 0)} have responded.`}
            >
              <span className="turn-indicator-arrow">&#9654;</span>
              <span className="turn-indicator-name">{turnState.relevance_order[turnState.current_index] || "?"}</span>
              <span className="turn-indicator-pos">({turnState.current_index + 1}/{turnState.relevance_order.length})</span>
            </span>
          )}
          {turnState?.completed && (project?.config?.settings?.work_mode === "consecutive") && (
            <span className="turn-indicator turn-indicator-done" aria-label="Turn round complete — all agents have responded">
              <span className="turn-indicator-check">&#10003;</span>
              <span className="turn-indicator-name">Round complete</span>
            </span>
          )}
          {!discussionState?.active && !sequenceTurn && (
            <button
              className="start-discussion-btn"
              onClick={handleOpenStartDiscussion}
              title="Start a structured session (Delphi, Oxford, Red Team, or Continuous)"
              aria-label="Start session"
            >
              &#9998; Start Session
            </button>
          )}
          {!discussionState?.active && !sequenceTurn && (
            <button
              className="start-sequence-header-btn"
              onClick={() => setStartSequenceOpen(true)}
              title="Start a strict sequential-turn session where agents take turns one at a time"
              aria-label="Start sequence"
            >
              &#8680; Start Sequence
            </button>
          )}
          <button
            className="project-settings-btn"
            onClick={() => setSettingsOpen(!settingsOpen)}
            title="Board settings"
          >
            &#9881;
          </button>
          {/* Build identity footer — proves what's actually running, quiet unless
              SHAs diverge. Auto-detects Tauri vs browser; renders "unknown"
              when get_build_info is missing (older binary). */}
          <BuildIdentityFooter />
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
                <div className="settings-label-text">Auto-restart crashed agents</div>
                <div className="settings-label-hint">Respawn team members if their process dies mid-session. Off keeps the human in control; on recovers from PowerShell-only crashes.</div>
              </div>
              <select
                className="settings-select"
                value={project?.config?.settings?.watchdog_respawn_dead_agents === true ? "on" : "off"}
                onChange={(e) => handleSetWatchdogEnabled(e.target.value === "on")}
              >
                <option value="off">Off</option>
                <option value="on">On</option>
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

        {/* Section Dropdown */}
        <div className="section-selector">
          <div className="section-dropdown" ref={sectionDropdownRef}>
            <button
              className="section-dropdown-trigger"
              onClick={() => setSectionDropdownOpen(!sectionDropdownOpen)}
              aria-expanded={sectionDropdownOpen}
              aria-label="Switch section"
              aria-haspopup="listbox"
              disabled={sectionLoading}
            >
              <span className="section-dropdown-hash">#</span>
              <span className="section-dropdown-name">{sections.find(s => s.slug === activeSection)?.name || activeSection}</span>
              <span className="section-dropdown-chevron">{sectionDropdownOpen ? "▴" : "▾"}</span>
            </button>
            {sectionDropdownOpen && (
              <div className="section-dropdown-menu" role="listbox" aria-label="Sections">
                {sections.map(s => (
                  <div
                    key={s.slug}
                    className={`section-dropdown-item${s.slug === activeSection ? " section-dropdown-item-active" : ""}`}
                    role="option"
                    aria-selected={s.slug === activeSection}
                    onClick={() => {
                      if (s.slug !== activeSection) handleSwitchSection(s.slug);
                      setSectionDropdownOpen(false);
                    }}
                  >
                    <span className="section-dropdown-item-label">
                      <span className="section-dropdown-hash">#</span>
                      {s.name}
                    </span>
                    {s.message_count > 0 && (
                      <span className="section-dropdown-item-count">{s.message_count}</span>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
          {creatingSectionMode ? (
            <div className="section-create-inline">
              <input
                className="section-create-input"
                type="text"
                placeholder="Section name..."
                value={newSectionName}
                onChange={e => setNewSectionName(e.target.value)}
                onKeyDown={e => {
                  if (e.key === "Enter") handleCreateSection();
                  if (e.key === "Escape") { setCreatingSectionMode(false); setNewSectionName(""); }
                }}
                autoFocus
              />
              <button className="section-create-ok" onClick={handleCreateSection} disabled={!newSectionName.trim() || sectionLoading}>{sectionLoading ? "\u2026" : "\u2713"}</button>
              <button className="section-create-cancel" onClick={() => { setCreatingSectionMode(false); setNewSectionName(""); }}>&times;</button>
            </div>
          ) : (
            <button className="section-new-btn" onClick={() => setCreatingSectionMode(true)}>
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

        {/* Audience config moved to Start Discussion dialog */}

        {/* QuickLaunchBar — inline discussion starter (visible when no discussion active) */}
        <QuickLaunchBar
          discussionActive={!!discussionState?.active}
          launching={sdStarting}
          onLaunch={async (format, topic) => {
            // Quick-launch: auto-include all active participants, then start directly
            setSdStarting(true);
            try {
              if (window.__TAURI__) {
                const { invoke } = await import("@tauri-apps/api/core");
                const activeSessions = project?.sessions?.filter(s => s.status === "active") || [];
                const participants = activeSessions.map(s => `${s.role}:${s.instance}`);
                const effectiveTopic = format === "continuous"
                  ? "Continuous review — auto-triggered micro-rounds"
                  : topic;
                const modSession = project?.sessions?.find(s => s.role === "moderator" && s.status === "active");
                const moderator = modSession ? `moderator:${modSession.instance}` : undefined;
                try {
                  await invoke("start_session", { dir: projectDir, mode: format, topic: effectiveTopic, moderator, participants });
                } catch (e) {
                  console.warn("[QuickLaunch] start_session error (will post announcement):", e);
                }
                // Post announcement to board.
                const annBody = format === "continuous"
                  ? `Continuous Review mode activated.\n\nTopic: ${effectiveTopic}\nModerator: ${moderator || "auto"}\nParticipants: ${participants.join(", ")}`
                  : `A ${format} discussion has been started.\n\nTopic: ${effectiveTopic}\nModerator: ${moderator || "auto"}\nParticipants: ${participants.join(", ")}\nRound: 1`;
                try {
                  await invoke("send_team_message", {
                    dir: projectDir, to: "all",
                    subject: `${format} discussion started: ${effectiveTopic}`,
                    body: annBody, msgType: "moderation",
                    metadata: { discussion_action: "start", mode: format, round: 1 },
                  });
                } catch { /* ignore send failure — optimistic UI */ }
                const state = await invoke<DiscussionState | null>("get_session_state", { dir: projectDir });
                if (state) setDiscussionState(state);
              }
            } catch (e) {
              console.error("[QuickLaunch] Failed:", e);
            } finally {
              setSdStarting(false);
            }
          }}
          onOpenAdvanced={handleOpenStartDiscussion}
        />

        {/* Discussion Status Panel — extracted to DiscussionPanel component */}
        {discussionState?.active ? (
          <DiscussionPanel
            discussionState={discussionState}
            messages={project?.messages || []}
            sessions={project?.sessions || []}
            closingRound={closingRound}
            continuousTimeout={continuousTimeout}
            autoModActive={!!etherealSettings["moderator"]}
            onCloseRound={handleCloseRound}
            onEndDiscussion={handleEndDiscussion}
            onSetContinuousTimeout={handleSetContinuousTimeout}
            onTogglePause={handleTogglePause}
            onSetMaxRounds={handleSetMaxRounds}
            onSetModerator={handleSetModerator}
          />
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

              // Count active (non-vacant) roles in a group including children
              const deepActiveCount = (g: RoleGroup): number => {
                const activeSessions = project.sessions || [];
                let count = g.roles.filter(r =>
                  activeSessions.some(s => s.role === r.slug && s.status === "active")
                ).length;
                for (const child of childrenOf(g.slug)) count += deepActiveCount(child);
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
                const activeCount = deepActiveCount(group);
                const hasActiveRoles = activeCount > 0;

                return (
                  <div key={group.slug} className="group-tree-branch">
                    <button
                      className={`group-tree-node${isActive ? " group-tree-node-active" : ""}${!group.builtin ? " group-tree-node-custom" : ""}${!hasActiveRoles ? " group-tree-node-dim" : ""}`}
                      style={{ paddingLeft: `${8 + depth * 16}px` }}
                      onClick={() => setExpandedGroup(group.slug)}
                      title={group.description}
                      aria-expanded={hasChildren ? isExpanded : undefined}
                      aria-label={`${group.name}, ${activeCount} active of ${count} roles${isActive ? ", selected" : ""}`}
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
                      <span className="group-tree-count">{hasActiveRoles ? `${activeCount}/${count}` : count}</span>
                      {!group.builtin && (
                        <span className="group-tree-actions" onClick={(e) => e.stopPropagation()}>
                          <button
                            className="group-tree-action-btn"
                            onClick={(e) => { e.stopPropagation(); openEditGroupModal(group); }}
                            title={`Edit ${group.name} group`}
                            aria-label={`Edit ${group.name} group`}
                          >&#9998;</button>
                          <button
                            className="group-tree-action-btn group-tree-action-delete"
                            onClick={(e) => {
                              e.stopPropagation();
                              setConfirmAction({
                                title: `Delete "${group.name}" group?`,
                                message: `This will remove the group definition. Running sessions are not affected.`,
                                onConfirm: () => handleDeleteGroup(group.slug),
                                confirmLabel: "Delete Group",
                              });
                            }}
                            title={`Delete ${group.name} group`}
                            aria-label={`Delete ${group.name} group`}
                          >&times;</button>
                        </span>
                      )}
                    </button>
                    {hasChildren && isExpanded && (
                      <div className="group-tree-children">
                        {children.map(child => renderTreeNode(child, depth + 1))}
                      </div>
                    )}
                  </div>
                );
              };

              return (
                <>
                  {/* Compact trigger button — opens role management modal */}
                  <button
                    className="manage-roles-trigger"
                    onClick={() => setTeamSectionOpen(true)}
                    aria-label={`Manage Roles & Groups. ${allRoleSlugs.length} roles in ${topLevel.length} groups. Click to open.`}
                  >
                    <span className="manage-roles-trigger-icon">{"\u2699\uFE0F"}</span>
                    <span className="manage-roles-trigger-label">Manage Roles & Groups</span>
                    <span className="manage-roles-trigger-count">{allRoleSlugs.length} roles</span>
                  </button>

                  {/* Full-screen modal for role management */}
                  {teamSectionOpen && (
                    <div className="roles-modal-overlay" onClick={() => setTeamSectionOpen(false)}>
                      <div className="roles-modal" onClick={(e) => e.stopPropagation()}>
                        <div className="roles-modal-header">
                          <h2 className="roles-modal-title">Roles & Groups</h2>
                          <span className="roles-modal-subtitle">{allRoleSlugs.length} roles in {topLevel.length} groups</span>
                          <span className="roles-modal-views">
                            <button
                              className={`roster-view-btn${rosterViewMode === "grid" ? " roster-view-btn-active" : ""}`}
                              onClick={() => updateRosterViewMode("grid")}
                              title="Grid view"
                            >&#9638;</button>
                            <button
                              className={`roster-view-btn${rosterViewMode === "list" ? " roster-view-btn-active" : ""}`}
                              onClick={() => updateRosterViewMode("list")}
                              title="List view"
                            >&#9776;</button>
                            <button
                              className={`roster-view-btn${rosterViewMode === "chip" ? " roster-view-btn-active" : ""}`}
                              onClick={() => updateRosterViewMode("chip")}
                              title="Compact chip view"
                            >&#11044;</button>
                          </span>
                          <button className="roles-modal-close" onClick={() => setTeamSectionOpen(false)} aria-label="Close">&times;</button>
                        </div>
                        <div className="roles-modal-body">
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
                        <button
                          className="group-tree-node group-tree-node-import"
                          style={{ paddingLeft: "8px" }}
                          onClick={() => handleImportRoles()}
                          title="Import roles from another saved project"
                          aria-label="Import roles from another project"
                          disabled={importRolesStatus === "Importing..."}
                        >
                          <span className="group-tree-leaf" />
                          <span className="group-tree-icon" style={{ color: "#f5a623" }}>{"\u{1F4E5}"}</span>
                          <span className="group-tree-name">{importRolesStatus || "Import Roles"}</span>
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

                      {/* Active Roster inside modal — mirrors the main roster */}
                      {(() => {
                        const timeoutSecs = project.config?.settings?.heartbeat_timeout_seconds || 300;
                        const modalCards = buildRosterCards(
                          project.config.roster,
                          project.config.roles,
                          project.role_statuses,
                          project.sessions,
                          timeoutSecs
                        );
                        const companionSlugs = new Set<string>();
                        for (const [, roleDef] of Object.entries(project.config.roles)) {
                          const rd = roleDef as RoleConfig;
                          if (rd.companions) {
                            for (const comp of rd.companions) {
                              companionSlugs.add((comp as any).role);
                            }
                          }
                        }
                        const modalFiltered = modalCards.filter(card => {
                          if (!companionSlugs.has(card.slug)) return true;
                          const parentActive = Object.entries(project.config.roles).some(([parentSlug, parentDef]) => {
                            const pd = parentDef as RoleConfig;
                            if (!pd.companions?.some((c: any) => c.role === card.slug)) return false;
                            return project.sessions?.some(s => s.role === parentSlug && s.status === "active");
                          });
                          return parentActive;
                        });
                        if (modalFiltered.length === 0) return null;
                        // Sort modal cards: active first, then vacant, alphabetical within each
                        const modalStatusOrder: Record<string, number> = { working: 0, active: 0, stale: 1, vacant: 2 };
                        const modalSorted = [...modalFiltered].sort((a, b) => {
                          const sa = modalStatusOrder[a.status] ?? 2;
                          const sb = modalStatusOrder[b.status] ?? 2;
                          if (sa !== sb) return sa - sb;
                          const oa = ROLE_ORDER[a.slug] ?? 99;
                          const ob = ROLE_ORDER[b.slug] ?? 99;
                          if (oa !== ob) return oa - ob;
                          return a.title.localeCompare(b.title);
                        });
                        return (
                          <>
                            <div className="roles-modal-roster-label">Active Roster</div>
                            <div className={`project-roles-grid${rosterViewMode === "list" ? " project-roles-list" : ""}${rosterViewMode === "chip" ? " project-roles-chips" : ""}`}>
                              {modalSorted.map((card) => {
                                const cardKey = `${card.slug}:${card.instance}`;
                                const matchingRole = project.role_statuses.find((r) => r.slug === card.slug);
                                const handleCardClick = () => {
                                  matchingRole && setSelectedRole(matchingRole);
                                };
                                if (rosterViewMode === "chip") {
                                  return (
                                    <button
                                      key={cardKey}
                                      className={`role-chip${card.status === "working" ? " role-chip-working" : ""}${card.status === "ready" ? " role-chip-ready" : ""}${card.status === "vacant" ? " role-chip-vacant" : ""}`}
                                      style={{ borderColor: card.roleColor + "40", color: card.roleColor }}
                                      onClick={handleCardClick}
                                      title={`${card.title} — ${getStatusLabel(card.status)}`}
                                    >
                                      <span className={getStatusDotClass(card.status)} />
                                      <span className="role-chip-name">{card.title}</span>
                                      <span className={`role-chip-status role-card-status-${card.status}`}>{getStatusLabel(card.status)}</span>
                                    </button>
                                  );
                                }
                                return (
                                  <div
                                    key={cardKey}
                                    className={`project-role-card role-card-status-${card.status}`}
                                    style={{ borderColor: card.roleColor + "30" }}
                                    onClick={handleCardClick}
                                  >
                                    <div className="role-card-header">
                                      <span className={getStatusDotClass(card.status)} />
                                      <span className="role-card-title" style={{ color: card.roleColor }}>{card.title}</span>
                                      {card.instance > 0 && <span className="role-card-instance">#{card.instance}</span>}
                                      <button
                                        className="role-card-remove-x"
                                        onClick={(e) => { e.stopPropagation(); handleRemoveRosterSlot(card.slug, card.instance >= 0 ? card.instance : 0); }}
                                        title="Remove from roster"
                                        aria-label={`Remove ${card.title} from roster`}
                                      >&times;</button>
                                    </div>
                                    <div className="role-card-status">{getStatusLabel(card.status)}</div>
                                    {card.status === "vacant" && (
                                      <button
                                        className="role-card-launch-btn"
                                        onClick={(e) => { e.stopPropagation(); handleLaunchMember(card.slug, card.instance); }}
                                        disabled={launchCooldown || claudeInstalled === false}
                                        title={claudeInstalled === false ? "Claude CLI not found — install with: npm i -g @anthropic-ai/claude-code" : `Launch Claude agent as ${card.title}`}
                                      >{claudeInstalled === false ? "CLI Missing" : "Launch"}</button>
                                    )}
                                  </div>
                                );
                              })}
                            </div>
                          </>
                        );
                      })()}

                    </div>
                  </div>
                </div>
              )}
                </>
              );
            })()}
          </div>
        )}

        {/* Setup Checklist — guides new users through prerequisites */}
        {project && !setupDismissed && (npmInstalled === false || claudeInstalled === false || apiKeyStatus?.has_key === false) && (
          <div className="setup-checklist-banner">
            <div className="setup-checklist-header">
              <span className="setup-checklist-title">Setup Required</span>
              <span className="setup-checklist-subtitle">
                {autoSetupRunning ? autoSetupStatus : "Complete these steps to launch AI agents"}
              </span>
              {!autoSetupRunning && (
                <button
                  className="setup-action-btn setup-action-primary"
                  style={{ marginLeft: "auto", marginRight: 8, flexShrink: 0, whiteSpace: "nowrap" }}
                  onClick={async () => {
                    if (!window.__TAURI__) return;
                    setAutoSetupRunning(true);
                    try {
                      const { invoke } = await import("@tauri-apps/api/core");

                      // Step 1: Check Node.js
                      setAutoSetupStatus("Checking Node.js...");
                      let hasNpm = await invoke<boolean>("check_npm_installed");
                      setNpmInstalled(hasNpm);

                      if (!hasNpm) {
                        setAutoSetupStatus("Node.js required — opening download page...");
                        try { await invoke("open_url_in_browser", { url: "https://nodejs.org" }); } catch {}
                        // Poll for npm every 5s for up to 5 minutes
                        setAutoSetupStatus("Install Node.js, then come back — auto-detecting...");
                        for (let i = 0; i < 60; i++) {
                          await new Promise(r => setTimeout(r, 5000));
                          hasNpm = await invoke<boolean>("check_npm_installed");
                          if (hasNpm) { setNpmInstalled(true); break; }
                        }
                        if (!hasNpm) {
                          setAutoSetupStatus("Node.js not detected after 5 min. Click to retry.");
                          setAutoSetupRunning(false);
                          return;
                        }
                      }

                      // Step 2: Install Claude CLI
                      setAutoSetupStatus("Installing Claude Code CLI...");
                      let hasClaude = await invoke<boolean>("check_claude_installed");
                      if (!hasClaude) {
                        try {
                          await invoke<string>("install_claude_cli");
                          hasClaude = await invoke<boolean>("check_claude_installed");
                        } catch (e: any) {
                          setAutoSetupStatus(`CLI install failed: ${e?.message || e}`);
                          setAutoSetupRunning(false);
                          return;
                        }
                      }
                      setClaudeInstalled(hasClaude);
                      if (!hasClaude) {
                        setAutoSetupStatus("Claude CLI install failed. Try manually: npm install -g @anthropic-ai/claude-code");
                        setAutoSetupRunning(false);
                        return;
                      }

                      // Step 3: Check API key
                      setAutoSetupStatus("Checking API key...");
                      const keyStatus = await invoke<{ has_key: boolean; key_source: string | null }>("check_anthropic_key");
                      setApiKeyStatus(keyStatus);
                      if (!keyStatus.has_key) {
                        setAutoSetupStatus("Opening terminal for login — follow the browser prompt...");
                        try { await invoke("open_terminal_in_dir", { dir: projectDir || "" }); } catch {}
                        // Poll for key every 5s for up to 3 minutes
                        for (let i = 0; i < 36; i++) {
                          await new Promise(r => setTimeout(r, 5000));
                          const k = await invoke<{ has_key: boolean; key_source: string | null }>("check_anthropic_key");
                          if (k.has_key) { setApiKeyStatus(k); break; }
                        }
                      }

                      setAutoSetupStatus("Setup complete!");
                      setTimeout(() => setAutoSetupRunning(false), 2000);
                    } catch (e) {
                      setAutoSetupStatus(`Setup error: ${e}`);
                      setAutoSetupRunning(false);
                    }
                  }}
                >Set Up Automatically</button>
              )}
              <button
                className="setup-checklist-dismiss"
                onClick={() => { setSetupDismissed(true); localStorage.setItem("vaak_setup_dismissed", "true"); }}
                title="Dismiss (you can still launch manually)"
              >&times;</button>
            </div>
            <div className="setup-checklist-items">
              {/* Step 1: Node.js / npm */}
              <div className={`setup-checklist-item${npmInstalled === true ? " setup-done" : npmInstalled === false ? " setup-needed" : " setup-checking"}`}>
                <span className="setup-check-icon">{npmInstalled === true ? "\u2713" : npmInstalled === false ? "1" : "\u22EF"}</span>
                <div className="setup-check-content">
                  <div className="setup-check-label">Install Node.js</div>
                  {npmInstalled === true ? (
                    <div className="setup-check-status">Installed</div>
                  ) : npmInstalled === false ? (
                    <div className="setup-check-actions">
                      <button
                        className="setup-action-btn setup-action-primary"
                        disabled={installingNode}
                        onClick={async () => {
                          setInstallingNode(true);
                          try {
                            const { invoke } = await import("@tauri-apps/api/core");
                            const result = await invoke<string>("install_nodejs");
                            if (result === "installed") {
                              const installed = await invoke<boolean>("check_npm_installed");
                              setNpmInstalled(installed);
                              if (installed) {
                                // Auto-advance: install Claude Code too
                                setInstallingCli(true);
                                try {
                                  await invoke<string>("install_claude_cli");
                                  const claudeOk = await invoke<boolean>("check_claude_installed");
                                  setClaudeInstalled(claudeOk);
                                } catch (e: any) {
                                  alert(`Claude Code install failed: ${e?.message || e}`);
                                } finally {
                                  setInstallingCli(false);
                                }
                              }
                            } else {
                              alert("Node.js download page opened in your browser. Install it, then click Re-check.");
                            }
                          } catch (e: any) {
                            alert(`Install failed: ${e?.message || e}`);
                          } finally {
                            setInstallingNode(false);
                          }
                        }}
                      >{installingNode ? "Installing..." : "Install Node.js"}</button>
                      <button
                        className="setup-action-btn"
                        onClick={async () => {
                          try {
                            const { invoke } = await import("@tauri-apps/api/core");
                            const installed = await invoke<boolean>("check_npm_installed");
                            setNpmInstalled(installed);
                            if (!installed) {
                              alert("Node.js/npm not detected yet. Install Node.js, then click Re-check.");
                            }
                          } catch { /* ignore */ }
                        }}
                      >Re-check</button>
                      <span className="setup-check-hint">Auto-installs via Homebrew on Mac, or opens download page</span>
                    </div>
                  ) : (
                    <div className="setup-check-status">Checking...</div>
                  )}
                </div>
              </div>

              {/* Step 2: Claude Code CLI */}
              <div className={`setup-checklist-item${claudeInstalled === true ? " setup-done" : claudeInstalled === false ? " setup-needed" : " setup-checking"}`}>
                <span className="setup-check-icon">{claudeInstalled === true ? "\u2713" : claudeInstalled === false ? "2" : "\u22EF"}</span>
                <div className="setup-check-content">
                  <div className="setup-check-label">Install Claude Code CLI</div>
                  {claudeInstalled === true ? (
                    <div className="setup-check-status">Installed</div>
                  ) : claudeInstalled === false ? (
                    <div className="setup-check-actions">
                      <button
                        className="setup-action-btn setup-action-primary"
                        disabled={installingCli || npmInstalled === false}
                        onClick={async () => {
                          setInstallingCli(true);
                          try {
                            const { invoke } = await import("@tauri-apps/api/core");
                            const npmOk = await invoke<boolean>("check_npm_installed");
                            if (!npmOk) {
                              setInstallingCli(false);
                              setNpmInstalled(false);
                              alert("Node.js/npm is not installed. Complete Step 1 first.");
                              return;
                            }
                            await invoke<string>("install_claude_cli");
                            const installed = await invoke<boolean>("check_claude_installed");
                            if (installed) {
                              setClaudeInstalled(true);
                            } else {
                              setInstallingCli(false);
                              alert("Installation completed but Claude CLI was not found. Try restarting the app.");
                            }
                          } catch (e: any) {
                            setInstallingCli(false);
                            alert(`Install failed: ${e?.message || e}. Try manually: npm install -g @anthropic-ai/claude-code`);
                          }
                        }}
                      >{installingCli ? "Installing..." : "Express Install"}</button>
                      <button
                        className="setup-action-btn"
                        onClick={async () => {
                          try {
                            const { invoke } = await import("@tauri-apps/api/core");
                            await invoke("open_terminal_in_dir", { dir: projectDir || "" });
                          } catch { /* ignore */ }
                        }}
                      >Open Terminal</button>
                      {npmInstalled === false && <span className="setup-check-hint" style={{ color: "#e8912d" }}>Complete Step 1 first</span>}
                    </div>
                  ) : (
                    <div className="setup-check-status">Checking...</div>
                  )}
                </div>
              </div>

              {/* Step 3: API Key */}
              <div className={`setup-checklist-item${apiKeyStatus?.has_key ? " setup-done" : apiKeyStatus?.has_key === false ? " setup-needed" : " setup-checking"}`}>
                <span className="setup-check-icon">{apiKeyStatus?.has_key ? "\u2713" : apiKeyStatus?.has_key === false ? "3" : "\u22EF"}</span>
                <div className="setup-check-content">
                  <div className="setup-check-label">Set Anthropic API Key</div>
                  {apiKeyStatus?.has_key ? (
                    <div className="setup-check-status">Found ({apiKeyStatus.key_source})</div>
                  ) : apiKeyStatus?.has_key === false ? (
                    <div className="setup-check-actions">
                      <div className="setup-check-hint">
                        <strong>Option A (easiest):</strong> Run <code>claude</code> in a terminal — it will prompt you to log in via browser.
                        <br/>
                        <strong>Option B:</strong> Set <code>ANTHROPIC_API_KEY</code> environment variable:
                        <br/>
                        <span style={{ opacity: 0.7, fontSize: "0.9em" }}>
                          Windows: <code>setx ANTHROPIC_API_KEY "sk-ant-..."</code> &nbsp;|&nbsp;
                          Mac/Linux: <code>export ANTHROPIC_API_KEY="sk-ant-..."</code> in shell profile
                        </span>
                      </div>
                      <button
                        className="setup-action-btn setup-action-primary"
                        disabled={claudeInstalled === false}
                        onClick={async () => {
                          try {
                            const { invoke } = await import("@tauri-apps/api/core");
                            await invoke("open_terminal_in_dir", { dir: projectDir || "" });
                          } catch { /* ignore */ }
                        }}
                      >{claudeInstalled === false ? "Install CLI First" : "Open Terminal to Log In"}</button>
                      <button
                        className="setup-action-btn"
                        onClick={async () => {
                          try {
                            const { invoke } = await import("@tauri-apps/api/core");
                            const status = await invoke<{ has_key: boolean; key_source: string | null }>("check_anthropic_key");
                            setApiKeyStatus(status);
                            if (!status.has_key) {
                              alert("API key not detected in environment variables. If you logged in via OAuth, you may still be able to launch agents — try launching one to test.");
                            }
                          } catch { /* ignore */ }
                        }}
                      >Re-check</button>
                    </div>
                  ) : (
                    <div className="setup-check-status">Checking...</div>
                  )}
                </div>
              </div>

              {/* MCP configuration is automatic on app startup — no user action needed */}
            </div>
            {npmInstalled === true && claudeInstalled === true && apiKeyStatus?.has_key && (
              <div className="setup-checklist-complete">All set! You can now launch agents from the roster below.</div>
            )}
          </div>
        )}

        {/* Team Roster — shows all roster slots with status */}
        {project && (() => {
          const timeoutSecs = project.config?.settings?.heartbeat_timeout_seconds || 300;
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
          // Sort: active/working first, then stale, then vacant. Alphabetical within each group.
          const statusOrder: Record<string, number> = { working: 0, active: 0, stale: 1, vacant: 2 };
          const sortedCards = [...filteredCards].sort((a, b) => {
            const sa = statusOrder[a.status] ?? 2;
            const sb = statusOrder[b.status] ?? 2;
            if (sa !== sb) return sa - sb;
            const oa = ROLE_ORDER[a.slug] ?? 99;
            const ob = ROLE_ORDER[b.slug] ?? 99;
            if (oa !== ob) return oa - ob;
            return a.title.localeCompare(b.title);
          });
          const vacantCount = sortedCards.filter(c => c.status === "vacant").length;
          return (
            <>
              {sortedCards.length > 0 && (
                <div className={`project-roles-grid${rosterViewMode === "list" ? " project-roles-list" : ""}${rosterViewMode === "chip" ? " project-roles-chips" : ""}`}>
                  {sortedCards.map((card) => {
                    const cardKey = `${card.slug}:${card.instance}`;
                    const matchingRole = project.role_statuses.find((r) => r.slug === card.slug);
                    const handleCardClick = () => {
                      matchingRole && setSelectedRole(matchingRole);
                    };
                    const handleCardKeyDown = (e: React.KeyboardEvent) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        handleCardClick();
                      }
                    };

                    // Pipeline turn state for this card
                    const pipelineOrder = discussionState?.active && discussionState?.mode === "pipeline" ? discussionState.pipeline_order : undefined;
                    const pipelineStage = discussionState?.pipeline_stage ?? 0;
                    const pipelineIdx = pipelineOrder ? pipelineOrder.indexOf(cardKey) : -1;
                    const isPipelineActive = pipelineOrder && pipelineOrder.length > 0;
                    const pipelineTurn: "on-turn" | "completed" | "waiting" | null =
                      isPipelineActive && pipelineIdx >= 0
                        ? pipelineIdx < pipelineStage ? "completed"
                        : pipelineIdx === pipelineStage ? "on-turn"
                        : "waiting"
                        : null;
                    const pipelineQueuePos = pipelineTurn === "waiting" ? pipelineIdx - pipelineStage : 0;

                    // Compact chip view
                    if (rosterViewMode === "chip") {
                      return (
                        <button
                          key={cardKey}
                          className={`role-chip${card.status === "working" ? " role-chip-working" : ""}${card.status === "ready" ? " role-chip-ready" : ""}${card.status === "vacant" ? " role-chip-vacant" : ""}${pipelineTurn === "on-turn" ? " pipeline-on-turn" : ""}${pipelineTurn === "completed" ? " pipeline-completed" : ""}${pipelineTurn === "waiting" ? " pipeline-waiting" : ""}`}
                          style={{ borderColor: card.roleColor + "40", color: card.roleColor }}
                          onClick={handleCardClick}
                          title={`${card.title} — ${getStatusLabel(card.status)}${card.instance > 0 ? ` (instance ${card.instance})` : ""}. Click for details.`}
                          aria-label={`${card.title}, status: ${getStatusLabel(card.status)}${card.instance > 0 ? `, instance ${card.instance}` : ""}. Press Enter for details and actions.`}
                        >
                          <span className={getStatusDotClass(card.status)} />
                          <span className="role-chip-name">{card.title}</span>
                          {pipelineTurn === "on-turn" && <span className="pipeline-badge pipeline-badge-active" aria-label="Currently on turn">ON TURN</span>}
                          {pipelineTurn === "completed" && <span className="pipeline-badge pipeline-badge-done" aria-label="Turn completed">{"\u2713"}</span>}
                          {pipelineTurn === "waiting" && <span className="pipeline-badge pipeline-badge-queue" aria-label={`Queue position ${pipelineQueuePos}`}>#{pipelineQueuePos}</span>}
                          {!pipelineTurn && <span className={`role-chip-status role-card-status-${card.status}`}>{getStatusLabel(card.status)}</span>}
                        </button>
                      );
                    }

                    // PR H2: privileged-role visual distinction.
                    // Why: moderator and manager hold out-of-turn speech and
                    // pipeline-override capabilities (per msg 200); users need
                    // to see at a glance which seats carry that authority.
                    const isPrivilegedRole = card.slug === "moderator" || card.slug === "manager";
                    return (
                      <div
                        key={cardKey}
                        className={`role-card role-card-clickable ${card.status === "working" ? "role-card-working" : ""} ${card.status === "vacant" ? "role-card-vacant" : ""}${pipelineTurn === "on-turn" ? " pipeline-on-turn" : ""}${pipelineTurn === "completed" ? " pipeline-completed" : ""}${pipelineTurn === "waiting" ? " pipeline-waiting" : ""}${isPrivilegedRole ? " role-card-privileged" : ""}`}
                        style={{ borderLeftColor: card.roleColor }}
                        role="button"
                        tabIndex={0}
                        aria-label={`${card.title}${isPrivilegedRole ? " (privileged role — out-of-turn speech allowed)" : ""}, status: ${getStatusLabel(card.status)}${pipelineTurn === "on-turn" ? ", currently on turn" : pipelineTurn === "completed" ? ", turn completed" : pipelineTurn === "waiting" ? `, queue position ${pipelineQueuePos}` : ""}. Click to view details.`}
                        onClick={handleCardClick}
                        onKeyDown={handleCardKeyDown}
                      >
                        <div className="role-card-header">
                          <span className={getStatusDotClass(card.status)} />
                          {isPrivilegedRole && (
                            <span className="role-card-crown" title={`${card.title} — privileged role, can speak out of turn`} aria-hidden="true">
                              {"\u265B"}
                            </span>
                          )}
                          <span className="role-card-title" style={{ color: card.roleColor }}>
                            {card.title}
                          </span>
                          <button
                            className="role-card-remove-x"
                            onClick={(e) => { e.stopPropagation(); handleRemoveRosterSlot(card.slug, card.instance >= 0 ? card.instance : 0); }}
                            title="Remove from roster"
                            aria-label={`Remove ${card.title} from roster`}
                          >&times;</button>
                        </div>
                        <div className="role-card-meta">
                          <span className="role-card-slug">{card.slug}</span>
                          {pipelineTurn === "on-turn" ? (
                            <span className="pipeline-badge pipeline-badge-active">ON TURN</span>
                          ) : pipelineTurn === "completed" ? (
                            <span className="pipeline-badge pipeline-badge-done">{"\u2713"} Done</span>
                          ) : pipelineTurn === "waiting" ? (
                            <span className="pipeline-badge pipeline-badge-queue">#{pipelineQueuePos} in queue</span>
                          ) : (
                            <span className={`role-card-status role-card-status-${card.status}`}>
                              {getStatusLabel(card.status)}
                            </span>
                          )}
                        </div>
                        {/* Companion badge — show if this role is a companion of another role */}
                        {(() => {
                          const parentRole = Object.entries(project.config.roles).find(([, r]) =>
                            (r as any).companions?.some((c: any) => c.role === card.slug)
                          );
                          if (!parentRole) return null;
                          return <span className="role-companion-badge">paired with {(parentRole[1] as any).title || parentRole[0]}</span>;
                        })()}
                        {/* Action row — primary action + overflow menu */}
                        <div className="role-card-actions" role="group" aria-label={`Actions for ${card.title}`} onClick={(e) => e.stopPropagation()}>
                          {/* Primary action: View for active, nothing extra for vacant */}
                          {card.status !== "vacant" && (
                            <button
                              className="role-action-btn role-action-view"
                              onClick={() => handleViewAgent(card.slug, card.instance >= 0 ? card.instance : 0)}
                              title={`View ${card.title} terminal`}
                            >&#128065; View</button>
                          )}
                          {/* Overflow menu trigger */}
                          {(() => {
                            const cardKey = `${card.slug}:${card.instance >= 0 ? card.instance : 0}`;
                            const isOpen = openCardMenu === cardKey;
                            return (
                              <div className="role-action-overflow-wrap">
                                <button
                                  className={`role-action-btn role-action-overflow-btn${isOpen ? " active" : ""}`}
                                  onClick={(e) => {
                                    if (isOpen) { setOpenCardMenu(null); setMenuPos(null); }
                                    else {
                                      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                                      const menuW = 200; // approx menu width
                                      // If menu would overflow left edge, anchor to left of button; otherwise right-align
                                      const wouldClipLeft = r.right - menuW < 8;
                                      setMenuPos({
                                        top: r.bottom + 4,
                                        right: wouldClipLeft ? -1 : window.innerWidth - r.right,
                                        left: wouldClipLeft ? Math.max(8, r.left) : -1,
                                      });
                                      setOpenCardMenu(cardKey);
                                    }
                                  }}
                                  title="More actions"
                                  aria-expanded={isOpen}
                                  aria-haspopup="menu"
                                >&#8943;</button>
                                {isOpen && menuPos && createPortal(
                                  <>
                                  <div className="role-action-overflow-backdrop" onClick={() => { setOpenCardMenu(null); setMenuPos(null); }} />
                                  <div className="role-action-overflow-menu" role="menu" style={{ position: 'fixed', top: menuPos.top, ...(menuPos.left >= 0 ? { left: menuPos.left, right: 'auto' } : { right: menuPos.right, left: 'auto' }) }} onClick={() => { setOpenCardMenu(null); setMenuPos(null); }}>
                                    {card.instance === 0 && (
                                      <button className="role-overflow-item role-overflow-edit" role="menuitem" onClick={() => openEditRoleForm(card.slug)}>&#9998; Edit Role</button>
                                    )}
                                    {card.status !== "vacant" && (() => {
                                      const bk = `${card.slug}:${card.instance >= 0 ? card.instance : 0}`;
                                      const isBuzzed = buzzedKey === bk;
                                      return (
                                        <button className="role-overflow-item role-overflow-buzz" role="menuitem" onClick={() => handleBuzz(card.slug, card.instance >= 0 ? card.instance : 0)} disabled={isBuzzed}>
                                          {isBuzzed ? "\u2713 Buzzed" : "\uD83D\uDD14 Buzz"}
                                        </button>
                                      );
                                    })()}
                                    {card.status !== "vacant" && (
                                      <button className="role-overflow-item role-overflow-interrupt" role="menuitem" onClick={() => setInterruptTarget({ slug: card.slug, instance: card.instance >= 0 ? card.instance : 0, title: card.title })}>&#9889; Interrupt</button>
                                    )}
                                    {card.status !== "vacant" && (
                                      <button className="role-overflow-item role-overflow-disconnect" role="menuitem" onClick={() => {
                                        const inst = card.instance >= 0 ? card.instance : 0;
                                        setConfirmAction({
                                          title: "Disconnect agent",
                                          message: `Disconnect ${card.title}:${inst}? This ends their session but keeps the slot on the roster.`,
                                          confirmLabel: "Disconnect",
                                          onConfirm: async () => {
                                            try {
                                              if (window.__TAURI__) {
                                                const { invoke } = await import("@tauri-apps/api/core");
                                                await invoke("kill_team_member", { role: card.slug, instance: inst });
                                              }
                                            } catch (e) {
                                              console.error("[CollabTab] Failed to disconnect:", e);
                                            }
                                            setConfirmAction(null);
                                          },
                                        });
                                      }}>&#10005; Disconnect</button>
                                    )}
                                    <span className="role-overflow-separator" />
                                    <button className="role-overflow-item role-overflow-remove" role="menuitem" onClick={() => handleRemoveRosterSlot(card.slug, card.instance >= 0 ? card.instance : 0)}>&#8722; Remove from Roster</button>
                                    {card.instance === 0 && card.status === "vacant" && project?.config?.roles?.[card.slug]?.custom === true && (
                                      <button className="role-overflow-item role-overflow-delete" role="menuitem" onClick={() => handleDeleteRole(card.slug)}>&#128465; Delete Role</button>
                                    )}
                                  </div>
                                  </>,
                                  document.body
                                )}
                              </div>
                            );
                          })()}
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
                        {card.status === "vacant" && (
                          <button
                            className="role-card-launch-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleLaunchMember(card.slug, card.instance);
                            }}
                            disabled={launchCooldown || claudeInstalled === false}
                            title={claudeInstalled === false ? "Claude CLI not found — install with: npm i -g @anthropic-ai/claude-code" : `Launch Claude agent as ${card.title}`}
                            aria-label={claudeInstalled === false ? "Claude CLI not installed" : `Launch Claude agent as ${card.title}`}
                          >{claudeInstalled === false ? "CLI Missing" : "Launch"}</button>
                        )}
                      </div>
                    );
                  })}

                  {/* Audience card now rendered through normal roster loop above — click opens audience panel */}
                </div>
              )}

              {/* Launch All Vacant button */}
              {vacantCount > 1 && (
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
                  disabled={launching || launchCooldown || claudeInstalled === false}
                  title={claudeInstalled === false ? "Claude CLI not found — install with: npm i -g @anthropic-ai/claude-code" : undefined}
                >
                  {launching && <span className="launch-team-spinner" />}
                  {claudeInstalled === false ? "Claude CLI Not Found" : launching ? "Launching..." : `Launch All Vacant (${vacantCount})`}
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
          </div>
        )}

        {/* Old Claude CLI banner replaced by Setup Checklist above roster */}

        <SequenceSessionCard
          turn={sequenceTurn}
          projectDir={projectDir}
          availableRoleInstances={
            ((project?.sessions || [])
              .filter((s) => s.status === "active")
              .map<ModeratorSequencePanelRosterEntry>((s) => ({
                id: `${s.role}:${s.instance}`,
                title: project?.config?.roles?.[s.role]?.title ?? s.role,
              })))
          }
        />

        <PreviousTeamBanner
          projectDir={projectDir}
          claudeInstalled={claudeInstalled}
          onRequestLaunch={handleRequestPreviousTeamLaunch}
        />

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

        {/* macOS permission warning banner */}
        {macPermissions && (!macPermissions.automation || !macPermissions.accessibility) && (
          <div className="project-hint-banner" style={{ borderColor: "#e8935a", background: "rgba(232,147,90,0.08)" }}>
            <div className="project-hint-title" style={{ color: "#e8935a" }}>macOS Permissions Required</div>
            <div className="project-hint-body">
              {!macPermissions.automation && !macPermissions.accessibility
                ? "Vaak needs Automation and Accessibility permissions to launch and buzz team agents."
                : !macPermissions.automation
                  ? "Vaak needs Automation permission to launch and manage Terminal windows for agents."
                  : "Vaak needs Accessibility permission to buzz agents and read screen content."
              }
              {" "}Go to <strong>System Settings → Privacy & Security</strong>:
              {!macPermissions.automation && <><br/>• <strong>Automation</strong> → enable Terminal for Vaak</>}
              {!macPermissions.accessibility && <><br/>• <strong>Accessibility</strong> → enable Vaak</>}
              <br/>Then restart the app.
            </div>
          </div>
        )}

        {/* PR H: Inbox tabs — separates human-directed messages from team chatter.
            Why: the board was one big stream; users asked for a clean view of "what's
            addressed to me" without scrolling past 30+ role-to-role turns per pipeline.
            PR H2: added unread-count badges + Ctrl+1/2/3 keyboard shortcuts. */}
        {(() => {
          // PR H2: compute unread count per tab. Each tab counts messages newer
          // than its last-seen timestamp, using the same filter predicates the
          // timeline uses. Active tab is never marked unread — seeing it counts.
          const msgs = project?.messages ?? [];
          const countUnread = (tab: InboxTab): number => {
            if (tab === inboxTab) return 0;
            const since = tabLastSeen[tab] || 0;
            return msgs.reduce((n, m) => {
              const ts = new Date(m.timestamp).getTime();
              if (isNaN(ts) || ts <= since) return n;
              if (tab === "all") return n + 1;
              if (tab === "human") {
                if (m.to === "human"
                  || m.from.startsWith("human:")
                  || m.from.startsWith("manager:")
                  || m.from.startsWith("moderator:")) return n + 1;
                return n;
              }
              if (tab === "pipeline") {
                const meta = m.metadata as Record<string, unknown> | undefined;
                if (m.type === "moderation"
                  || !!meta?.pipeline_notification
                  || !!meta?.discussion_turn
                  || !!meta?.pipeline_position) return n + 1;
                return n;
              }
              return n;
            }, 0);
          };
          const humanUnread = countUnread("human");
          const pipelineUnread = countUnread("pipeline");
          const allUnread = countUnread("all");
          const renderBadge = (count: number) => count > 0 ? (
            <span className="inbox-tab-badge" aria-label={`${count} unread`}>
              {count > 99 ? "99+" : count}
            </span>
          ) : null;
          return (
            <div className="inbox-tabs" role="tablist" aria-label="Message inbox views">
              <button
                role="tab"
                id="inbox-tab-human"
                aria-selected={inboxTab === "human"}
                aria-controls="inbox-panel"
                className={`inbox-tab${inboxTab === "human" ? " inbox-tab-active" : ""}`}
                onClick={() => changeInboxTab("human")}
                title="You &amp; Moderator (Ctrl+1)"
              >
                You &amp; Moderator
                {renderBadge(humanUnread)}
              </button>
              <button
                role="tab"
                id="inbox-tab-pipeline"
                aria-selected={inboxTab === "pipeline"}
                aria-controls="inbox-panel"
                className={`inbox-tab${inboxTab === "pipeline" ? " inbox-tab-active" : ""}`}
                onClick={() => changeInboxTab("pipeline")}
                title="Active Pipeline (Ctrl+2)"
              >
                Active Pipeline
                {renderBadge(pipelineUnread)}
              </button>
              <button
                role="tab"
                id="inbox-tab-all"
                aria-selected={inboxTab === "all"}
                aria-controls="inbox-panel"
                className={`inbox-tab${inboxTab === "all" ? " inbox-tab-active" : ""}`}
                onClick={() => changeInboxTab("all")}
                title="All Activity (Ctrl+3)"
              >
                All Activity
                {renderBadge(allUnread)}
              </button>
            </div>
          );
        })()}

        {/* Message Timeline */}
        <div className="message-timeline" id="inbox-panel" role="tabpanel" aria-labelledby={`inbox-tab-${inboxTab}`} ref={messageTimelineRef}>
          {hasNoMessages ? (
            <div className="message-timeline-empty" role="status">
              {hasNoSessions
                ? "No team members connected yet. Launch agents from the roster above to get started."
                : "Team connected — no messages yet. Your team's communication will appear here."}
            </div>
          ) : (
            (() => {
              const activeCount = project!.sessions.filter(
                (s) => s.status === "active"
              ).length;
              const voteTallies = getActiveVotes(project!.messages, activeCount);
              const voteProposalIds = new Set(voteTallies.map((t) => t.proposalId));
              // IDs of response votes — hide from timeline
              const voteResponseIds = new Set(
                project!.messages
                  .filter((m) => m.type === "vote" && m.metadata?.in_reply_to)
                  .map((m) => m.id)
              );

              // PR H: filter predicates key on from/to only, not body text —
              // `@human` as a mention in a broadcast body is formatting, not a
              // capability flag, so body-text matches would leak team chatter into
              // the human tab.
              const rawMessages = project!.messages;
              const allMessages = inboxTab === "all"
                ? rawMessages
                : rawMessages.filter((m: BoardMessage) => {
                    if (inboxTab === "human") {
                      // Human tab: directed to/from the user, plus both leadership roles'
                      // broadcasts. Moderator and manager are distinct privileged roles
                      // (per tech-leader msg 200 retraction): moderator runs the format,
                      // manager coordinates the project and owns @human direct-message.
                      return m.to === "human"
                        || m.from.startsWith("human:")
                        || m.from.startsWith("manager:")
                        || m.from.startsWith("moderator:");
                    }
                    if (inboxTab === "pipeline") {
                      // Pipeline tab: active session turns and system stage-advance events,
                      // not free-form directives or role→role status messages unrelated to
                      // the current pipeline.
                      const meta = m.metadata as Record<string, unknown> | undefined;
                      return m.type === "moderation"
                        || !!meta?.pipeline_notification
                        || !!meta?.discussion_turn
                        || !!meta?.pipeline_position;
                    }
                    return true;
                  });
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
                        {isStart ? "Session Started" : isEnd ? "Session Ended" : isAggregate ? `Round ${msg.metadata.round || "?"} Aggregate` : msg.subject}
                      </span>
                      <span className="message-card-time" title={msg.timestamp}>{formatRelativeTime(msg.timestamp)}</span>
                      <button className="message-play-btn" onClick={(e) => { e.stopPropagation(); playMessage(msg.id, `${msg.subject || ""}. ${msg.body}`, msg.from.split(":")[0]); }} title={playingMsgId === msg.id ? "Stop" : "Play"}>{playingMsgId === msg.id ? "\u23F9" : "\u25B6"}</button>
                    </div>
                    <div className="discussion-event-body">{msg.body}</div>
                  </div>
                );
              }

              // System messages (turn changes) render as compact centered lines
              if (msg.type === "system") {
                return (
                  <div key={msg.id} className="message-system" role="status" aria-label={msg.body}>
                    {msg.body}
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
                      <span className="message-card-from message-card-from-clickable" style={{ color: getRoleColor(fromRole) }} onClick={(e) => { e.stopPropagation(); handleReplyTo(msg.from); }} title={`Reply to ${msg.from}`}>{formatSender(msg)}</span>
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
                    <span className="message-card-from message-card-from-clickable" style={{ color: getRoleColor(fromRole) }} onClick={(e) => { e.stopPropagation(); handleReplyTo(msg.from); }} title={`Reply to ${msg.from}`}>
                      {formatSender(msg)}
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
          {/* Pipeline: "Waiting for response" inline indicator */}
          {discussionState?.active && discussionState.mode === "pipeline" && discussionState.phase === "pipeline_active" && discussionState.pipeline_order && (() => {
            const currentAgent = discussionState.pipeline_order[discussionState.pipeline_stage ?? 0];
            if (!currentAgent) return null;
            const [role] = currentAgent.split(":");
            const roleColor = ROLE_COLORS[role] || HASH_PALETTE[hashSlug(role) % HASH_PALETTE.length];
            // Check if the current agent has already posted in the latest messages
            const msgs = project?.messages || [];
            const lastFew = msgs.slice(-5);
            const hasPosted = lastFew.some(m => m.from === currentAgent && m.type !== "system");
            if (hasPosted) return null;
            return (
              <div className="pipeline-inline-waiting" style={{ borderLeftColor: roleColor }}>
                <span className="pipeline-inline-waiting-dot" style={{ background: roleColor }} />
                <span style={{ color: roleColor, fontWeight: 600 }}>{currentAgent}</span>
                <span style={{ color: "#8899a6", marginLeft: 6 }}>is composing a response...</span>
              </div>
            );
          })()}
          <div ref={messagesEndRef} />
        </div>

        {/* New messages indicator */}
        {newMsgCount > 0 && (
          <button className="new-messages-indicator" onClick={scrollToBottom}>
            {newMsgCount} new message{newMsgCount !== 1 ? "s" : ""} &darr;
          </button>
        )}

        {/* PR H3: moderator-action error toast.
            Why: previously End Session / Pause failures were silent; users couldn't
            tell whether the backend rejected them or the click didn't register.
            aria-live="assertive" because an action-required error should interrupt
            screen readers (platform-engineer msg 175 priority split). */}
        {modErrorToast && (
          <div className="moderator-error-toast" role="alert" aria-live="assertive">
            <span className="moderator-error-toast-text">{modErrorToast}</span>
            <button
              className="moderator-error-toast-dismiss"
              onClick={() => setModErrorToast(null)}
              aria-label="Dismiss error"
            >
              {"\u00D7"}
            </button>
          </div>
        )}

        {/* PR H3 v2: End Session typed-confirm modal. Rendered here (rather than
            inside DiscussionPanel) so the parent owns the state and the MCP
            invoke lives alongside the other moderator handlers. Backdrop click,
            Escape, and Cancel all route to onCancel; onConfirm receives the
            validated reason string. */}
        <EndSessionConfirmModal
          open={endConfirmOpen}
          topic={discussionState?.topic ?? undefined}
          onConfirm={handleConfirmEndSession}
          onCancel={() => setEndConfirmOpen(false)}
        />

        {/* Live-region announcer for moderator-action confirmations (separate
            from the error toast so assistive-tech users hear both). */}
        {endSessionAnnouncement && (
          <div className="sr-only" role="status" aria-live="polite">
            {endSessionAnnouncement}
          </div>
        )}

        {/* Compose Bar */}
        <div className="compose-bar">
          <select
            className="compose-target"
            value={msgTo}
            onChange={(e) => setMsgTo(e.target.value)}
          >
            <option value="all">@ Everyone</option>
            <option value="human">@ Human</option>
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
            ref={composeInputRef}
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

        {/* Audience Panel Modal — REMOVED: audience config is now in Start Discussion dialog */}




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

        <StartSequenceModal
          open={startSequenceOpen}
          onClose={() => setStartSequenceOpen(false)}
          projectDir={projectDir}
          candidates={
            ((project?.sessions || [])
              .filter((s) => s.status === "active")
              .map<StartSequenceCandidate>((s) => ({
                id: `${s.role}:${s.instance}`,
                title: project?.config?.roles?.[s.role]?.title ?? s.role,
              })))
          }
        />

        {/* Start Session Dialog (session = pipeline / delphi / oxford / red_team / continuous) */}
        {startDiscussionOpen && (
          <div className="confirm-dialog" onClick={() => setStartDiscussionOpen(false)}>
            <div className="start-discussion-dialog" onClick={(e) => e.stopPropagation()}>
              <div className="confirm-dialog-title">Start Session</div>

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

              {/* Background Agents — auto-start with session */}
              <div className="sd-section-label">Background Agents (auto-start with session)</div>
              <div className="sd-agents-config">
                <div className="sd-agent-row">
                  <label style={{ display: "flex", alignItems: "center", gap: "8px", cursor: "pointer" }}>
                    <input
                      type="checkbox"
                      checked={sdModeratorEnabled}
                      onChange={(e) => setSdModeratorEnabled(e.target.checked)}
                    />
                    <span className="sd-agent-dot" style={{ background: sdModeratorEnabled ? "#9b59b6" : "#657786" }} />
                    <div className="sd-agent-info">
                      <span className="sd-agent-name">Session Moderator</span>
                      <span className="sd-agent-desc">{sdModeratorEnabled
                        ? "Guides conversation, manages rounds, enforces turn order, and produces decision records."
                        : "Disabled — session will run without moderation. No stall detection, no synthesis, no round management."
                      }</span>
                    </div>
                  </label>
                  <span className="sd-agent-badge" style={{ opacity: sdModeratorEnabled ? 1 : 0.4 }}>{sdModeratorEnabled ? "Auto" : "Off"}</span>
                </div>
                <div className="sd-agent-row">
                  <label style={{ display: "flex", alignItems: "center", gap: "8px", cursor: "pointer" }}>
                    <input
                      type="checkbox"
                      checked={sdAudienceEnabled}
                      onChange={(e) => setSdAudienceEnabled(e.target.checked)}
                    />
                    <span className="sd-agent-dot" style={{ background: sdAudienceEnabled ? "#17bf63" : "#657786" }} />
                    <div className="sd-agent-info">
                      <span className="sd-agent-name">Audience Pool</span>
                      <span className="sd-agent-desc">Panel of AI personas that independently react to the discussion. Off by default.</span>
                    </div>
                  </label>
                </div>
                {sdAudienceEnabled && tauriPools.length > 0 && (
                  <div className="sd-audience-inline-config">
                    <label className="sd-audience-config-label">
                      Pool:
                      <select
                        value={sdAudiencePool}
                        onChange={e => setSdAudiencePool(e.target.value)}
                        className="sd-audience-select"
                      >
                        {tauriPools.map(p => (
                          <option key={p.id} value={p.id}>{p.name} ({p.persona_count} personas)</option>
                        ))}
                      </select>
                    </label>
                    <label className="sd-audience-config-label">
                      Active per round: {sdAudienceSize}
                      <input
                        type="range"
                        min={1}
                        max={10}
                        value={sdAudienceSize}
                        onChange={e => setSdAudienceSize(parseInt(e.target.value, 10))}
                        style={{ width: "120px" }}
                      />
                    </label>
                  </div>
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
                  {sdStarting ? "Starting..." : "Start Session"}
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
              <div className="confirm-dialog-title">{editingGroupSlug ? "Edit Role Group" : "Create Role Group"}</div>
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
                  >{editingGroupSlug ? "Update Group" : "Save Group"}</button>
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
