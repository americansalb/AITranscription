import { useEffect, useLayoutEffect, useState, useRef, useMemo, useCallback, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { ParsedProject, BoardMessage, RoleStatus, SessionBinding, QuestionChoice, FileClaim, DiscussionState, Section, RosterSlot, RoleConfig, RoleGroup, ActiveDelphiDebate } from "../lib/collabTypes";
import { DELPHI_DEFAULTS } from "../lib/collabTypes";
import { BUILTIN_ROLE_GROUPS } from "../utils/roleGroupPresets";
import { RoleBriefingModal } from "./RoleBriefingModal";
import { AdjustBalanceModal, type AdjustDirection } from "./AdjustBalanceModal";
import { EconomySettingsModal } from "./EconomySettingsModal";
import { OxfordSetupModal } from "./OxfordSetupModal";
import { DelphiSetupModal } from "./DelphiSetupModal";
import { useToast } from "./Toast";
// AssemblyBanner removed per spec §11 step 3 (#954 vote-3 gate cleared by
// R1 6/6 + R2 9/9 + R5 18/18 tests passing). ProtocolPanel is the sole
// section-pinned widget. Legacy `assembly_line` MCP tool still alive
// (Slice 6 owns that decom) but the UI surface is unified.
import { ProtocolPanel } from "./ProtocolPanel";
import { AssemblyControls } from "./AssemblyControls";
import { Avatar, parseSeatInstance } from "./Avatar";
import { useProtocolState } from "../hooks/useProtocolState";
import { detectMicTo, type SeatRef } from "./ProtocolPanel/composer/micToDetector";
import { MicToHint } from "./ProtocolPanel/composer/MicToHint";
import { getAvailableVoices, fetchAvailableVoices, getDefaultVoice } from "../lib/queueStore";
import { getAuthToken } from "../lib/api";
import { CANONICAL_TAGS, ROLE_TEMPLATES, generateBriefing, type PeerRole, type RoleTemplate, type RoleStats } from "../utils/briefingGenerator";
import { DecisionPanel } from "./DecisionPanel";
import { CollapsibleSection } from "./CollapsibleSection";
import { RolesTab } from "./RolesTab";
import { useProjectDir } from "../contexts/ProjectDirContext";
import { loadJSON, saveJSON, isBoolean } from "../lib/persistedState";

const STAT_AXES: Array<{ key: keyof RoleStats; label: string; short: string; hint: string }> = [
  { key: "td", label: "Technical Depth", short: "TD", hint: "Code, architecture, systems engagement" },
  { key: "ar", label: "Adversarial Rigor", short: "AR", hint: "Push-back + verification intensity" },
  { key: "cp", label: "Communication Precision", short: "CP", hint: "Clarity + conciseness in messages" },
  { key: "do", label: "Domain Ownership", short: "DO", hint: "Depth in one area vs spread" },
  { key: "pd", label: "Process Discipline", short: "PD", hint: "Verify-before-asserting reflex" },
  { key: "ja", label: "Judgment Under Ambiguity", short: "JA", hint: "Clean calls under uncertainty" },
];
const DEFAULT_STATS: RoleStats = { td: 5, ar: 5, cp: 5, do: 5, pd: 5, ja: 5 };
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

// Phase 5 Chitragupta (human msg 1872): raw rows returned by read_currency_feed_cmd.
// Backend returns the verbatim currency.jsonl ledger row; ALL human-readable
// formatting + color classification happens here in the frontend.
interface CurrencyFeedRow {
  id?: string;
  type?: string;          // init | credit | debit | escrow_hold | escrow_release | passive | interest | clawback | penalty | reinstate
  seat?: string;          // role:instance label
  amount?: number;        // copper
  reason?: string;        // prose (per ruling 5) — matched by keyword, not equality
  ref_msg?: number | string;
  balance_after?: number;
  escrow_id?: string;
  release_turn?: number;
  action_kind?: string;   // Phase 2 opcode hint
  turn?: number;          // Phase 2 turn counter at write-time (for per-turn batching)
  linked_edit_msg?: number; // Phase 4 Test→Edit link
  at?: string;            // ISO timestamp
}

// Phase 6 Bounties (human msg 2074): raw bounties.jsonl row.
interface BountyRow {
  id: string;
  description?: string;
  amount?: number;
  posted_by?: string;
  deadline_turn?: number;
  status?: string;        // open | claimed | submitted | approved | rejected | expired | abandoned
  claimant?: string | null;
  claim_stake?: number;
  submission_msg?: number | null;
  approved_by?: string | null;
  last_rejection_reason?: string | null;
  posted_at?: string;
  resolved_at?: string | null;
  turn_posted?: number;
}

// Phase 7 (human msg 2074): one end-of-session snapshot from currency-history/.
interface SessionSnapshotSeat {
  final_balance?: number;
  total_earned?: number;
  total_lost?: number;
  disputes_won?: number;
  disputes_lost?: number;
  bounties_completed?: number;
  times_timed_out?: number;
}
interface SessionSnapshot {
  session_date?: string;
  seats?: Record<string, SessionSnapshotSeat>;
}

interface DisputeMessage {
  seat?: string;
  text?: string;
  at?: string;
}

interface DisputeRow {
  id?: string;
  challenger?: string;
  target?: string;
  target_msg?: number | string;   // challenged board msg #
  objection_reason?: string;
  pool?: number;                   // copper in escrow pool
  pool_breakdown?: Array<{ seat?: string; amount?: number; reason?: string }>;
  status?: string;                 // open | resolved
  resolution?: string;
  judge?: string | null;
  messages?: DisputeMessage[];
  edit_paths?: string[];
  edit_line_count?: number;
  opened_at?: string;
  resolved_at?: string;
}

type CurrencyTier = "earn" | "hold" | "loss" | "dispute" | "passive" | "destroyed" | "bounty" | "session-start";

// The 13-row transaction→display mapping (human msg 1872, verbatim).
// reason is prose so sub-types are matched by keyword (includes), not equality.
function formatCurrencyLine(row: CurrencyFeedRow): { text: string; tier: CurrencyTier } {
  const seat = row.seat || "someone";
  const amt = typeof row.amount === "number" ? Math.abs(row.amount).toLocaleString() : "?";
  const reason = (row.reason || "").toLowerCase();
  const kind = (row.action_kind || "").toLowerCase();
  const ref = row.ref_msg != null ? `#${row.ref_msg}` : "";
  const has = (...keys: string[]) => keys.some((k) => reason.includes(k) || kind.includes(k));

  switch (row.type) {
    case "init":
      // Phase 7 (c) — Session-start carry-over banner. Backend writes a
      // single Init row with multi-line `reason` summarizing per-seat
      // carry-over. Renderer keeps the prose verbatim (CSS handles wrap).
      if (row.reason && row.reason.startsWith("Session started. Carry-over:"))
        return { text: row.reason, tier: "session-start" as CurrencyTier };
      return { text: `${seat} joined with 10,000 copper`, tier: "passive" };
    case "escrow_hold":
      return { text: `${seat} — ${amt} copper held in escrow${row.release_turn != null ? ` (turn ${row.release_turn})` : ""}`, tier: "hold" };
    case "escrow_release":
      return { text: `${seat} — ${amt} copper escrow released`, tier: "earn" };
    case "passive":
      return { text: `${seat} earned 1 copper passive`, tier: "passive" };
    case "interest":
      return { text: `${seat} earned ${amt} copper interest`, tier: "earn" };
    case "clawback":
      return { text: `${seat} — ${amt} copper seized for dispute`, tier: "dispute" };
    case "penalty":
      return { text: `${seat} penalized ${amt} copper (adversarial pass)`, tier: "loss" };
    case "reinstate":
      return { text: `${seat} reinstated — balance reset to 0`, tier: "earn" };
    case "bounty_stake":
      return { text: `${seat} staked ${amt} copper to claim a bounty`, tier: "bounty" };
    case "bounty_earn":
      return { text: `${seat} earned ${amt} copper completing a bounty`, tier: "bounty" };
    case "bounty_clawback":
      return { text: `${seat} — ${amt} copper clawed back on bounty objection`, tier: "dispute" };
    case "bounty_expire":
      return { text: `${amt} copper destroyed on expired/abandoned bounty`, tier: "destroyed" };
    case "credit":
      if (has("dispute_won", "dispute won", "won dispute"))
        return { text: `${seat} won dispute — ${amt} copper awarded`, tier: "dispute" };
      if (has("edit"))
        return { text: `${seat} earned ${amt} copper editing${ref ? ` (msg ${ref})` : ""}`, tier: "earn" };
      if (has("pass"))
        return { text: `${seat} earned 1 copper passing`, tier: "earn" };
      if (has("speak"))
        return { text: `${seat} earned ${amt} copper speaking${ref ? ` (msg ${ref})` : ""}`, tier: "earn" };
      return { text: `${seat} earned ${amt} copper${ref ? ` (msg ${ref})` : ""}`, tier: "earn" };
    case "debit":
      if (has("pool_destroyed", "pool destroyed"))
        return { text: `Pool destroyed — ${amt} copper to catnip`, tier: "destroyed" };
      if (has("objection"))
        return { text: `${seat} filed objection — 50 copper`, tier: "dispute" };
      return { text: `${seat} — ${amt} copper debited${row.reason ? ` (${row.reason})` : ""}`, tier: "loss" };
    default:
      return { text: `${seat} — ${amt} copper${row.reason ? ` (${row.reason})` : ""}`, tier: "passive" };
  }
}

// Change #1 (human msg 2262): per-message economic footer. The economic story
// belongs ON the message that caused it, not only in the sidebar feed. Given the
// currency.jsonl rows whose `ref_msg` points at one board message (plus an
// optional dispute targeting it), collapse them into a short list of footer
// chips: the earn + its escrow state, any penalty/clawback, and any objection.
// Returns null when there's nothing economic to show (e.g. human messages, which
// never earn currency, so naturally produce no rows).
type EconChip = { text: string; tier: CurrencyTier };
function buildMsgEconomy(rows: CurrencyFeedRow[] | undefined, dispute: DisputeRow | undefined): EconChip[] | null {
  const chips: EconChip[] = [];
  if (rows && rows.length) {
    const hold = rows.find((r) => r.type === "escrow_hold");
    const release = rows.find((r) => r.type === "escrow_release");
    const credit = rows.find((r) => r.type === "credit");
    const penalty = rows.find((r) => r.type === "penalty");
    const clawback = rows.find((r) => r.type === "clawback");

    // The action label: prefer the explicit action_kind on the hold/credit row,
    // else sniff the reason prose (speak/pass/edit/test).
    const reason = `${hold?.reason ?? ""} ${credit?.reason ?? ""}`.toLowerCase();
    const actLabel =
      (hold?.action_kind || credit?.action_kind || "").toLowerCase() ||
      (reason.includes("edit") ? "edit"
        : reason.includes("test") ? "test"
        : reason.includes("pass") ? "pass"
        : reason.includes("speak") ? "speak"
        : "earn");

    // The earn magnitude: the credit (settled) amount, else the held amount.
    const earnAmt =
      typeof credit?.amount === "number" ? Math.abs(credit.amount)
        : typeof hold?.amount === "number" ? Math.abs(hold.amount)
        : null;

    if (earnAmt != null) {
      if (release || credit) {
        chips.push({ text: `+${earnAmt}⊕ ${actLabel} · ✓ released`, tier: "earn" });
      } else if (hold) {
        const turnTxt = hold.release_turn != null ? ` · in escrow → turn ${hold.release_turn}` : " · in escrow";
        chips.push({ text: `+${earnAmt}⊕ ${actLabel}${turnTxt}`, tier: "hold" });
      } else {
        chips.push({ text: `+${earnAmt}⊕ ${actLabel}`, tier: "earn" });
      }
    }
    if (penalty && typeof penalty.amount === "number") {
      chips.push({ text: `−${Math.abs(penalty.amount)}⊕ penalty`, tier: "loss" });
    }
    if (clawback && typeof clawback.amount === "number") {
      chips.push({ text: `−${Math.abs(clawback.amount)}⊕ clawed back`, tier: "dispute" });
    }
  }
  if (dispute) {
    if (dispute.status === "open") {
      chips.push({
        text: `⊗ objected by ${dispute.challenger ?? "?"} · pool ${dispute.pool ?? 0}⊕`,
        tier: "dispute",
      });
    } else {
      chips.push({ text: `⊗ ${dispute.resolution || "resolved"}`, tier: "dispute" });
    }
  }
  return chips.length ? chips : null;
}

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

// Minimal markdown renderer for message bodies (human msg 2981).
// Subset covered: **bold**, `inline code`, ```fenced code blocks```, # headers,
// - / * / 1. lists, > blockquotes. Storage/transport unchanged — render-only.
// Long messages (>100 words) collapse to ~3-line preview with expand toggle;
// the full text stays in the DOM behind the toggle (not deleted/summarized)
// per human directive "All content must remain accessible."
function renderInline(text: string, keyPrefix: string): ReactNode[] {
  // Tokenize on `code` and **bold** simultaneously via single regex pass.
  // Backtick takes precedence (so **`x`** keeps the literal asterisks inside code).
  const parts: ReactNode[] = [];
  const re = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    if (m[1]) {
      parts.push(<code key={`${keyPrefix}-c${i++}`}>{m[1].slice(1, -1)}</code>);
    } else if (m[2]) {
      parts.push(<strong key={`${keyPrefix}-b${i++}`}>{m[2].slice(2, -2)}</strong>);
    }
    last = re.lastIndex;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

function renderMarkdown(body: string): ReactNode[] {
  const out: ReactNode[] = [];
  const lines = body.split("\n");
  let i = 0;
  let key = 0;
  while (i < lines.length) {
    const line = lines[i];
    // Fenced code block
    if (line.trim().startsWith("```")) {
      const codeLines: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith("```")) {
        codeLines.push(lines[i]);
        i++;
      }
      if (i < lines.length) i++; // skip closing fence
      out.push(<pre key={`md-${key++}`}><code>{codeLines.join("\n")}</code></pre>);
      continue;
    }
    // Header
    const hMatch = /^(#{1,3})\s+(.*)$/.exec(line);
    if (hMatch) {
      const level = hMatch[1].length;
      const content = renderInline(hMatch[2], `md-${key}`);
      if (level === 1) out.push(<h1 key={`md-${key++}`}>{content}</h1>);
      else if (level === 2) out.push(<h2 key={`md-${key++}`}>{content}</h2>);
      else out.push(<h3 key={`md-${key++}`}>{content}</h3>);
      i++;
      continue;
    }
    // Blockquote (consecutive > lines)
    if (/^>\s/.test(line)) {
      const quoteLines: string[] = [];
      while (i < lines.length && /^>\s?/.test(lines[i])) {
        quoteLines.push(lines[i].replace(/^>\s?/, ""));
        i++;
      }
      out.push(
        <blockquote key={`md-${key++}`}>
          {quoteLines.map((ql, j) => (
            <div key={j}>{renderInline(ql, `md-${key}-q${j}`)}</div>
          ))}
        </blockquote>
      );
      continue;
    }
    // Unordered list (consecutive - or * lines)
    if (/^[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^[-*]\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^[-*]\s+/, ""));
        i++;
      }
      out.push(
        <ul key={`md-${key++}`}>
          {items.map((it, j) => (
            <li key={j}>{renderInline(it, `md-${key}-li${j}`)}</li>
          ))}
        </ul>
      );
      continue;
    }
    // Ordered list (consecutive `1. ` `2. ` lines)
    if (/^\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\d+\.\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\d+\.\s+/, ""));
        i++;
      }
      out.push(
        <ol key={`md-${key++}`}>
          {items.map((it, j) => (
            <li key={j}>{renderInline(it, `md-${key}-oli${j}`)}</li>
          ))}
        </ol>
      );
      continue;
    }
    // Blank line → paragraph separator
    if (line.trim() === "") {
      out.push(<div key={`md-${key++}`} className="md-blank" />);
      i++;
      continue;
    }
    // Plain text line — collect consecutive non-special lines into one paragraph
    const para: string[] = [line];
    i++;
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !lines[i].trim().startsWith("```") &&
      !/^(#{1,3}\s|>\s|[-*]\s+|\d+\.\s+)/.test(lines[i])
    ) {
      para.push(lines[i]);
      i++;
    }
    out.push(
      <p key={`md-${key++}`}>{renderInline(para.join("\n"), `md-${key}`)}</p>
    );
  }
  return out;
}

function MarkdownBody({ text, className }: { text: string; className?: string }) {
  const [expanded, setExpanded] = useState(false);
  const wordCount = text.trim() === "" ? 0 : text.trim().split(/\s+/).length;
  const isLong = wordCount > 100;
  // Human msg 3313: highlight+copy must capture FULL body even when collapsed.
  // Render the full markdown tree ALWAYS — visual collapse is a CSS-only
  // `max-height: overflow-hidden` clip (with a fade-out gradient). Selection
  // can extend through the hidden region; browsers copy the underlying DOM
  // text including the clipped portion. No conditional render swap that
  // strips text out of the DOM.
  const collapsedClipped = isLong && !expanded;
  return (
    <div className={`md-body${isLong ? " md-body-collapsible" : ""}${collapsedClipped ? " md-body-clipped" : ""}${className ? ` ${className}` : ""}`}>
      <div className="md-body-content">
        {renderMarkdown(text)}
      </div>
      {collapsedClipped && <span className="md-preview-fade" aria-hidden="true">…</span>}
      {isLong && (
        <button
          type="button"
          className="md-expand-toggle"
          onClick={(e) => { e.stopPropagation(); setExpanded(!expanded); }}
          aria-expanded={expanded}
        >
          {expanded ? "Collapse" : `Expand (${wordCount} words)`}
        </button>
      )}
    </div>
  );
}

// First answer wins, preserves messages.find(...) original semantics.
// Build once per messages-reference change via useMemo at the call site.
function buildAnswerLookup(messages: BoardMessage[]): Map<number, BoardMessage> {
  const m = new Map<number, BoardMessage>();
  for (const msg of messages) {
    if (
      msg.type === "answer" &&
      typeof msg.metadata?.in_reply_to === "number" &&
      !m.has(msg.metadata.in_reply_to)
    ) {
      m.set(msg.metadata.in_reply_to, msg);
    }
  }
  return m;
}

function getAnswerForQuestion(
  questionId: number,
  lookup: Map<number, BoardMessage>
): { choiceId: string } | null {
  const answer = lookup.get(questionId);
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
      <MarkdownBody text={msg.body} className="message-card-body" />
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
      {tally.reason && <MarkdownBody text={tally.reason} className="message-card-body" />}
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

// `vaak_collab_project_dir` ownership moved to ProjectDirContext per
// architect msg 5249 + evil-arch F-EA-CTR-A (msg 5246): the standalone
// RolesTab and the (forthcoming, Change C) embedded RolesTab inside this
// CollabTab will both read/write this same key. Routing both mounts
// through one provider's useState closes the divergent-WRITER class.
// The empty-string remove-key semantics now live inside the provider's
// setProjectDir; CollabTab is just a consumer.
const SAVED_PROJECTS_KEY = "vaak_projects";

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
  const { showToast } = useToast();
  const [project, setProject] = useState<ParsedProject | null>(null);
  // ProjectDirContext consumption — replaces the prior local `useState(() => loadPersistedDir())`
  // pattern. Calling `setProjectDir` from the hook both updates the
  // in-memory value and persists to localStorage; the prior paired
  // `setProjectDir(...) + persistDir(...)` call sites collapse to a single
  // call (see startWatching / stopWatching below).
  const { projectDir, setProjectDir } = useProjectDir();
  // Human msg 616/626: replace window.prompt-based +/- buttons with a proper
  // modal. Single seat/direction tracked here; AdjustBalanceModal renders
  // when adjustTarget is non-null.
  const [adjustTarget, setAdjustTarget] = useState<{ seat: string; direction: AdjustDirection } | null>(null);
  // Human msg 657: Economy Settings page modal.
  const [economySettingsOpen, setEconomySettingsOpen] = useState(false);
  // Human msg 706: Oxford debate setup modal.
  const [oxfordSetupOpen, setOxfordSetupOpen] = useState(false);
  // Human msg 1939: Delphi discussion setup modal (Oxford-parity build).
  const [delphiSetupOpen, setDelphiSetupOpen] = useState(false);
  // Per spec §7.1 item 5 — aggregate render with collapse/expand affordance.
  // Keyed by aggregate_message_id so multi-round Delphi reveals can be
  // independently toggled. Defaults to expanded for the latest round.
  const [delphiAggregateExpanded, setDelphiAggregateExpanded] = useState<Record<number, boolean>>({});
  const [delphiRealNamesRevealed, setDelphiRealNamesRevealed] = useState(false);
  // ActiveDelphiDebate — canonical type from collabTypes.ts:298 per spec §5.6
  // serde-parity contract. Rust source: collab.rs:delphi module per SHA-D10.1
  // (commit 4a18a03).
  const [activeDelphi, setActiveDelphi] = useState<ActiveDelphiDebate | null>(null);
  // Human msg 870: active Oxford debate snapshot, polled every 2s. When
  // non-null an End Debate button replaces the Start button so the human can
  // force-end a stuck debate via UI without having to delete JSON files.
  // Extended for human msg 1090/1092 — ActiveOxfordPanel needs current_speaker
  // + turn_count to render the live debate state in the right rail.
  // SHA-10.5 (per ui-architect msg 1400 spec v2 + dev-challenger msg 1402
  // adversarial review): extended with phase + phase_started_at for the
  // phase-machine indicator. Defaults to "none" for legacy pre-SHA-10.1
  // debates (#[serde(default)] back-compat handles read-side).
  const [activeOxford, setActiveOxford] = useState<{
    debate_id: number;
    moderator: string;
    premise: string;
    side_a: string[];
    side_b: string[];
    current_speaker: string | null;
    turn_count: number;
    phase: string;
    phase_started_at: string | null;
  } | null>(null);
  const [watching, setWatching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [autoCollab, setAutoCollab] = useState(false);
  const [humanInLoop, setHumanInLoop] = useState(false);
  const [selectedRole, setSelectedRole] = useState<RoleStatus | null>(null);
  const [msgTo, setMsgTo] = useState("all");
  const [msgBody, setMsgBodyRaw] = useState(() => localStorage.getItem("vaak_compose_draft") || "");
  // Per human msg 4346 "make it so i can add ulimited text to you" — cap removed.
  // localStorage will accept arbitrarily large strings up to ~5-10MB browser limit;
  // if the draft exceeds localStorage quota, setItem silently fails (caught + ignored
  // so typing isn't interrupted). In-memory msgBody state remains unlimited regardless.
  const setMsgBody = (v: string) => {
    setMsgBodyRaw(v);
    try {
      localStorage.setItem("vaak_compose_draft", v);
    } catch {
      // QuotaExceededError on very large drafts → fall back to no-persist;
      // user's in-progress text remains usable until window close.
    }
  };
  const [sending, setSending] = useState(false);
  // Slice 4 — mic_to composer state. Spec §4.3: regex is hint, metadata is
  // authoritative. micToConfirmed is set ONLY when the user clicks the hint's
  // confirm button. Without click, message ships with NO mic_to metadata
  // (the §4.3 defense against "Mic to architect's approach" false positives).
  const [micToConfirmed, setMicToConfirmed] = useState<string | null>(null);
  const [micToHintDismissed, setMicToHintDismissed] = useState(false);
  const [workflowDropdownOpen, setWorkflowDropdownOpen] = useState(false);
  const [discussionModeOpen, setDiscussionModeOpen] = useState(false);
  const [discussionState, setDiscussionState] = useState<DiscussionState | null>(null);
  // Assembly Line state — minimum mic-control mechanism. Reads .vaak/assembly.json
  // via the get_assembly_state Tauri command; toggled by set_assembly_state.
  const [assemblyState, setAssemblyState] = useState<{
    active: boolean;
    current_speaker: string | null;
    rotation_order: string[];
  } | null>(null);
  const [assemblyToggling, setAssemblyToggling] = useState(false);
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [closingRound, setClosingRound] = useState(false);
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [continuousTimeout, setContinuousTimeout] = useState(60);
  // Unused state retained — `setClosingRound`/`setContinuousTimeout` are still
  // referenced inside the handler bodies below + an initializer at L~1337.
  // The displayed `closingRound`/`continuousTimeout` values were rendered by
  // the deleted discussion-status-panel; ConsensusRow in ProtocolPanel will
  // surface equivalent state in a follow-on commit. Suppress TS6133 inline.
  void closingRound; void continuousTimeout;
  // Change D (human msg 5538 / msg 5237 directive 5): autoCollab + humanInLoop
  // read sites removed (header checkboxes deleted). Setters still run from
  // watch_project_dir effect to keep state in sync; suppress TS6133 inline
  // since the values aren't currently displayed anywhere.
  void autoCollab; void humanInLoop;
  // discussionState was read in the deleted "{!discussionState?.active && Discuss}"
  // header gate. setDiscussionState still runs from multiple Tauri callbacks;
  // suppress the read-side warning the same way.
  void discussionState;
  const [startDiscussionOpen, setStartDiscussionOpen] = useState(false);
  const [sdFormat, setSdFormat] = useState<"delphi" | "oxford" | "red_team" | "continuous">("delphi");
  const [sdTopic, setSdTopic] = useState("");
  const [sdParticipants, setSdParticipants] = useState<Record<string, boolean>>({});
  const [sdStarting, setSdStarting] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // Phase 1b (architect msg 484): real ⚙ popover hosting AssemblyControls
  // + ProtocolPanel. Replaces the always-rendered Discussion Mode band
  // expansion. Closes msg-5450 risk via always-visible thin strip above
  // (preset name + Configure ⚙ button); controls live in the popover.
  // P5-v2: DiscussionSettingsPopover removed (AssemblyControls now lives in
  // the sidebar Discussion Mode card). State retained-but-unused for one
  // commit to avoid a wider rip-out; safe to delete in follow-up.
  const [_discussionPopoverOpen, _setDiscussionPopoverOpen] = useState(false);
  void _discussionPopoverOpen; void _setDiscussionPopoverOpen;
  const [interruptTarget, setInterruptTarget] = useState<{ slug: string; instance: number; title: string } | null>(null);
  const [interruptReason, setInterruptReason] = useState("");
  const [buzzedKey, setBuzzedKey] = useState<string | null>(null);
  // Auto-buzz watchdog: tracks which agents have been auto-buzzed in the current staleness episode
  const [openCardMenu, setOpenCardMenu] = useState<string | null>(null); // "slug:instance" key
  const [menuPos, setMenuPos] = useState<{ top: number; right: number; left: number } | null>(null);
  // human #1280: claims used to be visible always; collapsed-by-default
  // hid them. Default expanded so the team's "who's working on what" is
  // surfaced without an extra click. User can still collapse via the title.
  // Layout-density-v1.2: nullable so first render auto-derives (collapsed when
  // claims.length === 0, expanded when > 0) until human explicitly toggles.
  // F-TESTER-LD12-1 sister-fix: persist explicit toggle via Path A symmetric
  // pattern so the user's choice survives reload — matches DecisionPanel +
  // Team Roster persistence behavior.
  const [claimsCollapsed, setClaimsCollapsed] = useState<boolean | null>(
    () => loadJSON<boolean | null>("vaak_collab_claims_collapsed", null, (v): v is boolean | null => v === null || typeof v === "boolean"),
  );
  const updateClaimsCollapsed = (next: boolean) => {
    setClaimsCollapsed(next);
    saveJSON("vaak_collab_claims_collapsed", next);
  };
  // P5-v2 (architect:0 msg 935 + human msg 932 = B "auto-hide empty"):
  // Audience + Replanning rail-sections track collapse state. When count===0
  // the section renders null entirely (no header, no empty-state pixels) per
  // the locked spec; this state only matters when the section actually mounts.
  const [audienceCollapsed, setAudienceCollapsed] = useState<boolean | null>(
    () => loadJSON<boolean | null>("vaak_collab_audience_collapsed", null, (v): v is boolean | null => v === null || typeof v === "boolean"),
  );
  const updateAudienceCollapsed = (next: boolean) => {
    setAudienceCollapsed(next);
    saveJSON("vaak_collab_audience_collapsed", next);
  };
  const [replanningCollapsed, setReplanningCollapsed] = useState<boolean | null>(
    () => loadJSON<boolean | null>("vaak_collab_replanning_collapsed", null, (v): v is boolean | null => v === null || typeof v === "boolean"),
  );
  const updateReplanningCollapsed = (next: boolean) => {
    setReplanningCollapsed(next);
    saveJSON("vaak_collab_replanning_collapsed", next);
  };
  // Discussion Mode card collapse — sidebar slot, top of rail. Default-expanded
  // since the controls (preset / mic mode / moderator / review intensity /
  // plan) are the primary discoverability surface in the new layout.
  const [discussionModeCardCollapsed, setDiscussionModeCardCollapsed] = useState<boolean | null>(
    () => loadJSON<boolean | null>("vaak_collab_discussion_mode_card_collapsed", null, (v): v is boolean | null => v === null || typeof v === "boolean"),
  );
  const updateDiscussionModeCardCollapsed = (next: boolean) => {
    setDiscussionModeCardCollapsed(next);
    saveJSON("vaak_collab_discussion_mode_card_collapsed", next);
  };
  // Currency toggle uses inline handlers in both IIFEs (header badge + Settings
  // entry); the re-fetch-after-invoke pattern (mirrors set_discussion_mode at
  // line ~2384) gives the badge label sub-second refresh, which proved sufficient.
  // The optimistic-state primitive that was here was deleted as dead-code by
  // architect:0 after both IIFEs landed the re-fetch + showToast pattern.
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

  // Layout-density-v1 corrected per human msg 5125 ("I CANT ALLOW YOU TO LOSE
  // FUNCTIONALITY") + ui-arch:1 msg 5128 revised spec: collapse-not-filter.
  // The roster section folds to a one-line header showing status counts;
  // click expands back to the full grid. Every role stays reachable — only
  // the screen-space-when-not-looking is reclaimed. Default false
  // (expanded) per ui-arch lean (A), preserving current discoverability.
  //
  // Persist with JSON.stringify/JSON.parse symmetric pattern per evil-arch
  // F-EA-LAYOUT-LOCALSTORAGE-CLASS (msg 5123) — matches RolesTab Path A
  // (4796f5f) so a future Path B shared helper folds cleanly.
  const [rosterSectionCollapsed, setRosterSectionCollapsed] = useState<boolean>(
    () => loadJSON("vaak_collab_roster_collapsed", false, isBoolean),
  );
  const updateRosterSectionCollapsed = (next: boolean) => {
    setRosterSectionCollapsed(next);
    saveJSON("vaak_collab_roster_collapsed", next);
  };
  // Track D v1.2 (per human msg 163 "i cant uncollapse team tab") — v1.1's
  // per-render force-expand trapped the user. Replace with a one-time
  // auto-expand on the assembly-OFF → ON edge only; once expanded the user
  // can collapse freely and stays collapsed. autoExpandedForAssemblyRef
  // resets when assembly turns off, so a future enable triggers expand again.
  const autoExpandedForAssemblyRef = useRef<boolean>(false);
  // Change C (CollabTab restructure spec, architect msg 5238/5249/5259):
  // Team Section tab state — inside the existing roster-section
  // CollapsibleSection band, toggle between the role-cards grid
  // ("roster") and the embedded RolesTab edit surface ("roles").
  // F-UIA-CTR-2: default "roster" (read first, edit second). Persisted
  // across reloads.
  // F-DC-CTR-2: standalone RolesTab top-level Tauri tab REMAINS active
  // — this is the embedded dual-mount path. Both mounts consume
  // ProjectDirContext (closes F-EA-CTR-A divergent-WRITER class).
  type TeamTab = "roster" | "roles";
  const isTeamTab = (v: unknown): v is TeamTab => v === "roster" || v === "roles";
  const [activeTeamTab, setActiveTeamTab] = useState<TeamTab>(
    () => loadJSON<TeamTab>("vaak_collab_team_section_active_tab", "roster", isTeamTab),
  );
  const updateActiveTeamTab = (next: TeamTab) => {
    setActiveTeamTab(next);
    saveJSON("vaak_collab_team_section_active_tab", next);
  };

  // Change B (CollabTab restructure spec, architect msg 5238/5249/5259):
  // Discussion Mode card collapse-state. Wraps AssemblyControls in a
  // CollapsibleSection so the human can fold away the ~quarter-screen
  // assembly UI when they're not actively managing rotation/mic state.
  //
  // Sister-fix-CB1 (architect msg 5341) made the default COLLAPSED to
  // free first-load screen space per msg 5237 directive 4.
  //
  // Sister-fix-CB3 (human msg 5567 "i don't see the same UI mic passing
  // and arrows and stuff as before very important to assembly mode"):
  // tristate (null | true | false) — `null` derives default from
  // twoControlsProtocol presence at render time. When assembly IS active
  // (twoControlsProtocol non-null), card default-expands so the mic-
  // passing controls are immediately visible — the case where the
  // controls ARE active engagement. When no assembly is running, card
  // stays collapsed per the screen-space-reclamation intent. User's
  // explicit toggle (true/false) still overrides the derivation and
  // persists across reloads. Same tristate pattern as `claimsCollapsed`.
  // Phase 1b (architect msg 484) replaced inline Discussion Mode
  // CollapsibleSection with the always-visible strip + ⚙ popover.
  // The localStorage key remains preserved for back-compat with prior-
  // session state; not read by the new strip+popover flow.
  const [treeExpanded, setTreeExpanded] = useState<Set<string>>(new Set());
  const [teamSectionOpen, setTeamSectionOpen] = useState(false);
  // Phase 5 Chitragupta UI state (human msg 1872/1924).
  // Flow Feed collapse pref persists across reloads; Judge Seat + Balance Bar
  // expand are ephemeral (re-collapse on remount is fine — they re-derive from
  // live data each session).
  const [feedCollapsed, setFeedCollapsed] = useState<boolean>(
    () => loadJSON<boolean>("vaak_collab_flow_feed_collapsed", false, (v): v is boolean => typeof v === "boolean"),
  );
  const toggleFeedCollapsed = () =>
    setFeedCollapsed((prev) => { const next = !prev; saveJSON("vaak_collab_flow_feed_collapsed", next); return next; });
  const [judgeExpanded, setJudgeExpanded] = useState(false);
  const [balanceExpanded, setBalanceExpanded] = useState(false);
  // Phase 5 (human msg 1971): "More Stats" popup — deep currency breakdown.
  // Ephemeral (re-derives from live data on open).
  const [statsOpen, setStatsOpen] = useState(false);
  // Phase 6 (human msg 2074): Bounty Board collapse pref.
  const [bountyBoardCollapsed, setBountyBoardCollapsed] = useState<boolean>(
    () => loadJSON<boolean>("vaak_collab_bounty_board_collapsed", false, (v): v is boolean => typeof v === "boolean"),
  );
  const toggleBountyBoardCollapsed = () =>
    setBountyBoardCollapsed((p) => { const n = !p; saveJSON("vaak_collab_bounty_board_collapsed", n); return n; });
  // Phase 5 (human msg 1971): inline currency notices in the message timeline.
  // Default OFF per human msg 2077/2082 ("should not be 10 messages every turn
  // about gold and copper" — too disruptive). Opt-in via the 🔕 toggle, and even
  // when ON only SIGNIFICANT events (disputes/losses/pool-destroyed) interleave —
  // routine passive/escrow/interest stays in the sidebar Chitragupta feed.
  const [inlineCurrencyNotices, setInlineCurrencyNotices] = useState<boolean>(
    () => loadJSON<boolean>("vaak_collab_inline_currency_notices", false, (v): v is boolean => typeof v === "boolean"),
  );
  const toggleInlineCurrencyNotices = () =>
    setInlineCurrencyNotices((prev) => { const next = !prev; saveJSON("vaak_collab_inline_currency_notices", next); return next; });
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
  // Two-controls protocol subscription (commit B). Reads commit A's new
  // floor fields (assembly_active, phase, mic_passing_mode, etc.) for
  // AssemblyControls. Listener doubles up with ProtocolPanel's internal
  // useProtocolState — both re-render on protocol_changed pushes; cost is
  // one extra get_protocol_cmd per push, accepted for commit B simplicity.
  const {
    state: twoControlsProtocol,
    mutate: twoControlsMutate,
    lastError: twoControlsLastError,
  } = useProtocolState(projectDir, activeSection || "default");
  // Track D v1.2 — one-time auto-expand of the Team band on the assembly
  // OFF → ON edge. Replaces v1.1's per-render force-override which trapped
  // the user (human msg 163). Once the band auto-expands, user's manual
  // collapse is respected absolutely. Ref resets when assembly turns off
  // so a future enable triggers another auto-expand.
  useEffect(() => {
    const isAssemblyActive = twoControlsProtocol?.floor?.assembly_active === true;
    if (isAssemblyActive && !autoExpandedForAssemblyRef.current) {
      setRosterSectionCollapsed(false);
      autoExpandedForAssemblyRef.current = true;
    } else if (!isAssemblyActive && autoExpandedForAssemblyRef.current) {
      autoExpandedForAssemblyRef.current = false;
    }
  }, [twoControlsProtocol?.floor?.assembly_active]);
  const [newSectionName, setNewSectionName] = useState("");
  const [creatingSectionMode, setCreatingSectionMode] = useState(false);
  // Human msg 3191: max 4 visible tabs (active + 3 most recently active);
  // remaining sections live in an "All sections" dropdown with name-filter
  // typing. Everything reachable; tab bar stops scrolling past 4.
  const [sectionDropdownOpen, setSectionDropdownOpen] = useState(false);
  const [sectionFilterText, setSectionFilterText] = useState("");

  // Keepalive v3 (per architect msg 5143 queue + msg 4885 Path A spec).
  // Per-seat liveness map derived from `.vaak/sessions/<role>-<inst>.json:
  // last_alive_at_ms` via main.rs:list_active_seats_cmd (SHA 533b458). Polled
  // on a 30s interval; consumed by the roster card render to show an amber
  // ring + "(reconnecting…)" suffix on stale seats and a gray-dashed dot on
  // seats whose keepalive file is missing/unread.
  //
  // The map is keyed by "role:instance" so the roster card render does a
  // straight lookup. Backend already filters bindings to status==="active"
  // before returning seats, so we don't need to dedupe.
  const [seatAliveMap, setSeatAliveMap] = useState<Map<string, "active" | "stale" | "unknown">>(new Map());
  useEffect(() => {
    if (!projectDir || !window.__TAURI__) return;
    let cancelled = false;
    const fetchAlive = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const resp = await invoke<{ seats: Array<{ label: string; alive_state?: string }> }>(
          "list_active_seats_cmd",
          { dir: projectDir }
        );
        if (cancelled) return;
        const next = new Map<string, "active" | "stale" | "unknown">();
        for (const seat of resp.seats || []) {
          const state = seat.alive_state;
          // "human" entry is the synthesized human:0 seat — skip it; never
          // shown as a roster card. Anything else maps to the union we render.
          if (state === "active" || state === "stale" || state === "unknown") {
            next.set(seat.label, state);
          }
        }
        setSeatAliveMap(next);
      } catch {
        // Backward-compat: pre-keepalive Tauri binary won't have this command
        // OR an empty .vaak/sessions dir will return errors. Swallow + leave
        // map empty → all roster cards render with no alive-state styling
        // (matches pre-v3 behavior).
      }
    };
    void fetchAlive();
    const id = window.setInterval(fetchAlive, 30_000);
    return () => { cancelled = true; window.clearInterval(id); };
  }, [projectDir]);
  // Currency Phase 1 UI display (human msg 1300 "you need it in the UI ...
  // where are the coins"). Polls main.rs:get_currency_balances_cmd on the same
  // cadence as seatAliveMap. Keyed by "role:instance"; consumed by the roster
  // card render to show a gold/silver/copper balance pill. Empty map (pre-
  // currency Tauri binary OR error) → cards render with no pill, no regression.
  const [currencyBalances, setCurrencyBalances] = useState<Map<string, {
    balance_copper: number;
    escrow_held_copper: number;
    timed_out: boolean;
    initialized: boolean;
    display: { gold: number; silver: number; copper: number };
  }>>(new Map());
  // Phase 5 Chitragupta (human msg 1872/1924): raw currency.jsonl rows for the
  // Flow Feed + open disputes for the Judge Seat. Fetched on the SAME interval
  // as balances (no new setInterval, per the directive).
  const [currencyFeed, setCurrencyFeed] = useState<CurrencyFeedRow[]>([]);
  const [openDisputes, setOpenDisputes] = useState<DisputeRow[]>([]);
  const [bounties, setBounties] = useState<BountyRow[]>([]);
  const [sessionHistory, setSessionHistory] = useState<SessionSnapshot[]>([]);
  const [scoreboardCollapsed, setScoreboardCollapsed] = useState<boolean>(
    () => loadJSON<boolean>("vaak_collab_scoreboard_collapsed", true, (v): v is boolean => typeof v === "boolean"),
  );
  const toggleScoreboardCollapsed = () =>
    setScoreboardCollapsed((p) => { const n = !p; saveJSON("vaak_collab_scoreboard_collapsed", n); return n; });
  useEffect(() => {
    if (!projectDir || !window.__TAURI__) return;
    let cancelled = false;
    const fetchBalances = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const resp = await invoke<{ seats: Array<{
          label: string;
          balance_copper: number;
          escrow_held_copper: number;
          timed_out: boolean;
          initialized: boolean;
          display: { gold: number; silver: number; copper: number };
        }> }>("get_currency_balances_cmd", { dir: projectDir });
        if (cancelled) return;
        const next = new Map<string, {
          balance_copper: number; escrow_held_copper: number;
          timed_out: boolean; initialized: boolean;
          display: { gold: number; silver: number; copper: number };
        }>();
        for (const s of resp.seats || []) {
          next.set(s.label, {
            balance_copper: s.balance_copper,
            escrow_held_copper: s.escrow_held_copper,
            timed_out: s.timed_out,
            initialized: s.initialized,
            display: s.display,
          });
        }
        setCurrencyBalances(next);
      } catch {
        // Pre-currency Tauri binary lacks this command → leave map empty;
        // roster cards render with no balance pill (no regression).
      }
      // Flow Feed rows (read_currency_feed_cmd → { rows, total }, newest last).
      // Independent try so a missing feed command doesn't blank balances.
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const feedResp = await invoke<{ rows: CurrencyFeedRow[]; total: number }>(
          "read_currency_feed_cmd", { dir: projectDir, count: 80 });
        if (!cancelled && Array.isArray(feedResp?.rows)) setCurrencyFeed(feedResp.rows);
      } catch { /* command absent until Phase 5 backend rebuild — feed stays empty */ }
      // Open disputes (read_disputes_cmd → { disputes, open }) for the Judge Seat.
      // disputes.jsonl is append-only: the same dispute id appears once per state
      // transition, so collapse to the LATEST row per id for current state
      // (tester:0 contract msg 1935).
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const dispResp = await invoke<{ disputes: DisputeRow[]; open?: { open_by_target?: string[]; open_by_challenger?: string[] } }>(
          "read_disputes_cmd", { dir: projectDir });
        if (!cancelled && Array.isArray(dispResp?.disputes)) {
          const latest = new Map<string, DisputeRow>();
          let anon = 0;
          for (const d of dispResp.disputes) latest.set(d.id ?? `__anon_${anon++}`, d);
          setOpenDisputes(Array.from(latest.values()));
        }
      } catch { /* command absent until Phase 5 backend rebuild — disputes stay empty */ }
      // Phase 6 — bounties (read_bounties_cmd → { bounties, open }). Append-only;
      // collapse to latest row per id.
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const bResp = await invoke<{ bounties: BountyRow[] }>("read_bounties_cmd", { dir: projectDir });
        if (!cancelled && Array.isArray(bResp?.bounties)) {
          const latest = new Map<string, BountyRow>();
          let anon = 0;
          for (const b of bResp.bounties) latest.set(b.id ?? `__anon_${anon++}`, b);
          setBounties(Array.from(latest.values()));
        }
      } catch { /* command absent until Phase 6 backend rebuild — bounties stay empty */ }
      // Phase 7 — lifetime session snapshots (read_currency_history_cmd).
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const hResp = await invoke<{ snapshots: SessionSnapshot[] }>("read_currency_history_cmd", { dir: projectDir });
        if (!cancelled && Array.isArray(hResp?.snapshots)) setSessionHistory(hResp.snapshots);
      } catch { /* command absent until Phase 7 backend rebuild — history stays empty */ }
    };
    void fetchBalances();
    const id = window.setInterval(fetchBalances, 30_000);
    return () => { cancelled = true; window.clearInterval(id); };
  }, [projectDir]);

  // Flow Feed display rows: collapse ALL passive ticks sharing a turn into ONE
  // line — "N seats earned 1 copper passive (turn 47)" (human msg 1872 intent +
  // msg 2082 "should not be 10 messages every turn about gold and copper"). The
  // ledger writes one passive row per seat per turn, appended together, so we
  // batch by the `turn` field — NOT by consecutive same-seat (the old impl,
  // which broke the run on each seat → ~6-8 lines/turn = the human's complaint).
  // Keep the last 50 for render; newest is last (we scroll to bottom).
  const flowFeedRows = useMemo(() => {
    const out: Array<{ key: string; text: string; tier: CurrencyTier; seat?: string; at?: string }> = [];
    let passiveBatch: { turn: number | undefined; count: number; at?: string } | null = null;
    // Change #3 (human msg 2262): interest, like passive, fires once per seat
    // per rotation — six "earned 1 copper interest" lines every turn. Batch it
    // the same way passive is batched: ONE line per turn. Interest amounts vary
    // per seat (1c per 10c held), so we sum the copper instead of counting ×1.
    let interestBatch: { turn: number | undefined; count: number; total: number; at?: string } | null = null;
    const flushPassive = () => {
      if (!passiveBatch) return;
      const turnLabel = passiveBatch.turn != null ? ` (turn ${passiveBatch.turn})` : "";
      out.push({
        key: `passive-turn-${passiveBatch.turn ?? `idx${out.length}`}`,
        text: `${passiveBatch.count} seat${passiveBatch.count === 1 ? "" : "s"} earned 1 copper passive${turnLabel}`,
        tier: "passive",
        at: passiveBatch.at,
      });
      passiveBatch = null;
    };
    const flushInterest = () => {
      if (!interestBatch) return;
      const turnLabel = interestBatch.turn != null ? ` (turn ${interestBatch.turn})` : "";
      out.push({
        key: `interest-turn-${interestBatch.turn ?? `idx${out.length}`}`,
        text: `${interestBatch.count} seat${interestBatch.count === 1 ? "" : "s"} earned ${interestBatch.total.toLocaleString()} copper interest${turnLabel}`,
        tier: "earn",
        at: interestBatch.at,
      });
      interestBatch = null;
    };
    for (const row of currencyFeed) {
      if (row.type === "passive") {
        flushInterest();
        if (passiveBatch && passiveBatch.turn === row.turn) {
          passiveBatch.count += 1;
          passiveBatch.at = row.at;
        } else {
          flushPassive();
          passiveBatch = { turn: row.turn, count: 1, at: row.at };
        }
        continue;
      }
      if (row.type === "interest") {
        flushPassive();
        const amt = typeof row.amount === "number" ? Math.abs(row.amount) : 0;
        if (interestBatch && interestBatch.turn === row.turn) {
          interestBatch.count += 1;
          interestBatch.total += amt;
          interestBatch.at = row.at;
        } else {
          flushInterest();
          interestBatch = { turn: row.turn, count: 1, total: amt, at: row.at };
        }
        continue;
      }
      flushPassive();
      flushInterest();
      const formatted = formatCurrencyLine(row);
      out.push({ key: row.id || `${row.at ?? ""}-${out.length}`, text: formatted.text, tier: formatted.tier, seat: row.seat, at: row.at });
    }
    flushPassive();
    flushInterest();
    return out.slice(-50);
  }, [currencyFeed]);

  // Change #1 (human msg 2262): index currency rows + disputes by the board
  // message they reference, so each message card can render its own economic
  // footer. Keyed by String(msg.id) since ref_msg/target_msg are number|string.
  // Only the most recent ~80 ledger rows are fetched, so footers naturally
  // appear on recent messages and degrade to nothing on older ones.
  const economicByMsg = useMemo(() => {
    const map = new Map<string, CurrencyFeedRow[]>();
    for (const row of currencyFeed) {
      if (row.ref_msg == null) continue;
      const key = String(row.ref_msg);
      const arr = map.get(key);
      if (arr) arr.push(row);
      else map.set(key, [row]);
    }
    return map;
  }, [currencyFeed]);

  const disputeByMsg = useMemo(() => {
    const map = new Map<string, DisputeRow>();
    for (const d of openDisputes) {
      if (d.target_msg == null) continue;
      map.set(String(d.target_msg), d);
    }
    return map;
  }, [openDisputes]);

  // Running net-flow over the visible window: earnings minus losses/destroyed.
  const flowNet = useMemo(() => {
    let net = 0;
    for (const row of currencyFeed) {
      const amt = typeof row.amount === "number" ? Math.abs(row.amount) : 0;
      const tier = formatCurrencyLine(row).tier;
      if (tier === "earn") net += amt;
      else if (tier === "loss" || tier === "destroyed") net -= amt;
    }
    return net;
  }, [currencyFeed]);

  // The open dispute that currently needs a ruling (judge assigned, still open).
  const rulingDispute = useMemo(
    () => openDisputes.find((d) => d.status === "open" && d.judge != null) ?? null,
    [openDisputes],
  );

  // Balance Bar (Surface 3) derived totals + leaderboard. "copper in play" =
  // every initialized seat's spendable balance + escrow held.
  const currencyLeaderboard = useMemo(() => {
    const seats: Array<{ label: string; balance: number; escrow: number }> = [];
    let total = 0;
    for (const [label, b] of currencyBalances.entries()) {
      if (!b.initialized) continue;
      const inPlay = b.balance_copper + b.escrow_held_copper;
      total += inPlay;
      seats.push({ label, balance: b.balance_copper, escrow: b.escrow_held_copper });
    }
    seats.sort((a, b) => (b.balance + b.escrow) - (a.balance + a.escrow));
    return { seats, total };
  }, [currencyBalances]);

  // Phase 7 — lifetime Scoreboard: aggregate every session snapshot per seat
  // (frontend TypeScript per directive). Net = earned - lost; avg = mean final
  // balance across sessions the seat appeared in.
  const scoreboard = useMemo(() => {
    type Agg = {
      label: string; sessions: number; earned: number; lost: number;
      disputesWon: number; disputesLost: number; bounties: number;
      timedOut: number; balanceSum: number;
    };
    const agg = new Map<string, Agg>();
    for (const snap of sessionHistory) {
      const seats = snap.seats || {};
      for (const [label, s] of Object.entries(seats)) {
        const a = agg.get(label) ?? { label, sessions: 0, earned: 0, lost: 0, disputesWon: 0, disputesLost: 0, bounties: 0, timedOut: 0, balanceSum: 0 };
        a.sessions += 1;
        a.earned += s.total_earned ?? 0;
        a.lost += s.total_lost ?? 0;
        a.disputesWon += s.disputes_won ?? 0;
        a.disputesLost += s.disputes_lost ?? 0;
        a.bounties += s.bounties_completed ?? 0;
        a.timedOut += s.times_timed_out ?? 0;
        a.balanceSum += s.final_balance ?? 0;
        agg.set(label, a);
      }
    }
    return Array.from(agg.values())
      .map((a) => ({ ...a, net: a.earned - a.lost, avg: a.sessions ? Math.round(a.balanceSum / a.sessions) : 0 }))
      .sort((x, y) => y.net - x.net);
  }, [sessionHistory]);

  // Auto-scroll the Flow Feed to the newest line whenever rows change.
  useEffect(() => {
    if (feedCollapsed) return;
    const el = flowFeedRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [flowFeedRows, feedCollapsed]);

  const sectionDropdownRef = useRef<HTMLDivElement | null>(null);
  const [sectionLoading, setSectionLoading] = useState(false);
  const [savedProjects, setSavedProjects] = useState(() => loadSavedProjects());
  const [expandedProject, setExpandedProject] = useState<string | null>(null);
  const [projectSections, setProjectSections] = useState<Record<string, Section[]>>({});
  const workflowDropdownRef = useRef<HTMLDivElement>(null);
  const discussionModeRef = useRef<HTMLDivElement>(null);
  // Held in MutableRefObject form (not RefObject) so messagesEndCallbackRef
  // below can assign .current = el manually while React drives the lifecycle.
  const messagesEndRef = useRef<HTMLDivElement | null>(null);
  // Tracks which section we've already auto-scrolled-to-bottom for. Prevents
  // re-firing the initial scroll on every re-render while still firing once
  // per section-switch. Paired with messagesEndCallbackRef below.
  const initialScrollSectionRef = useRef<string | null>(null);
  const messageTimelineRef = useRef<HTMLDivElement>(null);
  const flowFeedRef = useRef<HTMLDivElement>(null);
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [newMsgCount, setNewMsgCount] = useState(0);
  const prevMsgCountRef = useRef(0);
  const savedScrollRef = useRef<number | null>(null);
  const scrollingToBottomRef = useRef(false);
  // Human msg 3194 initial-scroll-to-bottom ref declared down at line ~2454
  // alongside its useLayoutEffect (per-section tracking).
  const MSG_PAGE_SIZE = 100;
  const [visibleMsgLimit, setVisibleMsgLimit] = useState(MSG_PAGE_SIZE);

  // Team Launcher state
  const [launching, setLaunching] = useState(false);
  const [npmInstalled, setNpmInstalled] = useState<boolean | null>(null);
  const [claudeInstalled, setClaudeInstalled] = useState<boolean | null>(null);
  const [installingCli, setInstallingCli] = useState(false);
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
  const [roleFormStats, setRoleFormStats] = useState<RoleStats>(DEFAULT_STATS);
  const [roleFormAvatarUrl, setRoleFormAvatarUrl] = useState("");
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
    ? ["Name", "Description", "Capabilities", "Permissions", "Instances", "Stats", "Briefing"]
    : ["Template", "Name", "Description", "Capabilities", "Permissions", "Instances", "Stats", "Briefing"];

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
    setRoleFormStats(DEFAULT_STATS);
    setRoleFormAvatarUrl("");
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
    setRoleFormStats((role as any).stats || DEFAULT_STATS);
    setRoleFormAvatarUrl((role as any).avatar_url || "");
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
          stats: roleFormStats,
          avatarUrl: roleFormAvatarUrl,
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
          stats: roleFormStats,
          avatarUrl: roleFormAvatarUrl,
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
      const projectContext = project ? {
        roles: Object.fromEntries(
          Object.entries(project.config.roles).map(([slug, role]) => [slug, {
            title: role.title, description: role.description,
            tags: role.tags || [], permissions: role.permissions,
          }])
        ),
      } : { roles: {} };
      const apiUrl = (import.meta as any).env?.VITE_API_URL || "http://127.0.0.1:19836";
      // Fix per dev-chall #1189 + tech-leader #1191: backend's /roles/design
      // requires `Depends(get_current_user)` and rejects 401 if no Bearer
      // token. Send the auth token same shape as `lib/speak.ts` does.
      const token = getAuthToken();
      const headers: Record<string, string> = { "Content-Type": "application/json" };
      if (token) headers["Authorization"] = `Bearer ${token}`;
      const res = await fetch(`${apiUrl}/api/v1/roles/design`, {
        method: "POST",
        headers,
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
  }, [project?.config?.roles?.length]); // A6: coarsen deps — was [project], fired on every heartbeat tick

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

  // Human msg 3191: close section overflow dropdown on click outside.
  useEffect(() => {
    if (!sectionDropdownOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (sectionDropdownRef.current && !sectionDropdownRef.current.contains(e.target as Node)) {
        setSectionDropdownOpen(false);
        setSectionFilterText("");
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to create section:", e);
      showToast(`Couldn't create section — ${msg}`, "error");
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to switch section:", e);
      showToast(`Couldn't switch to section "${slug}" — ${msg}`, "error");
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

  // Poll Assembly Line state — current_speaker rotates on every accepted send,
  // so we re-read frequently enough to keep the indicator fresh.
  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    const pollAssembly = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const state = await invoke<{
          active: boolean;
          current_speaker: string | null;
          rotation_order: string[];
        }>("get_assembly_state", { dir: projectDir });
        if (!cancelled) setAssemblyState(state);
      } catch { /* command may not exist on older binaries */ }
    };
    pollAssembly();
    const interval = setInterval(pollAssembly, 1000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [projectDir]);

  // Human msg 870 — poll active oxford debate so the right-rail trigger can
  // swap between Start / End. Cheap file read; 2s cadence is plenty since
  // initiate + end are rare events.
  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    const pollActiveOxford = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<{
          debate_id: number;
          moderator: string;
          premise: string;
          side_a: string[];
          side_b: string[];
          current_speaker?: string | null;
          turn_history?: Array<unknown>;
          phase?: string;
          phase_started_at?: string | null;
        } | null>("read_active_oxford_cmd", { dir: projectDir });
        if (!cancelled) {
          if (result) {
            setActiveOxford({
              debate_id: result.debate_id,
              moderator: result.moderator,
              premise: result.premise,
              side_a: result.side_a,
              side_b: result.side_b,
              current_speaker: result.current_speaker ?? null,
              turn_count: Array.isArray(result.turn_history) ? result.turn_history.length : 0,
              phase: result.phase ?? "none",
              phase_started_at: result.phase_started_at ?? null,
            });
          } else {
            setActiveOxford(null);
          }
        }
      } catch { /* pre-fix binary — no End button surface */ }
    };
    pollActiveOxford();
    const interval = setInterval(pollActiveOxford, 2000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [projectDir]);

  // Human msg 1939 — poll active delphi discussion via dev:0 SHA-D10.2 Tauri
  // command `delphi_get_state_cmd` (main.rs:4358). Response is wrapped:
  // `{ active: bool, caller, caller_role, ...ActiveDelphiDebate fields }`.
  // When `active: false`, set state to null. When active, narrow into the
  // canonical ActiveDelphiDebate shape per spec §4.2. UI is always called as
  // human:0 so visibility = moderator_view (full state).
  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    const pollActiveDelphi = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<{
          active: boolean;
          caller?: string;
          caller_role?: string;
        } & Partial<ActiveDelphiDebate>>("delphi_get_state_cmd", {
          dir: projectDir,
          includeUnshuffle: true,
        });
        if (cancelled) return;
        if (result && result.active && result.discussion_id !== undefined) {
          setActiveDelphi(result as ActiveDelphiDebate);
        } else {
          setActiveDelphi(null);
        }
      } catch { /* pre-Delphi-SHA binary — poll inert; optimistic seed from modal remains */ }
    };
    pollActiveDelphi();
    const interval = setInterval(pollActiveDelphi, 2000);
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
            // Context setter persists to localStorage; the prior paired
            // setProjectDir + persistDir call collapses to one.
            setProjectDir(result.effective_dir);
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to launch team member:", e);
      showToast(`Couldn't launch ${role}:${instance} — ${msg}`, "error");
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to send interrupt:", e);
      showToast(`Couldn't send interrupt — ${msg}`, "error");
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to save group:", e);
      showToast(`Couldn't save group "${group.name}" — ${msg}`, "error");
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to delete group:", e);
      showToast(`Couldn't delete group "${slug}" — ${msg}`, "error");
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
      // Surface the error so the user knows the click was handled.
      // Previously this was console.error only — silent failure made the
      // View button feel broken (human msg 276 on 2026-05-13: "doesn't
      // even pop up its PowerShell window"). Common causes: PID/window
      // mismatch when the terminal is owned by Windows Terminal or
      // conhost rather than the spawned PowerShell, or the agent's
      // window was closed externally.
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to focus agent window:", e);
      showToast(`Couldn't focus ${slug}:${instance} — ${msg}`, "error");
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
          showToast(`Buzzed ${key} via terminal`, "success");
        } catch {
          // Terminal buzz failed (PID not found, window gone, etc.) — fall
          // back to board message. Distinct toast so the user knows which
          // path succeeded — they expect different reliability from each:
          // terminal buzz works even when MCP is dead; board message
          // requires the agent to poll the board.
          await invoke("send_team_message", {
            dir: projectDir,
            to: key,
            subject: "BUZZ",
            body: "Wake up — you may have lost your session. Rejoin and resume standby.",
            msgType: "buzz",
          });
          setBuzzedKey(key);
          showToast(`Buzzed ${key} via board message (terminal not reachable)`, "info");
        }
        setTimeout(() => setBuzzedKey(prev => prev === key ? null : prev), 1500);
      }
    } catch (e) {
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to buzz agent:", e);
      showToast(`Couldn't buzz ${key} — both terminal and board paths failed: ${msg}`, "error");
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to add roster slot:", e);
      showToast(`Couldn't add roster slot for "${role}" — ${msg}`, "error");
    }
  };

  const handleRemoveRosterSlot = async (role: string, instance: number) => {
    const seatLabel = `${role}:${instance}`;
    setConfirmAction({
      title: "Kick from project",
      message: `Kick ${seatLabel} from this project? Their seat will be released, their rotation slot cleared, and they will need to be re-invited to rejoin.`,
      confirmLabel: "Kick",
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
          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
          console.error("[CollabTab] Failed to remove roster slot:", e);
          showToast(`Couldn't remove ${role}:${instance} from roster — ${msg}`, "error");
        }
        setConfirmAction(null);
      },
    });
  };

  // Change D (human msg 5538 / msg 5237 directive 5): toggleAutoCollab +
  // toggleHumanInLoop handler functions removed alongside the deleted
  // header checkboxes. The setAutoCollab + setHumanInLoop setters still
  // run from the watch_project_dir effect (line ~1756) so the
  // autoCollab/humanInLoop state remains in sync with backend; just no
  // longer toggleable from the header.

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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to set workflow type:", e);
      showToast(`Couldn't set workflow type — ${msg}`, "error");
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to set discussion mode:", e);
      showToast(`Couldn't set discussion mode — ${msg}`, "error");
    }
  };

  const handleToggleAssembly = async () => {
    if (assemblyToggling || !projectDir) return;
    setAssemblyToggling(true);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        const action = assemblyState?.active ? "disable" : "enable";
        const next = await invoke<{
          active: boolean;
          current_speaker: string | null;
          rotation_order: string[];
        }>("set_assembly_state", { dir: projectDir, action });
        setAssemblyState(next);
      }
    } catch (e) {
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to toggle Assembly Line:", e);
      setError(msg || "Failed to toggle Assembly Line");
      showToast(`Couldn't toggle Assembly Line — ${msg}`, "error");
    } finally {
      setAssemblyToggling(false);
    }
  };

  // @ts-expect-error TS6133: retained for future ConsensusRow enrichment
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to close round:", e);
      showToast(`Couldn't close round — ${msg}`, "error");
    } finally {
      setClosingRound(false);
    }
  };

  // @ts-expect-error TS6133: retained for future ConsensusRow enrichment
  const handleEndDiscussion = async () => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("end_discussion", { dir: projectDir });
        setDiscussionState(null);
      }
    } catch (e) {
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to end discussion:", e);
      showToast(`Couldn't end discussion — ${msg}`, "error");
    }
  };

  // Change D: handleOpenStartDiscussion (header "Discuss" button trigger)
  // removed. The start-discussion modal (lines ~6055+) is preserved —
  // it remains reachable via the existing structured-discussion flow
  // (settings panel + future moderator-tab affordance per architect
  // forward-flag in commit msg).

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

  // @ts-expect-error TS6133: retained for future ConsensusRow enrichment
  const handleSetContinuousTimeout = async (seconds: number) => {
    setContinuousTimeout(seconds);
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_continuous_timeout", { dir: projectDir, timeoutSeconds: seconds });
      }
    } catch (e) {
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to set continuous timeout:", e);
      showToast(`Couldn't set continuous timeout — ${msg}`, "error");
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to send vote:", e);
      showToast(`Couldn't send vote — ${msg}`, "error");
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
          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
          console.error("[CollabTab] Failed to delete message:", e);
          showToast(`Couldn't delete message — ${msg}`, "error");
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
          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
          console.error("[CollabTab] Failed to clear messages:", e);
          showToast(`Couldn't clear messages — ${msg}`, "error");
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to set retention:", e);
      showToast(`Couldn't set retention — ${msg}`, "error");
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
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to send answer:", e);
      showToast(`Couldn't send answer — ${msg}`, "error");
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
      // Human msg 3330 perf fix: only grow visible limit if user is scrolled UP
      // (preserves their view of older revealed messages by extending the slice).
      // If user is at bottom (following the newest), DON'T grow — new messages
      // appear in the existing last-N slice; older slice contents slide out
      // naturally. Prevents visibleMsgLimit from creeping toward total count
      // over a long session (was hitting 3K → defeating pagination + render lag).
      if (!isAtBottom) {
        setVisibleMsgLimit((prev) => prev + added);
      }

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

  // Human msg 3194: on initial chat load (Vaak restart / session reload /
  // section switch), scroll to bottom — newest messages first — instead of
  // defaulting to top. Tracks a "has-scrolled-on-load" ref per section so
  // the auto-scroll fires ONCE per section-load, not on every re-render.
  //
  // Human msg 3231 follow-up: prior implementation used useLayoutEffect +
  // direct scrollTop = scrollHeight, which raced against the save-and-
  // restore useLayoutEffect at line 2400 that captures user scroll position
  // on each project update. The save/restore could capture scrollTop=0
  // during a transient pre-paint moment and restore 0 over our scrollHeight.
  // Switching to: rAF-deferred scrollIntoView on messagesEndRef, which
  // matches the existing smart-scroll pattern AND runs after the save/
  // restore effect has finished (and clears savedScrollRef so the next
  // listener save doesn't capture our 0→scrollHeight midpoint).
  // Human msg 3351 "still started at the top" — fix v3. Root cause of v2
  // failure: cleanup `cancelAnimationFrame(raf1)` fired on the very next
  // [project, activeSection] dep change (which happens on EVERY heartbeat
  // tick because project is a fresh object reference each update). The rAF
  // was cancelled before it fired, scroll never executed, then the next
  // effect call early-returned because the ref was already set.
  //
  // Fix: (a) depend on `messages?.length + activeSection` not the whole
  // project (so heartbeat-only updates don't re-trigger). (b) drop the rAF
  // cleanup entirely — scrollToBottom is idempotent + cheap; let the rAF
  // fire even if deps change. (c) set the ref AFTER the scroll completes,
  // not before — so a cancelled rAF can be retried by the next effect call.
  // Human msg 3404 — pivot to anchor pattern. Three iterations of rAF/
  // setTimeout/scrollHeight observation all failed live. The `messagesEndRef`
  // sentinel already exists at the bottom of the message list (line 4649);
  // browser's native scrollIntoView on it is the right primitive. Fires on
  // mount + section switch only. No project state observation, no rAF dance,
  // no scrollHeight measurement. Per evil-arch msg 3402 detection heuristic:
  // single lifecycle stage (browser native scroll-to-target).
  // Human msg 3568 "STILL STARTING AT TOP BTW" — iteration v5, callback-ref pivot.
  // Root cause of v4 anchor-pattern failure (tester msg 3573): on first mount,
  // project is null → message-timeline conditional render skips the children
  // that contain the messagesEndRef <div> → ref.current stays null when the
  // useEffect fires → optional chaining silently no-ops → user stays at top.
  // When project eventually loads and the ref populates, activeSection hasn't
  // changed → useEffect doesn't re-fire → never recovers.
  //
  // Callback-ref fires WHEN the DOM element becomes non-null, not when the
  // useEffect dep changes. This sidesteps the ref-mount-timing-vs-effect-fire
  // race entirely. initialScrollSectionRef tracks which section we've already
  // auto-scrolled for, so re-renders within the same section don't yank scroll
  // away from the user (preserves smart-scroll UX). Section-switch creates a
  // new callback identity (useCallback deps change) → React detaches old + re-
  // attaches new → callback fires with el → scroll fires.
  const messagesEndCallbackRef = useCallback((el: HTMLDivElement | null) => {
    messagesEndRef.current = el;
    if (el && initialScrollSectionRef.current !== activeSection) {
      el.scrollIntoView({ block: "end" });
      initialScrollSectionRef.current = activeSection;
      setIsAtBottom(true);
      setNewMsgCount(0);
    }
  }, [activeSection]);

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

        // Update projectDir if the backend found a better subdirectory.
        // Context setter persists to localStorage; collapses the prior
        // paired setProjectDir + persistDir into one call.
        const finalDir = (result?.effective_dir && result.effective_dir !== dir) ? result.effective_dir : dir;
        setProjectDir(finalDir);
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
    // Context setter's empty-string semantics remove the localStorage key.
    setProjectDir("");
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
          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
          console.error("[CollabTab] Failed to start discussion:", e);
          showToast(`Couldn't start discussion — ${msg}`, "error");
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
          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
          console.error("[CollabTab] Failed to end discussion:", e);
          showToast(`Couldn't end discussion — ${msg}`, "error");
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
          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
          console.error("[CollabTab] Failed to close round:", e);
          showToast(`Couldn't close round — ${msg}`, "error");
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
        // Slice 4 spec §10 atomicity: pass mic_to in metadata when the user
        // has explicitly confirmed the hint. Backend's project_send hook
        // (Slice 2 vaak-mcp.rs) calls apply_protocol_mic_to_transfer in the
        // SAME with_file_lock window — board append + floor move atomic.
        const metadata = micToConfirmed ? { mic_to: micToConfirmed } : undefined;
        await invoke("send_team_message", {
          dir: projectDir,
          to: msgTo,
          subject: "",
          body: trimmed,
          metadata,
        });
        setMsgBody("");
        setMicToConfirmed(null);
        setMicToHintDismissed(false);
      }
    } catch (e) {
      const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
      console.error("[CollabTab] Failed to send message:", e);
      showToast(`Couldn't send message — ${msg}`, "error");
    } finally {
      setSending(false);
    }
  };

  // Content-stable dep proxies per ui-architect:1 msg 4279 F-UI-A +
  // dev-challenger:0 msg 4292 audit-extension. Hoisted above ALL useMemos
  // that previously bare-keyed on `project?.X` (which is fresh ref per
  // heartbeat tick per CollabTab.tsx:2497 comment chain, defeating those
  // useMemo caches every 30s). Per board.jsonl append-only invariant +
  // sessions activeCount being a primitive count, these proxies are stable
  // across heartbeats unless content actually changes. Append-only edge
  // case (in-place mutation on non-tail message): impossible per write-side
  // audit; if backend ever introduces it, these proxies need to expand.
  const messagesLength = project?.messages?.length ?? 0;
  const lastMsgId = project?.messages?.[messagesLength - 1]?.id;
  const sessionsLength = project?.sessions?.length ?? 0;
  const sessionsActiveCount = project?.sessions?.filter((s) => s.status === "active").length ?? 0;

  // Compute the live mic_to candidate from msgBody. Pure function over the
  // current body + roster — re-runs each render. The candidate is the HINT
  // (spec §4.3); it does NOT auto-write metadata. The user must click confirm
  // (handled by MicToHint below).
  const micToCandidate = useMemo(() => {
    if (!msgBody.trim() || micToHintDismissed) return null;
    const seatList: SeatRef[] = (project?.sessions || []).map((s) => ({
      role: s.role,
      instance: s.instance,
      connected: s.status === "active",
    }));
    // selfSeat=null because human view; currentSpeaker is the ProtocolPanel
    // floor.current_speaker but we don't have it directly here. Use null —
    // self_target detection runs only when seat-bound, not the human-side UI.
    return detectMicTo(msgBody, seatList, null, null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
    // ^ F-UI-A class-of-bug per dev-challenger:0 msg 4292 audit:
    // bare `project?.sessions` is fresh ref per heartbeat → useMemo defeated.
    // Use sessionsLength + sessionsActiveCount proxy (declared below) instead.
  }, [msgBody, sessionsLength, sessionsActiveCount, micToHintDismissed]);

  // O(N²) → O(N): pre-build question→answer Map once per messages-reference change.
  // Wave 1.5c F-DC-AUDIT-2 fix per dev-challenger:0 msg 4292: keyed on
  // messagesLength + lastMsgId (content-stable per append-only invariant) instead
  // of `project?.messages` (fresh ref per heartbeat → cache no-op).
  const answerLookup = useMemo(
    () => buildAnswerLookup(project?.messages || []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [messagesLength, lastMsgId]
  );

  // Wave 1.5c F-DC-AUDIT-3 fix per dev-challenger:0 msg 4292: same proxy as
  // answerLookup. Without this, pendingQuestionCount re-walked all messages
  // every heartbeat (cascading from answerLookup invalidation).
  const pendingQuestionCount = useMemo(() => {
    if (!project) return 0;
    return project.messages.filter(
      (m) =>
        m.to === "human" &&
        m.type === "question" &&
        m.metadata?.choices?.length &&
        !getAnswerForQuestion(m.id, answerLookup)
    ).length;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [messagesLength, lastMsgId, answerLookup]);

  // Decision Panel v1 — pending count after dedup, used for window-title badge
  // (flag #5). The DecisionPanel itself owns the grouping logic; it bubbles up
  // the count via onPendingCountChange so the title updates in lockstep.
  const [pendingDecisionCount, setPendingDecisionCount] = useState(0);
  useEffect(() => {
    // Mutate window title so the "(N) Vaak" indicator is visible even when
    // the user is in another tab/app — per evil-arch flag #5 (msg 4789).
    // No cleanup return — F-DC-3 sister-fix from dev-challenger:0 msg 5006:
    // a cleanup that resets to "Vaak" caused a brief flicker on every
    // count-change because cleanup runs BEFORE the next effect sets the
    // new title. The next effect run already handles the count=0 case.
    document.title = pendingDecisionCount > 0
      ? `(${pendingDecisionCount}) Vaak`
      : "Vaak";
  }, [pendingDecisionCount]);

  // Wave 1.5 partial B1+B2+B3 per architect Ruling 7 + human msg 4264:
  // cache the IIFE's derived data (activeCount, voteTallies, voteProposalIds,
  // voteResponseIds, allMessages, totalCount, hasHiddenMessages, visibleMessages)
  // so per-render O(N²) walks + Set allocations only happen when underlying
  // data actually changes. Per-keystroke composer re-renders still re-execute
  // the IIFE BODY but skip the expensive computation. Full IIFE-to-useMemo lift
  // (~425 LOC of JSX) deferred to next focused work cycle per developer:1
  // msg 4272 context-budget honest disclosure.
  //
  // messageListDerivedCache uses the same content-stable proxies hoisted above
  // (messagesLength + lastMsgId + sessionsLength + sessionsActiveCount).
  const messageListDerivedCache = useMemo(() => {
    const sessions = project?.sessions || [];
    const messages = project?.messages || [];
    const activeCount = sessions.filter((s) => s.status === "active").length;
    const voteTallies = getActiveVotes(messages, activeCount);
    const voteProposalIds = new Set(voteTallies.map((t) => t.proposalId));
    const voteResponseIds = new Set(
      messages
        .filter((m) => m.type === "vote" && m.metadata?.in_reply_to)
        .map((m) => m.id)
    );
    const allMessages = messages;
    const totalCount = allMessages.length;
    const hasHiddenMessages = totalCount > visibleMsgLimit;
    const visibleMessages = hasHiddenMessages
      ? allMessages.slice(totalCount - visibleMsgLimit)
      : allMessages;
    return {
      activeCount,
      voteTallies,
      voteProposalIds,
      voteResponseIds,
      allMessages,
      totalCount,
      hasHiddenMessages,
      visibleMessages,
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
    // ^ deps are intentionally content-proxy (messagesLength + lastMsgId +
    // sessionsLength + sessionsActiveCount) not full project ref, to avoid
    // heartbeat-fresh-ref cache defeat per ui-architect:1 msg 4279 F-UI-A.
    // Linter would suggest [project?.messages, project?.sessions, visibleMsgLimit]
    // which IS the no-op pattern this fix addresses.
  }, [messagesLength, lastMsgId, sessionsLength, sessionsActiveCount, visibleMsgLimit]);

  // ===== WATCHING STATE: Project Dashboard =====
  if (watching) {
    const hasNoSessions = !project || project.sessions.length === 0;
    const hasNoMessages = !project || project.messages.length === 0;

    return (
      <div className="project-tab">
        {/* Phase 5 Surface 2 — Judge Seat (human msg 1872/1924). Priority banner
            at the very top when an open dispute has a judge assigned. Read-only:
            visualizes evidence + economics. Ruling enforcement (writing the
            verdict) is Phase 4 — buttons surface that boundary rather than write
            to .vaak/. */}
        {rulingDispute && (() => {
          const d = rulingDispute;
          const poolCopper = typeof d.pool === "number" ? d.pool : 0;
          const challengedMsg = project?.messages?.find((m) => String(m.id) === String(d.target_msg));
          const balOf = (label?: string) => (label ? currencyBalances.get(label) : undefined);
          const copper = (n?: number) => (typeof n === "number" ? n.toLocaleString() : "?");
          const ruleNotice = (verdict: string) =>
            showToast(`"${verdict}" ruling recorded in the UI only — verdict enforcement (pool payout + penalties) ships with Phase 4.`, "info");
          return (
            <div className={`judge-seat ${judgeExpanded ? "judge-seat--expanded" : ""}`}>
              <div className="judge-seat-banner" role="alert">
                <span className="judge-seat-flag">⚖ RULING REQUIRED</span>
                <span className="judge-seat-parties">{d.challenger || "?"} vs {d.target || "?"}</span>
                <span className="judge-seat-pool">Pool: {copper(poolCopper)} copper</span>
                <button
                  className="judge-seat-expand-btn"
                  onClick={() => setJudgeExpanded((v) => !v)}
                  aria-expanded={judgeExpanded}
                >
                  {judgeExpanded ? "Collapse" : "Expand to judge"}
                </button>
              </div>
              {judgeExpanded && (
                <div className="judge-seat-panel">
                  <div className="judge-seat-evidence">
                    <h4>Evidence</h4>
                    <div className="judge-evidence-block">
                      <div className="judge-evidence-label">Challenged message {d.target_msg != null ? `#${d.target_msg}` : ""}</div>
                      <div className="judge-evidence-text">{challengedMsg?.body ?? "(message not found in current board window)"}</div>
                    </div>
                    {d.objection_reason && (
                      <div className="judge-evidence-block">
                        <div className="judge-evidence-label">Objection reason</div>
                        <div className="judge-evidence-text">{d.objection_reason}</div>
                      </div>
                    )}
                    {Array.isArray(d.messages) && d.messages.length > 0 && (
                      <div className="judge-evidence-block">
                        <div className="judge-evidence-label">Dispute thread</div>
                        {d.messages.map((m, i) => (
                          <div key={i} className="judge-dispute-msg">
                            <span className="judge-dispute-msg-seat">{m.seat || "?"}</span>
                            <span className="judge-dispute-msg-text">{m.text}</span>
                          </div>
                        ))}
                      </div>
                    )}
                    {Array.isArray(d.edit_paths) && d.edit_paths.length > 0 && (
                      <div className="judge-evidence-block">
                        <div className="judge-evidence-label">
                          Edits ({d.edit_line_count != null ? `${d.edit_line_count} lines` : `${d.edit_paths.length} files`})
                        </div>
                        {d.edit_paths.map((p, i) => (
                          <div key={i} className="judge-evidence-path">{p}</div>
                        ))}
                      </div>
                    )}
                  </div>
                  <div className="judge-seat-economics">
                    <h4>Economics</h4>
                    <div className="judge-economics-block">
                      <div className="judge-evidence-label">Pool breakdown</div>
                      {Array.isArray(d.pool_breakdown) && d.pool_breakdown.length > 0 ? (
                        d.pool_breakdown.map((b, i) => (
                          <div key={i} className="judge-economics-row">
                            <span>{b.seat || "?"}{b.reason ? ` — ${b.reason}` : ""}</span>
                            <span className="judge-economics-amt">{copper(b.amount)} copper</span>
                          </div>
                        ))
                      ) : (
                        <div className="judge-economics-row">
                          <span>Total escrowed</span>
                          <span className="judge-economics-amt">{copper(poolCopper)} copper</span>
                        </div>
                      )}
                    </div>
                    <div className="judge-economics-block">
                      <div className="judge-evidence-label">Party balances</div>
                      <div className="judge-economics-row">
                        <span>{d.challenger || "challenger"}</span>
                        <span className="judge-economics-amt">{copper(balOf(d.challenger)?.balance_copper)} copper</span>
                      </div>
                      <div className="judge-economics-row">
                        <span>{d.target || "target"}</span>
                        <span className="judge-economics-amt">{copper(balOf(d.target)?.balance_copper)} copper</span>
                      </div>
                    </div>
                  </div>
                  <div className="judge-seat-rulings">
                    <button className="judge-ruling-btn judge-ruling-btn--challenger" onClick={() => ruleNotice("Challenger wins")}>
                      Challenger wins
                    </button>
                    <button className="judge-ruling-btn judge-ruling-btn--target" onClick={() => ruleNotice("Target wins")}>
                      Target wins
                    </button>
                    <button className="judge-ruling-btn judge-ruling-btn--both" onClick={() => ruleNotice("Both wrong")}>
                      Both wrong
                    </button>
                  </div>
                </div>
              )}
            </div>
          );
        })()}
        {/* Header */}
        <div className="project-header">
          <button className="project-back-btn" onClick={stopWatching} title="Back to projects">&larr;</button>
          <div className="project-header-info">
            <span className={`project-status-dot ${!hasNoSessions ? "active" : ""}`} />
            <span className="project-header-name">
              {project?.config?.name || "Project"}
            </span>
            {/* Change A (CollabTab restructure spec, architect msg 5238/5249/5259):
                Section selector relocated to header strip — placed adjacent to
                project name per human msg 5237 directive 2 ("section card of
                course should be probably at the top, near where the name of
                the project is"). Compact pill shows the active section name +
                chevron; click opens a dropdown listing all sections with
                filter input + "+ New section" affordance. Replaces the
                wider section-tabs strip that previously lived below the
                header — reclaims vertical space the human flagged as
                "way too much space." */}
            {project && (() => {
              const activeSec = sections.find(s => s.slug === activeSection);
              const activeName = activeSec?.name || activeSection || "default";
              const sortedSections = [...sections].sort((a, b) => {
                if (a.slug === activeSection) return -1;
                if (b.slug === activeSection) return 1;
                const aLA = a.last_activity ?? "";
                const bLA = b.last_activity ?? "";
                if (aLA && bLA) return bLA.localeCompare(aLA);
                if (aLA) return -1;
                if (bLA) return 1;
                return (b.created_at || "").localeCompare(a.created_at || "");
              });
              const filteredSections = sectionFilterText.trim()
                ? sortedSections.filter(s => s.name.toLowerCase().includes(sectionFilterText.toLowerCase()))
                : sortedSections;
              return (
                <div className="header-section-selector-wrap" ref={sectionDropdownRef}>
                  <button
                    type="button"
                    className={`header-section-selector${sectionDropdownOpen ? " header-section-selector-open" : ""}`}
                    onClick={() => setSectionDropdownOpen(v => !v)}
                    aria-haspopup="listbox"
                    aria-expanded={sectionDropdownOpen}
                    title={`Active section: #${activeName}. Click to switch.`}
                    disabled={sectionLoading}
                  >
                    <span className="header-section-selector-label">Section:</span>
                    <span className="header-section-selector-hash">#</span>
                    <span className="header-section-selector-name">{activeName}</span>
                    <span className="header-section-selector-caret" aria-hidden="true">{'▾'}</span>
                  </button>
                  {sectionDropdownOpen && (
                    <div className="header-section-dropdown" role="listbox">
                      <input
                        className="header-section-dropdown-filter"
                        type="text"
                        placeholder="Filter sections…"
                        value={sectionFilterText}
                        onChange={e => setSectionFilterText(e.target.value)}
                        autoFocus
                      />
                      <div className="header-section-dropdown-list">
                        {filteredSections.length === 0 ? (
                          <div className="header-section-dropdown-empty">No matching sections</div>
                        ) : filteredSections.map(s => (
                          <button
                            key={s.slug}
                            type="button"
                            className={`header-section-dropdown-item${s.slug === activeSection ? " header-section-dropdown-item-active" : ""}`}
                            role="option"
                            aria-selected={s.slug === activeSection}
                            onClick={() => {
                              handleSwitchSection(s.slug);
                              setSectionDropdownOpen(false);
                              setSectionFilterText("");
                            }}
                          >
                            <span className="header-section-dropdown-item-name">#{s.name}</span>
                            {s.message_count > 0 && (
                              <span className="header-section-dropdown-item-count">{s.message_count}</span>
                            )}
                            {s.last_activity && (
                              <span className="header-section-dropdown-item-time" title={s.last_activity}>
                                {formatRelativeTime(s.last_activity)}
                              </span>
                            )}
                          </button>
                        ))}
                      </div>
                      {creatingSectionMode ? (
                        <div className="header-section-dropdown-create">
                          <input
                            className="header-section-dropdown-create-input"
                            type="text"
                            placeholder="Name…"
                            value={newSectionName}
                            onChange={e => setNewSectionName(e.target.value)}
                            onKeyDown={e => {
                              if (e.key === "Enter") {
                                handleCreateSection();
                                setSectionDropdownOpen(false);
                              }
                              if (e.key === "Escape") {
                                setCreatingSectionMode(false);
                                setNewSectionName("");
                              }
                            }}
                            autoFocus
                          />
                          <button
                            type="button"
                            className="header-section-dropdown-create-ok"
                            onClick={() => {
                              handleCreateSection();
                              setSectionDropdownOpen(false);
                            }}
                            disabled={!newSectionName.trim() || sectionLoading}
                          >
                            {sectionLoading ? "…" : "+"}
                          </button>
                          <button
                            type="button"
                            className="header-section-dropdown-create-cancel"
                            onClick={() => {
                              setCreatingSectionMode(false);
                              setNewSectionName("");
                            }}
                          >&times;</button>
                        </div>
                      ) : (
                        <button
                          type="button"
                          className="header-section-dropdown-new"
                          onClick={() => setCreatingSectionMode(true)}
                        >+ New section</button>
                      )}
                    </div>
                  )}
                </div>
              );
            })()}
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
            {/* Folder icon — human msg 3067: replaced full-width "Watching:" row.
                Hover shows full path; click copies path to clipboard. */}
            {projectDir && (
              <button
                type="button"
                className="project-filepath-icon"
                title={`${projectDir}/.vaak/  (click to copy)`}
                aria-label={`Watching ${projectDir}/.vaak/. Click to copy path to clipboard.`}
                onClick={(e) => {
                  e.stopPropagation();
                  void navigator.clipboard.writeText(`${projectDir}/.vaak/`)
                    .then(() => showToast("Path copied to clipboard", "info"))
                    .catch(() => showToast("Couldn't copy path — see browser console", "error"));
                }}
              >
                <span aria-hidden="true">📁</span>
              </button>
            )}
          </div>
          {/* Change D (msg 5237 directive 5 + human msg 5538 choice A):
              Auto / Review checkboxes + Discuss button removed as
              "useless controls" per human direction. Auto-collab + human-
              in-loop modes can return as settings-panel toggles in a
              future commit if needed; the header strip is the wrong
              place for them per density-first framing in msg 5237.

              Directed/Open visibility-mode selector previously rendered
              here; relocated INTO the Discussion Mode sidebar card body
              per human msg 571 + ui-arch msg 729 — colocation so both
              controls live under one heading. The trailing badge on the
              collapsed Discussion Mode bar (dc677e8) preserves the
              at-a-glance state read. */}
          {/* Currency on/off toggle (human msg 1366) — sibling to the
              visibility-mode badge. Writes settings.currency_enabled via the
              set_currency_enabled Tauri command; the sidecar's
              record_currency_earn gate honors it (skips currency when off),
              and the roster-card coin pills hide when off. Reuses the
              discussion-mode-badge layout with currency-colored inline style.
              Default (absent) is ON — currency is opt-out. */}
          {projectDir && (() => {
            const currencyOn = project?.config?.settings?.currency_enabled !== false;
            const color = currencyOn ? "#f5c518" : "#8899a6";
            const toggleCurrency = async () => {
              const next = !currencyOn;
              try {
                const { invoke } = await import("@tauri-apps/api/core");
                await invoke("set_currency_enabled", { dir: projectDir, enabled: next });
                // BUGFIX (human msg 1506 "toggle does nothing"): re-fetch project so
                // the badge + pills reflect the change immediately. Mirrors
                // set_discussion_mode (line ~2384) — don't rely on the event listener,
                // which wasn't refreshing settings.currency_enabled in the UI.
                const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
                if (result) setProject(result);
                // Visible + screen-reader confirmation per tester:0 msg 1521 a11y note.
                showToast(`Currency mode ${next ? "ON" : "OFF"}`, "success");
              } catch (e) {
                const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                console.error("[currency] toggle failed:", e);
                showToast(`Couldn't toggle currency — ${msg}`, "error");
              }
            };
            return (
              // a11y per ui-architect:2 msg 1392: keyboard-operable + screen-reader
              // labeled (the human runs screen-reader mode). role=button + tabIndex
              // + aria-pressed + Enter/Space activation.
              <span
                className="discussion-mode-badge"
                role="button"
                tabIndex={0}
                aria-pressed={currencyOn}
                aria-label={`Currency economy ${currencyOn ? "on" : "off"}. Activate to turn ${currencyOn ? "off" : "on"}.`}
                style={{ background: `${color}22`, color, borderColor: `${color}55`, marginLeft: 6 }}
                onClick={() => { void toggleCurrency(); }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    void toggleCurrency();
                  }
                }}
                title={currencyOn
                  ? "Currency economy ON — click to turn OFF (hides coin pills; sidecar stops recording currency on the next message, no restart)"
                  : "Currency economy OFF — click to turn ON (takes effect on the next message)"}
              >
                🪙 {currencyOn ? "On" : "Off"}
              </span>
            );
          })()}
          {/* Change D: "Discuss" button removed per human msg 5538 choice A
              + msg 5237 directive 5. Discussion-format start can return as
              a settings-panel action or moderator-tab affordance if needed. */}
          {/* Assembly Line toggle — legacy v1.5.1 one-speaker-at-a-time mic
              control. B.3 Item 1 (per spec §16-37): hide when the new
              AssemblyControls card is rendering (twoControlsProtocol loaded).
              Legacy is the strict subset of AssemblyControls' capabilities;
              hiding rather than dual-wiring avoids state divergence on the
              write side. Falls through to legacy render only on truly
              pre-commit-A sections (no protocol.json yet). */}
          {!twoControlsProtocol && (
            <button
              className="assembly-line-toggle"
              onClick={handleToggleAssembly}
              disabled={assemblyToggling}
              title={
                assemblyState?.active
                  ? `Assembly Line ON · current speaker: ${assemblyState.current_speaker ?? "(none)"} · click to disable`
                  : "Assembly Line OFF (simultaneous) · click to enable one-speaker-at-a-time"
              }
              aria-label={assemblyState?.active ? "Disable Assembly Line" : "Enable Assembly Line"}
              style={{
                background: assemblyState?.active ? "#137333" : "transparent",
                color: assemblyState?.active ? "#fff" : "#137333",
                border: "1px solid #137333",
                borderRadius: 6,
                padding: "4px 10px",
                marginLeft: 6,
                fontSize: 12,
                fontWeight: 600,
                cursor: assemblyToggling ? "wait" : "pointer",
                opacity: assemblyToggling ? 0.6 : 1,
              }}
            >
              {assemblyState?.active
                ? `🎙 Assembly: ${assemblyState.current_speaker ?? "—"}`
                : "Assembly Line"}
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

        {/* Discussion Mode card — Change B per CollabTab-restructure-v1
            spec (architect msg 5238/5249/5259). Wraps the AssemblyControls
            two-controls surface in a CollapsibleSection so the human can
            fold the ~quarter-screen assembly UI for message-timeline
            real estate (human msg 5237 directive 4 "the assembly line
            thing right now takes up like one fourth of the screen").

            F-UIA-CTR-1: NO mode dropdown in v1 — title reads literally
            "Discussion Mode: Assembly Line" until a second mode (Oxford,
            etc.) actually ships. Avoids shipping a single-item dropdown
            that reads as broken UI.

            Sister-fix-CB2 (human msg 5447 "assembly line is still not
            collapsible"): the original Change B (6bbb2b9) wrapped only
            AssemblyControls — but ProtocolPanel renders the floor +
            consensus + assembly-state UI ALSO, and that was always
            visible. From the human's POV "assembly line" is the whole
            visible discussion-mode surface (AssemblyControls + ProtocolPanel
            combined). Sister-fix-CB2 extends the CollapsibleSection to
            wrap BOTH so the entire surface folds together. Also drops
            the outer `{twoControlsProtocol && ...}` gate so the band
            is ALWAYS visible (matches the F-UIA-CTR-4 Path A intent —
            band discoverable even when no mode is active; ProtocolPanel
            inside still handles its own no-protocol render). */}
        {(() => {
          // Phase 1b PROPER (architect msg 484 push for full restructure):
          // Discussion Mode band → thin always-visible STRIP + ⚙ popover.
          // The strip shows live preset/phase/plan in ~30px; Configure ⚙
          // opens the popover hosting AssemblyControls + ProtocolPanel.
          // Closes the msg-5450 discoverability lesson AND the human's
          // "500px of chrome" complaint simultaneously: state visible
          // always, controls reachable on demand.
          const livePreset = (twoControlsProtocol?.preset as string) ?? "Default chat";
          const livePhase = twoControlsProtocol?.floor?.phase as string | undefined;
          const livePlanPath = twoControlsProtocol?.floor?.plan_path as string | undefined;
          const phaseLabel = livePhase
            ? livePhase === "execution"
              ? " · Executing"
              : livePhase === "planning"
                ? " · Planning"
                : ""
            : "";
          const planLabel = livePlanPath
            ? ` · Plan: ${livePlanPath.replace(/^.*[\\/]/, "")}`
            : "";
          // P5-v2 (architect:0 msg 935 + human msg 932 = B):
          // The horizontal .discussion-mode-strip + DiscussionSettingsPopover
          // were removed in this commit. Discussion Mode settings (preset /
          // mic mode / moderator / review intensity / plan) now live in the
          // sidebar Discussion Mode card below via <AssemblyControls
          // layout="vertical">. ProtocolPanel (floor + consensus + 1-click
          // yield / force-release per evil-arch msg 509 + tech-leader msg
          // 511) stays inline here because those urgency actions need to be
          // reachable without opening the sidebar card. _livePreset, _phaseLabel,
          // _planLabel are referenced below for the sidebar card title chip.
          void livePreset; void phaseLabel; void planLabel;
          return (
            <ProtocolPanel
              projectDir={projectDir}
              section={activeSection || "default"}
              selfSeat={null /* this is the human's view; selfSeat = null */}
              rosterRoles={project?.config?.roles ? Object.keys(project.config.roles) : []}
              rolesConfig={project?.config?.roles}
            />
          );
        })()}

        {/* Settings Panel */}
        {settingsOpen && (
          <div className="project-settings-panel">
            {/* Currency mode — Settings-panel entry per human msg 1493 ("both":
                header badge + Settings entry). Reads/writes the SAME
                settings.currency_enabled as the header 🪙 badge (via the
                set_currency_enabled command shipped in 1148381), so the two
                surfaces stay in sync automatically. Honest label: this is the
                engine gate, not display-only. role=switch + aria-checked for
                screen-reader operability (human runs screen-reader mode). */}
            {projectDir && (() => {
              const currencyOn = project?.config?.settings?.currency_enabled !== false;
              const toggleCurrency = async () => {
                const next = !currencyOn;
                try {
                  const { invoke } = await import("@tauri-apps/api/core");
                  await invoke("set_currency_enabled", { dir: projectDir, enabled: next });
                  // BUGFIX (human msg 1506): re-fetch project so both surfaces update
                  // immediately. Mirrors set_discussion_mode line ~2384.
                  const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
                  if (result) setProject(result);
                  // Visible + screen-reader confirmation per tester:0 msg 1521.
                  showToast(`Currency mode ${next ? "ON" : "OFF"}`, "success");
                } catch (e) {
                  const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                  console.error("[currency] toggle failed:", e);
                  showToast(`Couldn't toggle currency — ${msg}`, "error");
                }
              };
              const color = currencyOn ? "#f5c518" : "#8899a6";
              return (
                <div className="settings-row">
                  <div>
                    <div className="settings-label-text">Currency mode</div>
                    <div className="settings-label-hint">Agents earn/spend copper for actions. Off hides the coin pills and stops the economy on the next message (no restart). Same toggle as the 🪙 badge in the header.</div>
                  </div>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={currencyOn}
                    aria-label={`Currency economy ${currencyOn ? "on" : "off"}. Activate to turn ${currencyOn ? "off" : "on"}.`}
                    onClick={() => { void toggleCurrency(); }}
                    style={{
                      background: `${color}22`,
                      color,
                      border: `1px solid ${color}55`,
                      borderRadius: 4,
                      padding: "4px 12px",
                      fontWeight: 600,
                      cursor: "pointer",
                    }}
                    title={currencyOn
                      ? "Currency economy ON — click to turn OFF (hides coin pills; stops recording on the next message, no restart)"
                      : "Currency economy OFF — click to turn ON (takes effect on the next message)"}
                  >
                    🪙 {currencyOn ? "On" : "Off"}
                  </button>
                </div>
              );
            })()}
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

        {/* File path — human msg 3067: replaced full-width row with compact
            folder icon. Click copies path to clipboard; hover shows full path
            in tooltip. Same data, ~24px footprint vs the prior ~600px row. */}

        {/* Section Tabs strip removed per Change A (architect msg 5238/5249/5259):
            section selector relocated to header strip near project name.
            See `.header-section-selector*` block above for the replacement
            UI. State + handlers (sectionDropdownOpen, sectionFilterText,
            handleSwitchSection, creatingSectionMode, newSectionName,
            handleCreateSection) are unchanged \u2014 only the render location
            and visual surface moved. */}

        {/* Section join hint removed per human msg 3069 — "Tell agents: join as
            [role], section X" was a noisy permanent line for info almost never
            needed. Agents discover section via project_join's section arg or
            project_status; no UI surface needed. */}

        {/* Discussion Status Panel — REMOVED per spec §1.1 (human #1062 "two UIs").
            ProtocolPanel's ConsensusRow renders the topic + phase + close button
            when consensus.round is active. The legacy panel block is gated below
            with `false &&` to keep the JSX compile-clean while we ship; future
            cleanup commit drops the dead JSX entirely once team verifies the
            replacement is sufficient. */}

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
                    style={{ display: 'none' }}
                    /* HIDDEN per human msg 237: duplicate of Team band's
                       "Manage Roles" tab; created layout-hierarchy inversion. */
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
                                  if (card.slug === "audience") {
                                    setAudiencePanelOpen(true);
                                  } else {
                                    matchingRole && setSelectedRole(matchingRole);
                                  }
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
                                        className="role-card-kick-btn"
                                        onClick={(e) => { e.stopPropagation(); handleRemoveRosterSlot(card.slug, card.instance >= 0 ? card.instance : 0); }}
                                        title={`Kick ${card.slug}:${card.instance >= 0 ? card.instance : 0} from this project`}
                                        aria-label={`Kick ${card.slug}:${card.instance >= 0 ? card.instance : 0} from this project`}
                                      >Kick</button>
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
                        try { await invoke("open_terminal_in_dir", { dir: projectDir || "" }); } catch (e) {
                          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                          showToast(`Couldn't open terminal for login — ${msg}. Try manually: claude login`, "error");
                        }
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
                        onClick={async () => {
                          try {
                            if (window.__TAURI__) {
                              const { invoke } = await import("@tauri-apps/api/core");
                              await invoke("open_url_in_browser", { url: "https://nodejs.org" });
                              return;
                            }
                          } catch (e) {
                            const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                            console.error("[CollabTab] Failed to open URL via Tauri:", e);
                            showToast(`Couldn't open URL via Tauri (${msg}); falling back to browser`, "warning");
                          }
                          // Fallback: try window.open
                          window.open("https://nodejs.org", "_blank");
                        }}
                      >Download Node.js</button>
                      <button
                        className="setup-action-btn"
                        onClick={async () => {
                          try {
                            const { invoke } = await import("@tauri-apps/api/core");
                            const installed = await invoke<boolean>("check_npm_installed");
                            setNpmInstalled(installed);
                            if (!installed) {
                              showToast("Node.js/npm not detected yet. Install Node.js, then click Re-check.", "warning");
                            }
                          } catch (e) {
                            const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                            showToast(`Couldn't re-check Node.js install — ${msg}`, "error");
                          }
                        }}
                      >Re-check</button>
                      <span className="setup-check-hint">Required for Claude Code CLI installation</span>
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
                              showToast("Node.js/npm is not installed. Complete Step 1 first.", "warning");
                              return;
                            }
                            await invoke<string>("install_claude_cli");
                            const installed = await invoke<boolean>("check_claude_installed");
                            if (installed) {
                              setClaudeInstalled(true);
                            } else {
                              setInstallingCli(false);
                              showToast("Installation completed but Claude CLI was not found. Try restarting the app.", "warning");
                            }
                          } catch (e: any) {
                            setInstallingCli(false);
                            showToast(`Install failed: ${e?.message || e}. Try manually: npm install -g @anthropic-ai/claude-code`, "error");
                          }
                        }}
                      >{installingCli ? "Installing..." : "Express Install"}</button>
                      <button
                        className="setup-action-btn"
                        onClick={async () => {
                          try {
                            const { invoke } = await import("@tauri-apps/api/core");
                            await invoke("open_terminal_in_dir", { dir: projectDir || "" });
                          } catch (e) {
                            const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                            showToast(`Couldn't open terminal — ${msg}`, "error");
                          }
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
                          } catch (e) {
                            const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                            showToast(`Couldn't open terminal — ${msg}. Try manually: open a terminal and run \`claude\``, "error");
                          }
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
                              showToast("API key not detected in environment variables. If you logged in via OAuth, you may still be able to launch agents — try launching one to test.", "info");
                            }
                          } catch (e) {
                            const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                            showToast(`Couldn't re-check API key — ${msg}`, "error");
                          }
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
          // Track D v1 (per human msg 88) — assembly state from the section
          // protocol drives roster visuals + sort. When assembly_active is true,
          // cards in rotation_order are pinned to the front in rotation order;
          // remaining cards fall through to the existing status+role+title sort
          // beneath them. This is the visible-merge half of Track D — drag-
          // rearrange (Track D v2) requires the typed set_rotation_order
          // backend action which is still architect/dev lane.
          const assemblyActiveForRoster = twoControlsProtocol?.floor?.assembly_active === true;
          const rotationOrderArr: string[] = Array.isArray(twoControlsProtocol?.floor?.rotation_order)
            ? (twoControlsProtocol!.floor!.rotation_order as string[])
            : [];
          const currentSpeakerSeat: string | null = twoControlsProtocol?.floor?.current_speaker ?? null;
          const moderatorSeat: string | null = twoControlsProtocol?.floor?.moderator ?? null;
          const rotationIndexMap = new Map<string, number>();
          rotationOrderArr.forEach((seat, idx) => rotationIndexMap.set(seat, idx));
          // Sort: when assembly active, rotation_order entries pin to the front
          // in rotation order; otherwise the historical sort (status + role
          // priority + title alphabetic) is preserved unchanged.
          const statusOrder: Record<string, number> = { working: 0, active: 0, stale: 1, vacant: 2 };
          // Tier 0.0a 💤 drowsy indicator — last broadcast timestamp per seat,
          // derived from project.messages tail. Detects MW10 lockstep-fresh-
          // dead-agent (the overnight 2026-05-26→27 silent-team-death pattern)
          // that the ⚠ MW6 drift indicator does NOT catch. Per architect msg
          // 1754 urgent add-on to Tier 0.0.
          const lastBroadcastBySeat = new Map<string, number>();
          project.messages.forEach((m) => {
            if (m.from && m.from !== "system" && !m.from.startsWith("system:")) {
              const ts = new Date(m.timestamp).getTime();
              const prev = lastBroadcastBySeat.get(m.from);
              if (!prev || ts > prev) lastBroadcastBySeat.set(m.from, ts);
            }
          });
          const sortedCards = [...filteredCards].sort((a, b) => {
            if (assemblyActiveForRoster) {
              const ai = rotationIndexMap.get(`${a.slug}:${a.instance}`);
              const bi = rotationIndexMap.get(`${b.slug}:${b.instance}`);
              if (ai !== undefined && bi !== undefined) return ai - bi;
              if (ai !== undefined) return -1;
              if (bi !== undefined) return 1;
            }
            const sa = statusOrder[a.status] ?? 2;
            const sb = statusOrder[b.status] ?? 2;
            if (sa !== sb) return sa - sb;
            const oa = ROLE_ORDER[a.slug] ?? 99;
            const ob = ROLE_ORDER[b.slug] ?? 99;
            if (oa !== ob) return oa - ob;
            return a.title.localeCompare(b.title);
          });
          const vacantCount = sortedCards.filter(c => c.status === "vacant").length;
          // Layout-density-v1 corrected (collapse-not-filter per human msg 5125
          // + ui-arch:1 msg 5128 revised spec): one-line section header always
          // present + click to fold/unfold the full grid. NO filtering — every
          // role stays reachable on expand.
          const workingCount = sortedCards.filter(c => c.status === "working").length;
          const readyCount = sortedCards.filter(c => c.status === "ready").length;
          return (
            <>
              {sortedCards.length > 0 && (
              <CollapsibleSection
                id="roster-section"
                title="Team"
                trailing={
                  <>
                    {workingCount > 0 && <span className="roster-count-active">{workingCount} working</span>}
                    {readyCount > 0 && (
                      <span className="roster-count-stale">
                        {workingCount > 0 ? " · " : ""}{readyCount} ready
                      </span>
                    )}
                    {vacantCount > 0 && (
                      <span className="roster-count-vacant">
                        {(workingCount + readyCount) > 0 ? " · " : ""}{vacantCount} vacant
                      </span>
                    )}
                    {assemblyActiveForRoster && (
                      <span className="roster-assembly-indicator" aria-hidden="true">
                        {" · 🎙 assembly"}
                      </span>
                    )}
                    {/* Phase 5 Surface 3 — Balance Bar (human msg 1872/1924).
                        Running "copper in play" total appended to the team
                        summary; click toggles a mini-leaderboard inside the
                        section body. stopPropagation so it doesn't collapse the
                        section. Hidden until currency data exists. */}
                    {currencyLeaderboard.seats.length > 0 && (
                      <button
                        type="button"
                        className="balance-bar-summary"
                        aria-expanded={balanceExpanded}
                        title={balanceExpanded ? "Hide balances" : "Show balances"}
                        onClick={(e) => { e.stopPropagation(); setBalanceExpanded((v) => !v); }}
                      >
                        {" · "}{currencyLeaderboard.total.toLocaleString()} copper in play
                      </button>
                    )}
                  </>
                }
                collapsed={rosterSectionCollapsed}
                onToggle={() => updateRosterSectionCollapsed(!rosterSectionCollapsed)}
                className="roster-section"
                headerTooltip={{ expand: "Expand team section", collapse: "Collapse team section" }}
              >
                {/* Change C: Team Section tab strip per architect msg 5238/5249/5259
                    F-UIA-CTR-2 (Team Roster first / default-active; Manage Roles
                    second). Manage Roles tab embeds the standalone RolesTab
                    component for in-collab role editing without leaving the
                    Collab tab; the top-level Tauri RolesTab tab REMAINS available
                    per F-DC-CTR-2 (dual-path preserved; no removal). Both mounts
                    share `useProjectDir()` from pre-req 8162d3f so the
                    divergent-WRITER class on `vaak_collab_project_dir` (F-EA-CTR-A)
                    stays closed. */}
                {/* Phase 5 Surface 3 — Balance Bar mini-leaderboard. Per-seat
                    horizontal bar relative to the 10,000-copper start; red zone
                    when balance < 0. Read-only. */}
                {balanceExpanded && currencyLeaderboard.seats.length > 0 && (
                  <div className="balance-leaderboard" role="table" aria-label="Currency leaderboard">
                    {currencyLeaderboard.seats.map((s) => {
                      const net = s.balance + s.escrow;
                      const pct = Math.max(0, Math.min(100, (net / 10000) * 100));
                      const negative = s.balance < 0;
                      return (
                        <div key={s.label} className="balance-leaderboard-row" role="row">
                          <span className="balance-leaderboard-dot" style={{ background: getRoleColor(s.label.split(":")[0]) }} aria-hidden="true" />
                          <span className="balance-leaderboard-label">{s.label}</span>
                          <span className="balance-leaderboard-track">
                            <span
                              className={`balance-leaderboard-fill${negative ? " balance-leaderboard-fill--negative" : ""}`}
                              style={{ width: `${pct}%` }}
                            />
                          </span>
                          <span className={`balance-leaderboard-amt${negative ? " balance-leaderboard-amt--negative" : ""}`}>
                            {s.balance.toLocaleString()}{s.escrow > 0 ? ` (+${s.escrow.toLocaleString()} held)` : ""}
                          </span>
                        </div>
                      );
                    })}
                  </div>
                )}
                <div
                  className="team-section-tabs"
                  role="tablist"
                  aria-label="Team section view"
                >
                  <button
                    type="button"
                    role="tab"
                    id="team-tab-roster"
                    aria-controls="team-pane-roster"
                    aria-selected={activeTeamTab === "roster"}
                    tabIndex={activeTeamTab === "roster" ? 0 : -1}
                    className={`team-section-tab${activeTeamTab === "roster" ? " team-section-tab-active" : ""}`}
                    onClick={() => updateActiveTeamTab("roster")}
                    onKeyDown={(e) => {
                      if (e.key === "ArrowRight" || e.key === "ArrowDown") {
                        e.preventDefault();
                        updateActiveTeamTab("roles");
                        document.getElementById("team-tab-roles")?.focus();
                      }
                    }}
                  >
                    Team Roster
                  </button>
                  <button
                    type="button"
                    role="tab"
                    id="team-tab-roles"
                    aria-controls="team-pane-roles"
                    aria-selected={activeTeamTab === "roles"}
                    tabIndex={activeTeamTab === "roles" ? 0 : -1}
                    className={`team-section-tab${activeTeamTab === "roles" ? " team-section-tab-active" : ""}`}
                    onClick={() => updateActiveTeamTab("roles")}
                    onKeyDown={(e) => {
                      if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
                        e.preventDefault();
                        updateActiveTeamTab("roster");
                        document.getElementById("team-tab-roster")?.focus();
                      }
                    }}
                  >
                    Manage Roles
                  </button>
                </div>
                {activeTeamTab === "roles" && (
                  <div
                    id="team-pane-roles"
                    role="tabpanel"
                    aria-labelledby="team-tab-roles"
                    className="team-section-roles-pane"
                  >
                    {/* Per human msg 671: Manage Roles needs an "Add to Roster"
                        affordance INSIDE the pane — not buried in the
                        separate collapsed "Add to Team" section. This is a
                        compact quick-add toolbar that mirrors handleAddRosterSlot
                        and openCreateRoleForm from the existing Add Team flow,
                        but in-place at the top of Manage Roles. */}
                    {project && (
                      <div className="manage-roles-add-bar">
                        <div className="manage-roles-add-bar-label">
                          Add to roster:
                        </div>
                        <div className="manage-roles-add-bar-buttons">
                          {Object.keys(project.config.roles)
                            .sort((a, b) => (ROLE_ORDER[a] ?? 99) - (ROLE_ORDER[b] ?? 99))
                            .map((slug) => {
                              const role = project.config.roles[slug];
                              if (!role) return null;
                              const slotsForRole = (project.config.roster || []).filter(
                                (s: RosterSlot) => s.role === slug
                              ).length;
                              return (
                                <button
                                  key={slug}
                                  className="manage-roles-add-btn"
                                  onClick={() => handleAddRosterSlot(slug)}
                                  title={`Add ${role.title} to roster`}
                                  aria-label={`Add ${role.title} to roster, ${slotsForRole} currently on team`}
                                >
                                  <span
                                    className="manage-roles-add-btn-dot"
                                    style={{ background: getRoleColor(slug) }}
                                  />
                                  <span className="manage-roles-add-btn-label">
                                    + {role.title}
                                  </span>
                                  {slotsForRole > 0 && (
                                    <span className="manage-roles-add-btn-count">
                                      {slotsForRole}
                                    </span>
                                  )}
                                </button>
                              );
                            })}
                          <button
                            className="manage-roles-add-btn manage-roles-add-btn-create"
                            onClick={openCreateRoleForm}
                            title="Create a new custom role"
                            aria-label="Create a new custom role"
                          >
                            + New Role
                          </button>
                        </div>
                      </div>
                    )}
                    <RolesTab />
                  </div>
                )}
                {activeTeamTab === "roster" && (
                <>
                <div
                  id="team-pane-roster"
                  role="tabpanel"
                  aria-labelledby="team-tab-roster"
                  className={`project-roles-grid${rosterViewMode === "list" ? " project-roles-list" : ""}${rosterViewMode === "chip" ? " project-roles-chips" : ""}${assemblyActiveForRoster ? " project-roles-compact" : ""}`}>
                  {sortedCards.map((card) => {
                    const cardKey = `${card.slug}:${card.instance}`;
                    const matchingRole = project.role_statuses.find((r) => r.slug === card.slug);
                    // Keepalive v3 — overlay alive_state from list_active_seats_cmd
                    // on top of the existing card.status. Vacant cards have no
                    // seat to be alive, so skip them. Stale → amber ring + suffix;
                    // unknown → gray-dashed dot. "active" is the no-op default.
                    const seatAliveState = card.status === "vacant"
                      ? undefined
                      : seatAliveMap.get(cardKey);
                    const isSeatStale = seatAliveState === "stale";
                    const isSeatUnknown = seatAliveState === "unknown";
                    // Tier 0.0 MW6/MW6.1 cross-tracker divergence indicator.
                    // Surfaces MW6 corruption only — does NOT detect MW10 lockstep-
                    // fresh-dead-agent. Fires when per-seat file says stale but
                    // sessions.json-derived status says active.
                    const isMW6Divergent = isSeatStale && (card.status === "working" || card.status === "ready");
                    // Tier 0.0a 💤 drowsy indicator — MW10 lockstep-fresh detector.
                    // Fires when both trackers can be fresh (sidecar keepalive
                    // firing) but the agent loop hasn't broadcast in >5min. Per
                    // architect msg 1754. Excludes vacant + never-broadcast cases.
                    const lastBroadcastMs = lastBroadcastBySeat.get(cardKey);
                    const isDrowsy = (card.status === "working" || card.status === "ready")
                      && lastBroadcastMs !== undefined
                      && (Date.now() - lastBroadcastMs > 5 * 60 * 1000);
                    const aliveSuffix = isSeatStale
                      ? " (reconnecting…)"
                      : isSeatUnknown
                        ? " (joining…)"
                        : "";
                    // Track D v1 (per human msg 88) — assembly-mode visuals.
                    // Mic-holder gets an accent border + 🎙 glyph; moderator gets
                    // a gold ★. Rotation index (1)(2)(3)… on cards in
                    // rotation_order. All three are no-ops when assembly is
                    // inactive — the card renders identical to pre-Track-D.
                    const isMicHolder = assemblyActiveForRoster
                      && currentSpeakerSeat !== null
                      && cardKey === currentSpeakerSeat;
                    const isModeratorCard = assemblyActiveForRoster
                      && moderatorSeat !== null
                      && cardKey === moderatorSeat;
                    const rotationIdx = assemblyActiveForRoster
                      ? rotationIndexMap.get(cardKey) ?? -1
                      : -1;
                    const handleCardClick = () => {
                      if (card.slug === "audience") {
                        setAudiencePanelOpen(true);
                        if (audiencePersonas.length === 0) { fetchAudiencePersonas(); fetchAudiencePools(); }
                      } else {
                        matchingRole && setSelectedRole(matchingRole);
                      }
                    };
                    const handleCardKeyDown = (e: React.KeyboardEvent) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        handleCardClick();
                      }
                    };

                    // Compact chip view
                    if (rosterViewMode === "chip") {
                      return (
                        <button
                          key={cardKey}
                          className={`role-chip${card.status === "working" ? " role-chip-working" : ""}${card.status === "ready" ? " role-chip-ready" : ""}${card.status === "vacant" ? " role-chip-vacant" : ""}${isSeatStale ? " role-chip-alive-stale" : ""}${isSeatUnknown ? " role-chip-alive-unknown" : ""}${isMicHolder ? " role-chip-mic-holder" : ""}${isModeratorCard ? " role-chip-moderator" : ""}`}
                          style={{ borderColor: isMicHolder ? card.roleColor : card.roleColor + "40", color: card.roleColor }}
                          onClick={handleCardClick}
                          title={`${isMicHolder ? "🎙 has the floor — " : ""}${isModeratorCard ? "★ moderator — " : ""}${card.title}${aliveSuffix} — ${getStatusLabel(card.status)}${card.instance > 0 ? ` (instance ${card.instance})` : ""}${rotationIdx >= 0 ? ` (rotation ${rotationIdx + 1})` : ""}. Click for details.`}
                          aria-label={`${isMicHolder ? "Has the floor. " : ""}${isModeratorCard ? "Moderator. " : ""}${card.title}${aliveSuffix}, status: ${getStatusLabel(card.status)}${card.instance > 0 ? `, instance ${card.instance}` : ""}${rotationIdx >= 0 ? `, rotation position ${rotationIdx + 1}` : ""}. Press Enter for details and actions.`}
                          data-mic-holder={isMicHolder ? "true" : undefined}
                          data-moderator={isModeratorCard ? "true" : undefined}
                          data-rotation-idx={rotationIdx >= 0 ? rotationIdx : undefined}
                        >
                          {isMicHolder && <span className="role-chip-mic-glyph" aria-hidden="true">🎙</span>}
                          {isModeratorCard && <span className="role-chip-moderator-glyph" aria-hidden="true">★</span>}
                          {isMW6Divergent && <span className="role-chip-mw6-divergent-glyph" aria-hidden="true" title="MW6/MW6.1 cross-tracker drift (sessions.json active but per-seat file stale). Does NOT detect MW10 lockstep-fresh-dead-agent.">⚠</span>}
                          {isDrowsy && <span className="role-chip-drowsy-glyph" aria-hidden="true" title={`Agent silent — no broadcast in ${Math.floor((Date.now() - (lastBroadcastMs ?? 0)) / 60000)}min. MW10 lockstep-fresh signature: sidecar may be alive while agent loop is dead.`}>💤</span>}
                          <span className={`${getStatusDotClass(card.status)}${isSeatStale ? " alive-stale" : ""}${isSeatUnknown ? " alive-unknown" : ""}`} />
                          <span className="role-chip-name">{card.title}{aliveSuffix}</span>
                          {rotationIdx >= 0 && !isMicHolder && !isModeratorCard && (
                            <span className="role-chip-rotation-idx" aria-hidden="true">{rotationIdx + 1}</span>
                          )}
                          <span className={`role-chip-status role-card-status-${card.status}`}>{getStatusLabel(card.status)}</span>
                        </button>
                      );
                    }

                    return (
                      <div
                        key={cardKey}
                        className={`role-card role-card-clickable ${card.status === "working" ? "role-card-working" : ""} ${card.status === "vacant" ? "role-card-vacant" : ""}${isSeatStale ? " role-card-alive-stale" : ""}${isSeatUnknown ? " role-card-alive-unknown" : ""}${isMicHolder ? " role-card-mic-holder" : ""}${isModeratorCard ? " role-card-moderator" : ""}`}
                        style={{ borderLeftColor: card.roleColor }}
                        role="button"
                        tabIndex={0}
                        aria-label={`${isMicHolder ? "Has the floor. " : ""}${isModeratorCard ? "Moderator. " : ""}${card.title}${aliveSuffix}, status: ${getStatusLabel(card.status)}${rotationIdx >= 0 ? `, rotation position ${rotationIdx + 1}` : ""}. Click to view details.`}
                        data-mic-holder={isMicHolder ? "true" : undefined}
                        data-moderator={isModeratorCard ? "true" : undefined}
                        data-rotation-idx={rotationIdx >= 0 ? rotationIdx : undefined}
                        onClick={handleCardClick}
                        onKeyDown={handleCardKeyDown}
                      >
                        {/* Track D v1 — mic-holder + moderator glyphs in top-
                            right corner. Absolute-positioned via CSS so they
                            don't disrupt the existing header layout. Rotation
                            index uses the existing roleColor for visual link
                            to the rest of the card. */}
                        {isMicHolder && (
                          <span className="role-card-mic-glyph" aria-hidden="true" title={`${card.title} has the floor`}>🎙</span>
                        )}
                        {isModeratorCard && (
                          <span className="role-card-moderator-glyph" aria-hidden="true" title={`${card.title} is moderator`}>★</span>
                        )}
                        {isMW6Divergent && (
                          <span className="role-card-mw6-divergent-glyph" aria-hidden="true" title={`MW6/MW6.1 cross-tracker drift: sessions.json says active but per-seat file says stale. Does NOT detect lockstep-fresh-dead-agent (MW10) — both trackers can be fresh while agent is silent. For MW10, await tri-dot UR4 post-chain.`}>⚠</span>
                        )}
                        {isDrowsy && (
                          <span className="role-card-drowsy-glyph" aria-hidden="true" title={`Agent silent: no broadcast in ${Math.floor((Date.now() - (lastBroadcastMs ?? 0)) / 60000)} minutes (last broadcast ${new Date(lastBroadcastMs ?? 0).toLocaleTimeString()}). MW10 lockstep-fresh signature — sidecar keepalive may be firing while agent loop is dead. Detection only; for fix, await Mit 1+2+3 chain ship.`}>💤</span>
                        )}
                        {rotationIdx >= 0 && !isMicHolder && !isModeratorCard && (
                          <span className="role-card-rotation-idx" aria-hidden="true" title={`Rotation position ${rotationIdx + 1}`} style={{ color: card.roleColor }}>{rotationIdx + 1}</span>
                        )}
                        <div className="role-card-header">
                          <Avatar
                            slug={card.slug}
                            title={card.title}
                            // Vacant cards have no live instance → pass undefined
                            // so Avatar uses the role-definition alt-text branch
                            // ("Manager (manager) avatar") instead of falsely
                            // claiming ":0" per dev-challenger:0 msg 4663 +
                            // evil-architect:0 msg 4665 accessibility regression.
                            instance={card.instance >= 0 ? card.instance : undefined}
                            avatarUrl={project.config.roles[card.slug]?.avatar_url || null}
                            sizePx={28}
                            className="role-card-avatar-collab"
                          />
                          <span
                            className={`${getStatusDotClass(card.status)}${isSeatStale ? " alive-stale" : ""}${isSeatUnknown ? " alive-unknown" : ""}`}
                            title={isSeatStale
                              ? `${cardKey} — last keepalive >120s stale; seat may be reconnecting or dead`
                              : isSeatUnknown
                                ? `${cardKey} — keepalive not yet observed; seat may be just-joined or pre-instrumentation`
                                : undefined}
                          />
                          <span className="role-card-title" style={{ color: card.roleColor }}>
                            {card.title}{aliveSuffix}
                          </span>
                          <button
                            className="role-card-kick-btn"
                            onClick={(e) => { e.stopPropagation(); handleRemoveRosterSlot(card.slug, card.instance >= 0 ? card.instance : 0); }}
                            title={`Kick ${card.slug}:${card.instance >= 0 ? card.instance : 0} from this project`}
                            aria-label={`Kick ${card.slug}:${card.instance >= 0 ? card.instance : 0} from this project`}
                          >Kick</button>
                        </div>
                        <div className="role-card-meta">
                          <span className="role-card-slug">{card.slug}</span>
                          <span className={`role-card-status role-card-status-${card.status}`}>
                            {getStatusLabel(card.status)}
                          </span>
                        </div>
                        {/* Currency balance pill (Phase 1 UI, human msg 1300).
                            Functional baseline — ui-architect to polish styling.
                            Shows gold/silver/copper for active seats; escrow held
                            shown with a lock glyph. Hidden for vacant cards. */}
                        {(() => {
                          if (card.status === "vacant") return null;
                          // Currency on/off toggle (human msg 1366): when currency
                          // is disabled in settings, hide pills entirely.
                          if (project?.config?.settings?.currency_enabled === false) return null;
                          const bal = currencyBalances.get(cardKey);
                          if (!bal) return null;
                          const d = bal.display;
                          // dev-challenger:0 msg 1325 BLOCKING guardrail: a seat
                          // with no ledger entry yet (initialized:false) shows the
                          // 10,000 starting DEFAULT, not a real balance. Render it
                          // visibly distinct (dimmed + "starting" marker) so the
                          // human never reads a phantom balance as confirmed. Only
                          // initialized:true pills show as live numbers.
                          return (
                            <div
                              className={`role-card-currency${bal.timed_out ? " role-card-currency-timedout" : ""}${bal.initialized ? "" : " role-card-currency-pending"}`}
                              title={
                                bal.initialized
                                  ? `${bal.balance_copper.toLocaleString()} copper settled${bal.escrow_held_copper > 0 ? ` · ${bal.escrow_held_copper} held in escrow` : ""}${bal.timed_out ? " · TIMED OUT (deficit cap)" : ""}`
                                  : `Starting balance (${bal.balance_copper.toLocaleString()} copper) — this seat has no recorded currency activity yet; not a confirmed ledger balance.`
                              }
                            >
                              {d.gold > 0 && (
                                <span className="rc-cur rc-cur-gold">
                                  <span className="coin-icon coin-icon-gold" aria-hidden="true" />
                                  {d.gold}
                                </span>
                              )}
                              {(d.gold > 0 || d.silver > 0) && (
                                <span className="rc-cur rc-cur-silver">
                                  <span className="coin-icon coin-icon-silver" aria-hidden="true" />
                                  {d.silver}
                                </span>
                              )}
                              <span className="rc-cur rc-cur-copper">
                                <span className="coin-icon coin-icon-copper" aria-hidden="true" />
                                {d.copper}
                              </span>
                              {bal.initialized && bal.escrow_held_copper > 0 && (
                                <span className="rc-cur rc-cur-escrow" title={`${bal.escrow_held_copper} copper held in escrow`}>
                                  +{bal.escrow_held_copper}🔒
                                </span>
                              )}
                              {!bal.initialized && (
                                <span className="rc-cur rc-cur-pending-tag" aria-label="starting balance, no activity yet">starting</span>
                              )}
                              {/* Human msg 458: +/- buttons for direct balance adjustment.
                                  Always visible in Vaak UI because the human is the
                                  only user of this surface; backend gates calls to
                                  human:0 regardless. v1 uses window.prompt for amount
                                  + reason (simple, upgrade to inline editor later). */}
                              <span className="rc-cur-adjust-btns" aria-label="human balance adjust controls">
                                <button
                                  type="button"
                                  className="rc-cur-adjust-btn rc-cur-adjust-plus"
                                  title={`Add copper/silver/gold to ${card.slug}:${card.instance >= 0 ? card.instance : 0} (human override — writes permanent audit row)`}
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    setAdjustTarget({
                                      seat: `${card.slug}:${card.instance >= 0 ? card.instance : 0}`,
                                      direction: "credit",
                                    });
                                  }}
                                >+</button>
                                <button
                                  type="button"
                                  className="rc-cur-adjust-btn rc-cur-adjust-minus"
                                  title={`Remove copper/silver/gold from ${card.slug}:${card.instance >= 0 ? card.instance : 0} (human override — can trip timed_out if balance crosses -1000c)`}
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    setAdjustTarget({
                                      seat: `${card.slug}:${card.instance >= 0 ? card.instance : 0}`,
                                      direction: "debit",
                                    });
                                  }}
                                >−</button>
                              </span>
                            </div>
                          );
                        })()}
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
                                              const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                                              console.error("[CollabTab] Failed to disconnect:", e);
                                              showToast(`Couldn't disconnect ${card.slug}:${inst} — ${msg}`, "error");
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

              {/* Launch All Vacant button — collapse-gating now handled by
                  the enclosing <CollapsibleSection> (children only render
                  when expanded), so the explicit !rosterSectionCollapsed
                  check is dropped. */}
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
              )}
              </CollapsibleSection>
              )}
            </>
          );
        })()}

        {/* P5-v2 Right Sidebar (`<aside>`) — single container for ALL rail-
            resident sections per architect:0 msg 935 spec. Replaces the
            float-right-each-section pattern with a flex-column inside a
            position:absolute aside. Section visibility is per-component:
            Discussion Mode always-on, Decision Panel always-on (its own
            collapsible-header), Claims/Audience/Replanning auto-hide when
            count===0 so empty sidebar collapses to just Discussion Mode +
            Decisions header. Locked spec: 280px fixed, all viewports. */}
        <aside className="collab-sidebar" aria-label="Project sidebar">

        {/* Phase 5 Surface 1 — Flow Feed (Chitragupta's Scroll). Relocated into
            the sidebar rail ABOVE Discussion Mode per human msg 1962. Read-only
            transaction ticker; reads read_currency_feed_cmd on the shared 30s
            currency poll. Collapsible, pref persisted; auto-hides when empty. */}
        {currencyFeed.length > 0 && (
          <CollapsibleSection
            id="flow-feed-section"
            className="flow-feed-section rail-section"
            collapsed={feedCollapsed}
            onToggle={toggleFeedCollapsed}
            title="Chitragupta"
            trailing={
              <>
                <span
                  className={`flow-feed-net ${flowNet >= 0 ? "flow-feed-net--up" : "flow-feed-net--down"}`}
                  title="Net flow over recent transactions (earnings minus losses)"
                >
                  {flowNet >= 0 ? "▲" : "▼"} {Math.abs(flowNet).toLocaleString()} copper
                </span>
                <button
                  type="button"
                  className={`flow-feed-notices-btn${inlineCurrencyNotices ? " is-on" : ""}`}
                  title={inlineCurrencyNotices
                    ? "Inline notices ON — significant events (disputes/losses) show in the timeline; click to hide"
                    : "Inline notices OFF — click to show significant currency events (disputes/losses) in the timeline"}
                  aria-label="Toggle inline currency notices in the timeline"
                  aria-pressed={inlineCurrencyNotices}
                  onClick={(e) => { e.stopPropagation(); toggleInlineCurrencyNotices(); }}
                >
                  {inlineCurrencyNotices ? "🔔" : "🔕"}
                </button>
                <button
                  type="button"
                  className="flow-feed-stats-btn"
                  title="Open detailed currency stats"
                  aria-label="Open detailed currency stats"
                  onClick={(e) => { e.stopPropagation(); setStatsOpen(true); }}
                >
                  📊 Stats
                </button>
              </>
            }
            headerTooltip={{ expand: "Expand the divine ledger", collapse: "Collapse the divine ledger" }}
          >
            <div className="flow-feed" ref={flowFeedRef} role="log" aria-label="Currency transaction feed" aria-live="polite">
              {flowFeedRows.map((line) => (
                <div key={line.key} className={`currency-line currency-line--${line.tier}`}>
                  <span className="currency-line-time">{line.at ? formatRelativeTime(line.at) : ""}</span>
                  <span
                    className="currency-line-dot"
                    style={{ background: line.seat ? getRoleColor(line.seat.split(":")[0]) : "#888" }}
                    aria-hidden="true"
                  />
                  <span className="currency-line-text">{line.text}</span>
                </div>
              ))}
            </div>
          </CollapsibleSection>
        )}

        {/* Phase 5 (human msg 1971) — "More Stats" popup. Accessible modal with
            the deep currency breakdown: total in play, per-seat leaderboard with
            balance/escrow bars (red below 0), open disputes + pools, and recent
            transactions. Read-only; derives entirely from already-fetched data. */}
        {statsOpen && (
          <div
            className="currency-stats-overlay"
            role="presentation"
            onClick={() => setStatsOpen(false)}
          >
            <div
              className="currency-stats-modal"
              role="dialog"
              aria-modal="true"
              aria-label="Chitragupta — detailed currency stats"
              onClick={(e) => e.stopPropagation()}
              onKeyDown={(e) => { if (e.key === "Escape") setStatsOpen(false); }}
            >
              <div className="currency-stats-header">
                <h2 className="currency-stats-title">Chitragupta — Currency Stats</h2>
                <button
                  type="button"
                  className="currency-stats-close"
                  aria-label="Close currency stats"
                  autoFocus
                  onClick={() => setStatsOpen(false)}
                >
                  ×
                </button>
              </div>

              <div className="currency-stats-total">
                <span className="currency-stats-total-label">Total in play</span>
                <span className="currency-stats-total-value">
                  {currencyLeaderboard.total.toLocaleString()} copper
                </span>
              </div>

              <div className="currency-stats-section">
                <h3 className="currency-stats-subhead">Balances ({currencyLeaderboard.seats.length} seats)</h3>
                {currencyLeaderboard.seats.length === 0 ? (
                  <div className="currency-stats-empty">No initialized seats yet.</div>
                ) : (
                  currencyLeaderboard.seats.map((s) => {
                    const pct = Math.max(0, Math.min(100, (s.balance / 10000) * 100));
                    const negative = s.balance < 0;
                    return (
                      <div key={s.label} className="currency-stats-row">
                        <span
                          className="currency-stats-dot"
                          style={{ background: getRoleColor(s.label.split(":")[0]) }}
                          aria-hidden="true"
                        />
                        <span className="currency-stats-seat">{s.label}</span>
                        <span className="currency-stats-bar-track" aria-hidden="true">
                          <span
                            className={`currency-stats-bar${negative ? " currency-stats-bar--neg" : ""}`}
                            style={{ width: `${negative ? 100 : pct}%` }}
                          />
                        </span>
                        <span className={`currency-stats-amount${negative ? " currency-stats-amount--neg" : ""}`}>
                          {s.balance.toLocaleString()}
                          {s.escrow > 0 && <span className="currency-stats-escrow"> +{s.escrow.toLocaleString()} esc</span>}
                        </span>
                      </div>
                    );
                  })
                )}
              </div>

              <div className="currency-stats-section">
                <h3 className="currency-stats-subhead">Disputes ({openDisputes.length})</h3>
                {openDisputes.length === 0 ? (
                  <div className="currency-stats-empty">No disputes recorded.</div>
                ) : (
                  openDisputes.map((d) => (
                    <div key={d.id} className="currency-stats-dispute">
                      <span className={`currency-stats-dispute-status currency-stats-dispute-status--${d.status}`}>
                        {d.status}
                      </span>
                      <span className="currency-stats-dispute-parties">
                        {d.challenger} vs {d.target}
                      </span>
                      {typeof d.pool === "number" && (
                        <span className="currency-stats-dispute-pool">{d.pool.toLocaleString()} cu pool</span>
                      )}
                      {d.judge != null && <span className="currency-stats-dispute-judge">judge: {d.judge}</span>}
                    </div>
                  ))
                )}
              </div>

              <div className="currency-stats-section">
                <h3 className="currency-stats-subhead">Recent transactions</h3>
                <div className="currency-stats-feed">
                  {flowFeedRows.slice(-15).reverse().map((line) => (
                    <div key={line.key} className={`currency-line currency-line--${line.tier}`}>
                      <span className="currency-line-time">{line.at ? formatRelativeTime(line.at) : ""}</span>
                      <span className="currency-line-text">{line.text}</span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Phase 6 (human msg 2074) — Bounty Board. Read-only display; actions
            (claim/submit/approve/etc) are performed via the currency_* MCP tools
            by agents/human — mirrors the Judge Seat display-only pattern (buttons
            show the exact tool to run). Auto-hides when no bounties exist. */}
        {bounties.length > 0 && (
          <CollapsibleSection
            id="bounty-board-section"
            className="bounty-board-section rail-section"
            collapsed={bountyBoardCollapsed}
            onToggle={toggleBountyBoardCollapsed}
            title="Bounty Board"
            trailing={
              <span className="bounty-board-count">
                {bounties.filter((b) => ["open", "claimed", "submitted"].includes(b.status || "")).length} pending
              </span>
            }
          >
            <div className="bounty-board">
              {[...bounties].sort((a, b) => (b.turn_posted ?? 0) - (a.turn_posted ?? 0)).map((b) => (
                <div key={b.id} className={`bounty-row bounty-row--${b.status || "open"}`}>
                  <div className="bounty-row-head">
                    <span className={`bounty-status bounty-status--${b.status || "open"}`}>{b.status || "open"}</span>
                    <span className="bounty-amount">{(b.amount ?? 0).toLocaleString()}c</span>
                  </div>
                  <div className="bounty-desc">{b.description || ""}</div>
                  <div className="bounty-meta">
                    {b.claimant && <span className="bounty-claimant">claimant: {b.claimant}</span>}
                    {b.deadline_turn != null && <span className="bounty-deadline">deadline turn {b.deadline_turn}</span>}
                    {b.status === "approved" && b.approved_by && <span className="bounty-approved">✓ paid · approved by {b.approved_by}</span>}
                    {b.last_rejection_reason && <span className="bounty-reject">rejected: {b.last_rejection_reason}</span>}
                  </div>
                  <div className="bounty-actions">
                    {b.status === "open" && (
                      <button className="bounty-btn" onClick={() => showToast(`To claim: run currency_claim_bounty("${b.id}") — stakes ${Math.floor((b.amount ?? 0) * 0.1)} copper (10%).`, "info")}>Claim</button>
                    )}
                    {b.status === "claimed" && (
                      <>
                        <button className="bounty-btn" onClick={() => showToast(`Claimant: run currency_submit_bounty("${b.id}", <your work's msg id>) to submit.`, "info")}>Submit</button>
                        <button className="bounty-btn" onClick={() => showToast(`Claimant: run currency_abandon_bounty("${b.id}") to abandon (forfeits half the stake).`, "info")}>Abandon</button>
                      </>
                    )}
                    {b.status === "submitted" && (
                      <>
                        <button className="bounty-btn" onClick={() => showToast(`Human/judge: run currency_approve_bounty("${b.id}") to pay out ${(b.amount ?? 0).toLocaleString()} copper + return stake.`, "info")}>Approve</button>
                        <button className="bounty-btn" onClick={() => showToast(`Human/judge: run currency_reject_bounty("${b.id}", "<reason>") — claimant forfeits full stake.`, "info")}>Reject</button>
                      </>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </CollapsibleSection>
        )}

        {/* Phase 7 (human msg 2074) — Lifetime Scoreboard. Aggregates every
            end-of-session snapshot per agent (frontend TS). Collapsed by default;
            auto-hides until at least one session has ended + been snapshotted. */}
        {scoreboard.length > 0 && (
          <CollapsibleSection
            id="scoreboard-section"
            className="scoreboard-section rail-section"
            collapsed={scoreboardCollapsed}
            onToggle={toggleScoreboardCollapsed}
            title="Lifetime Scoreboard"
            trailing={<span className="scoreboard-count">{sessionHistory.length} session{sessionHistory.length === 1 ? "" : "s"}</span>}
          >
            <div className="scoreboard">
              <div className="scoreboard-row scoreboard-row--head">
                <span className="sb-agent">Agent</span>
                <span className="sb-num" title="Sessions">Sess</span>
                <span className="sb-num" title="Net (earned − lost)">Net</span>
                <span className="sb-num" title="Disputes won / lost">W/L</span>
                <span className="sb-num" title="Bounties completed">Bnty</span>
                <span className="sb-num" title="Times timed out">TO</span>
                <span className="sb-num" title="Average final balance">Avg</span>
              </div>
              {scoreboard.map((a) => (
                <div key={a.label} className="scoreboard-row">
                  <span className="sb-agent">
                    <span className="sb-dot" style={{ background: getRoleColor(a.label.split(":")[0]) }} aria-hidden="true" />
                    {a.label}
                  </span>
                  <span className="sb-num">{a.sessions}</span>
                  <span className={`sb-num ${a.net >= 0 ? "sb-pos" : "sb-neg"}`}>{a.net >= 0 ? "+" : ""}{a.net.toLocaleString()}</span>
                  <span className="sb-num">{a.disputesWon}/{a.disputesLost}</span>
                  <span className="sb-num">{a.bounties}</span>
                  <span className="sb-num">{a.timedOut}</span>
                  <span className="sb-num">{a.avg.toLocaleString()}</span>
                </div>
              ))}
            </div>
          </CollapsibleSection>
        )}

        {/* Discussion Mode sidebar card — replaces the inline horizontal
            `.discussion-mode-strip` per human msg 932 (option B) + architect
            msg 935. Always-on at top of the right rail. Uses the existing
            `.is-vertical` layout variant on AssemblyControls (CSS in
            AssemblyControls.css §vertical-stack, prop wiring already in
            AssemblyControls.tsx). Wrapping in CollapsibleSection so the
            human can collapse it to a 30px header when assembly is OFF. */}
        {twoControlsProtocol && (() => {
          const dmCollapsedEffective = discussionModeCardCollapsed ?? !twoControlsProtocol.floor?.assembly_active;
          const livePreset = (twoControlsProtocol?.preset as string) ?? "Default chat";
          const livePhase = twoControlsProtocol?.floor?.phase as string | undefined;
          const assemblyOn = twoControlsProtocol?.floor?.assembly_active === true;
          const visibilityMode = project?.config?.settings?.discussion_mode === "open" ? "Open" : "Directed";
          const phaseLabel = livePhase === "execution" ? " · Executing" : livePhase === "planning" ? " · Planning" : "";
          const dmTrailing = (
            <span className="discussion-mode-section-state" aria-label={`current mode: ${assemblyOn ? livePreset : "off"}${phaseLabel}, visibility ${visibilityMode}`}>
              <span className={`dm-state-dot${assemblyOn ? " dm-state-dot-on" : ""}`} aria-hidden="true" />
              {assemblyOn ? livePreset : "off"}{phaseLabel} · {visibilityMode}
            </span>
          );
          return (
          <CollapsibleSection
            id="discussion-mode-section"
            title="Discussion Mode"
            trailing={dmTrailing}
            collapsed={dmCollapsedEffective}
            onToggle={() => updateDiscussionModeCardCollapsed(!dmCollapsedEffective)}
            className="discussion-mode-section rail-section"
            headerTooltip={{ expand: "Expand discussion mode controls", collapse: "Collapse discussion mode controls" }}
          >
            {/* Visibility (Directed/Open) selector — relocated from the
                top header strip per human msg 571 + ui-arch msg 729. Lives
                above AssemblyControls so the human sees who-sees-what +
                who-speaks-when under the same heading. */}
            {(() => {
              const currentMode = project?.config?.settings?.discussion_mode || "directed";
              const modes: Record<string, { label: string; color: string; desc: string }> = {
                directed: { label: "Directed", color: "#1da1f2", desc: "Agents only see messages addressed to them" },
                open: { label: "Open", color: "#f5a623", desc: "All agents see all messages" },
              };
              const active = modes[currentMode] || modes.directed;
              return (
                <div className="dm-visibility-block" ref={discussionModeRef}>
                  <span className="dm-visibility-label">Visibility</span>
                  <div className="dm-visibility-current">
                    <span
                      className="discussion-mode-badge"
                      role="button"
                      tabIndex={0}
                      aria-haspopup="listbox"
                      aria-expanded={discussionModeOpen}
                      style={{
                        background: `${active.color}22`,
                        color: active.color,
                        borderColor: `${active.color}55`,
                      }}
                      onClick={() => setDiscussionModeOpen(!discussionModeOpen)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          setDiscussionModeOpen(!discussionModeOpen);
                        }
                      }}
                      title={active.desc}
                    >
                      {active.label}
                    </span>
                    <span className="dm-visibility-desc">{active.desc}</span>
                  </div>
                  {discussionModeOpen && (
                    <div className="discussion-mode-dropdown dm-visibility-dropdown" role="listbox">
                      {Object.entries(modes).map(([id, m]) => (
                        <div
                          key={id}
                          role="option"
                          aria-selected={currentMode === id}
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
            <AssemblyControls
              protocol={twoControlsProtocol}
              mutate={twoControlsMutate}
              lastError={twoControlsLastError}
              selfRole={null}
              projectDir={projectDir}
              layout="vertical"
            />
          </CollapsibleSection>
          );
        })()}

        {/* Active Oxford Debate panel (human msg 1090/1092 — "Still no UI
            changes with starting oxford style"). Pre-fix, the only visible
            signal was the right-rail button label flipping from Start → End
            after a 2s poll tick. This panel makes the active-debate state
            unmissable: premise, sides (with current-speaker highlight),
            moderator, turn count. Renders only when activeOxford is non-null,
            so the rail collapses to zero pixels when no debate is running. */}
        {projectDir && activeOxford && (
          <div className="active-oxford-panel rail-section" role="region" aria-label="Active Oxford debate">
            <div className="active-oxford-header">
              <span className="active-oxford-badge" aria-hidden="true">⚖</span>
              <span className="active-oxford-title">Active Oxford Debate #{activeOxford.debate_id}</span>
            </div>
            <div className="active-oxford-premise">
              <span className="active-oxford-field-label">Premise</span>
              <span className="active-oxford-premise-text">{activeOxford.premise}</span>
            </div>
            <div className="active-oxford-grid">
              <div className="active-oxford-block">
                <span className="active-oxford-field-label">Moderator</span>
                <span className="active-oxford-seat-pill">{activeOxford.moderator}</span>
              </div>
              <div className="active-oxford-block">
                <span className="active-oxford-field-label">Side A ({activeOxford.side_a.length})</span>
                <div className="active-oxford-seat-row">
                  {activeOxford.side_a.map((s) => (
                    <span
                      key={s}
                      className={`active-oxford-seat-pill${activeOxford.current_speaker === s ? " active-oxford-seat-pill-speaking" : ""}`}
                    >
                      {s}
                      {activeOxford.current_speaker === s && <span className="active-oxford-mic" aria-label="speaking"> 🎤</span>}
                    </span>
                  ))}
                </div>
              </div>
              <div className="active-oxford-block">
                <span className="active-oxford-field-label">Side B ({activeOxford.side_b.length})</span>
                <div className="active-oxford-seat-row">
                  {activeOxford.side_b.map((s) => (
                    <span
                      key={s}
                      className={`active-oxford-seat-pill${activeOxford.current_speaker === s ? " active-oxford-seat-pill-speaking" : ""}`}
                    >
                      {s}
                      {activeOxford.current_speaker === s && <span className="active-oxford-mic" aria-label="speaking"> 🎤</span>}
                    </span>
                  ))}
                </div>
              </div>
            </div>
            <div className="active-oxford-status">
              {activeOxford.current_speaker
                ? <span>Current speaker: <strong>{activeOxford.current_speaker}</strong> · turn {activeOxford.turn_count}</span>
                : activeOxford.turn_count === 0
                  ? <span className="active-oxford-status-pending">Awaiting opening declare from moderator <strong>{activeOxford.moderator}</strong> (auto-opens in ≤30s)</span>
                  : <span>Between turns · {activeOxford.turn_count} turn{activeOxford.turn_count === 1 ? "" : "s"} so far</span>
              }
            </div>
            {/* SHA-10.5 v1: phase indicator (architect msg 1280 + ui-architect
                msg 1400 spec v2 + dev-challenger msg 1402 finds). Renders only
                when activeOxford.phase != "none" (legacy debates). Phase chip
                uses distinct hue per category (Find A: not amber, that's the
                panel's house color). Elapsed time is static-at-render —
                refreshes on each 2s polling tick. Live countdown is a polish
                deferred to SHA-10.5.1 (would require a per-second tick that
                this rail doesn't currently maintain). */}
            {activeOxford.phase && activeOxford.phase !== "none" && (() => {
              const phase = activeOxford.phase;
              // SHA-10.5 v1a (ui-architect msg 1411 palette correction per
              // dev-challenger msg 1402 Find B empirical WCAG measurement):
              // chip TEXT uses 200-shade for ≥13:1 contrast on dark bg.
              // Border + bg-tint use 500-shade for identity. Same precedent
              // as existing .active-oxford-seat-pill-speaking (green-500
              // border + bg, green-200 text). Pre-v1a used 500-shade for
              // text → empirically failed AA on 3 of 4 hues (rose ~3.7,
              // purple ~2.7, slate ~2.1 vs required 4.5).
              const phase500 =                                              // border + bg-tint
                phase === "opening_a" || phase === "opening_b" ? "#14b8a6" :
                phase === "rebuttal_a" || phase === "rebuttal_b" ? "#f43f5e" :
                phase === "audience_q" ? "#a855f7" :
                phase === "closing_a" || phase === "closing_b" ? "#64748b" :
                phase === "ended" ? "#6b7280" : "#9ca3af";
              const phaseColor =                                            // text (200-shade)
                phase === "opening_a" || phase === "opening_b" ? "#99f6e4" :
                phase === "rebuttal_a" || phase === "rebuttal_b" ? "#fecdd3" :
                phase === "audience_q" ? "#e9d5ff" :
                phase === "closing_a" || phase === "closing_b" ? "#e2e8f0" :
                phase === "ended" ? "#e5e7eb" : "#f3f4f6";
              const phaseLabel =
                phase === "opening_a" ? "Opening · Side A" :
                phase === "opening_b" ? "Opening · Side B" :
                phase === "rebuttal_a" ? "Rebuttal · Side A" :
                phase === "rebuttal_b" ? "Rebuttal · Side B" :
                phase === "audience_q" ? "Audience Q&A" :
                phase === "closing_a" ? "Closing · Side A" :
                phase === "closing_b" ? "Closing · Side B" :
                phase === "ended" ? "Ended" : phase;
              // Floor values mirror collab::oxford::OXFORD_PHASE_*_SECS
              // const defaults (architect msg 1280 lock). Hardcoded
              // frontend-side for v1; future SHA-10.5.x can fetch from
              // backend via a new read_oxford_phase_config_cmd.
              const hardSecs =
                phase === "opening_a" || phase === "opening_b" ? 180 :
                phase === "rebuttal_a" || phase === "rebuttal_b" ? 120 :
                phase === "closing_a" || phase === "closing_b" ? 90 :
                phase === "audience_q" ? 300 : 0;
              const startedAt = activeOxford.phase_started_at
                ? new Date(activeOxford.phase_started_at).getTime()
                : 0;
              const elapsedSecs = startedAt > 0
                ? Math.max(0, Math.floor((Date.now() - startedAt) / 1000))
                : 0;
              const remainingSecs = hardSecs > 0
                ? Math.max(0, hardSecs - elapsedSecs)
                : 0;
              // Color shifts per dev-challenger msg 1402 Find A:
              // orange under 30s, red under 10s — NOT amber (which collides
              // with the panel's house color).
              const timerColor =
                hardSecs === 0 ? "#9ca3af" :
                remainingSecs <= 10 ? "#dc2626" :
                remainingSecs <= 30 ? "#f97316" :
                "#cbd5e1";
              const mm = Math.floor(remainingSecs / 60);
              const ss = remainingSecs % 60;
              const timerLabel = hardSecs > 0
                ? `${mm}:${ss.toString().padStart(2, "0")}`
                : "--:--";
              return (
                <div className="active-oxford-phase-row" role="group" aria-label="Debate phase">
                  <span
                    className="active-oxford-phase-chip"
                    style={{
                      // Border + bg-tint = 500-shade per existing
                      // .active-oxford-seat-pill-speaking precedent;
                      // text = 200-shade for WCAG AAA contrast.
                      background: `${phase500}2e`,            /* 18% alpha */
                      borderLeft: `3px solid ${phase500}`,
                      color: phaseColor,
                    }}
                    aria-label={`Phase: ${phaseLabel}`}
                  >
                    {phaseLabel}
                  </span>
                  <span
                    className="active-oxford-phase-timer"
                    style={{ color: timerColor, fontFamily: "ui-monospace, Consolas, monospace" }}
                    aria-live="polite"
                    aria-label={`Time remaining: ${mm} minutes ${ss} seconds`}
                  >
                    {timerLabel}
                  </span>
                  <span className="active-oxford-phase-elapsed" style={{ color: "#9ca3af", fontSize: "11px" }}>
                    elapsed {elapsedSecs}s / {hardSecs}s hard
                  </span>
                </div>
              );
            })()}
          </div>
        )}

        {/* Active Delphi Discussion panel — parallel to active-oxford-panel.
            Renders the live Delphi state (round counter, phase, submission
            progress, anonymized aggregate when ready). Per human msg 1939
            (Delphi-to-Oxford-parity build) + spec §7.1/§7.2 per-role branching.
            Human-role categorized from activeDelphi role assignment; preserves
            self-imposed blind when human is participant or audience. */}
        {projectDir && activeDelphi && (() => {
          const humanRole: "moderator" | "participant" | "audience" | "observer" =
            activeDelphi.moderator === "human:0" ? "moderator" :
            activeDelphi.participants.includes("human:0") ? "participant" :
            activeDelphi.audience.includes("human:0") ? "audience" :
            "observer";
          const inBlindPhase = activeDelphi.phase === "submitting";
          const revealAllowed = humanRole === "moderator" || activeDelphi.phase === "reviewing" || activeDelphi.phase === "ended";
          const showCountOnly = humanRole === "audience" && inBlindPhase;
          // Spec v3 §4.2 — submissions/aggregate live inside rounds[N-1], not at top level.
          // Derive flat shapes for the panel render.
          const currentRoundData = activeDelphi.current_round > 0
            ? activeDelphi.rounds[activeDelphi.current_round - 1] ?? null
            : null;
          const nonSubmitters = currentRoundData?.non_submitters ?? [];
          const submitted_seats = activeDelphi.participants.filter((p) => !nonSubmitters.includes(p));
          const aggregate_message_id = currentRoundData?.aggregate_message_id ?? null;
          const showAggregate = aggregate_message_id !== null && activeDelphi.phase !== "submitting" && activeDelphi.phase !== "opening";
          return (
            <div
              className={`active-delphi-panel rail-section role-${humanRole}`}
              role="region"
              aria-label={`Active Delphi discussion (your role: ${humanRole})`}
            >
              <div className="active-delphi-header">
                <span aria-hidden="true">🔮</span>
                <span className="active-delphi-title">Delphi Discussion #{activeDelphi.discussion_id}</span>
                {activeDelphi.phase && (
                  <span className={`active-delphi-phase-pill phase-${activeDelphi.phase}`} aria-label={`Phase: ${activeDelphi.phase}`}>
                    {activeDelphi.phase}
                  </span>
                )}
                <span className={`active-delphi-self-role-chip role-${humanRole}`} title={`Your role in this discussion: ${humanRole}`}>
                  you: {humanRole}
                </span>
              </div>
              <div className="active-delphi-topic">
                <span className="active-delphi-topic-text">{activeDelphi.topic}</span>
              </div>
              <div className="active-delphi-round-row">
                <span>
                  {activeDelphi.current_round === 0
                    ? <span className="active-delphi-round-label">Awaiting round 1</span>
                    : <><span className="active-delphi-round-label">Round {activeDelphi.current_round}</span> of {activeDelphi.max_rounds}</>
                  }
                </span>
                {inBlindPhase && (
                  <span>{submitted_seats.length} / {activeDelphi.participants.length} submitted</span>
                )}
              </div>
              {inBlindPhase && (
                <div
                  className="active-delphi-submissions-bar"
                  role="progressbar"
                  aria-valuenow={submitted_seats.length}
                  aria-valuemin={0}
                  aria-valuemax={activeDelphi.participants.length}
                  aria-label={`${submitted_seats.length} of ${activeDelphi.participants.length} participants submitted`}
                >
                  <div
                    className="active-delphi-submissions-fill"
                    style={{
                      width: activeDelphi.participants.length > 0
                        ? `${(submitted_seats.length / activeDelphi.participants.length) * 100}%`
                        : "0%",
                    }}
                  />
                </div>
              )}
              {/* Phase progress bar with soft/hard floor markers + live countdown.
                  Per spec §7.1 item 3. Floor defaults hardcoded frontend-side to
                  match spec §3.1 (180s soft / 360s hard submitting; 300s reviewing).
                  Mirrors Oxford pattern at CollabTab.tsx ~6191; refreshes on 2s
                  poll tick once delphi_get_state_cmd is wired. */}
              {(activeDelphi.phase === "submitting" || activeDelphi.phase === "reviewing") && activeDelphi.phase_started_at && (() => {
                const startedAt = new Date(activeDelphi.phase_started_at).getTime();
                const elapsedSecs = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
                const isSubmitting = activeDelphi.phase === "submitting";
                const softFloor = isSubmitting ? activeDelphi.submission_soft_floor_secs : activeDelphi.review_floor_secs;
                const hardFloor = isSubmitting ? activeDelphi.submission_hard_floor_secs : activeDelphi.review_floor_secs;
                const ceiling = Math.max(hardFloor, elapsedSecs);
                const elapsedPct = ceiling > 0 ? (elapsedSecs / ceiling) * 100 : 0;
                const softMarkerPct = ceiling > 0 ? (softFloor / ceiling) * 100 : 0;
                const hardMarkerPct = ceiling > 0 ? (hardFloor / ceiling) * 100 : 0;
                const remainingHard = Math.max(0, hardFloor - elapsedSecs);
                const remainingSoft = Math.max(0, softFloor - elapsedSecs);
                const mm = Math.floor(remainingHard / 60);
                const ss = remainingHard % 60;
                const overSoft = elapsedSecs >= softFloor;
                const overHard = elapsedSecs >= hardFloor;
                const timerColor = overHard ? "#dc2626" : overSoft ? "#f97316" : "#cbd5e1";
                const phaseLabel = isSubmitting ? "Submission window" : "Review window";
                return (
                  <div className="active-delphi-phase-progress" role="group" aria-label={`${phaseLabel} timing`}>
                    <div className="active-delphi-phase-progress-header">
                      <span className="active-delphi-phase-progress-label">{phaseLabel}</span>
                      <span
                        className="active-delphi-phase-progress-timer"
                        style={{ color: timerColor }}
                        aria-live="polite"
                        aria-label={overHard ? "Past hard floor" : `${mm} minutes ${ss} seconds until hard floor`}
                      >
                        {overHard ? "hard floor passed" : `${mm}:${ss.toString().padStart(2, "0")} until hard`}
                      </span>
                    </div>
                    <div className="active-delphi-phase-progress-bar" aria-hidden="true">
                      <div className="active-delphi-phase-progress-fill" style={{ width: `${Math.min(100, elapsedPct)}%` }} />
                      {!isSubmitting || softFloor !== hardFloor ? (
                        <div className="active-delphi-phase-progress-marker soft" style={{ left: `${softMarkerPct}%` }} title={`Soft floor: ${softFloor}s`} />
                      ) : null}
                      <div className="active-delphi-phase-progress-marker hard" style={{ left: `${hardMarkerPct}%` }} title={`Hard floor: ${hardFloor}s`} />
                    </div>
                    <div className="active-delphi-phase-progress-legend">
                      <span>elapsed {elapsedSecs}s</span>
                      {isSubmitting && softFloor !== hardFloor && (
                        <span style={{ color: overSoft ? "#fbbf24" : "rgba(255,255,255,0.4)" }}>
                          soft @ {softFloor}s {overSoft && remainingSoft === 0 ? "(passed)" : ""}
                        </span>
                      )}
                      <span style={{ color: overHard ? "#dc2626" : "rgba(255,255,255,0.4)" }}>
                        hard @ {hardFloor}s {overHard ? "(passed)" : ""}
                      </span>
                    </div>
                  </div>
                );
              })()}
              {/* Seat rendering — branched per humanRole.
                  - moderator: per-seat names + submission status (COI auditing)
                  - participant: own seat + anonymized count for others during blind
                  - audience: count-only during blind, names after reveal
                  - observer: same as audience (preserve blind) */}
              {showCountOnly ? (
                <div className="active-delphi-empty" aria-live="polite">
                  {submitted_seats.length === 0
                    ? "No submissions yet — participants are thinking blind."
                    : `${submitted_seats.length} of ${activeDelphi.participants.length} participants have submitted (identities blind until reveal).`}
                </div>
              ) : (
                <div className="active-delphi-seats">
                  <span className="active-delphi-seat-pill moderator" title={`Moderator${humanRole === "moderator" ? " (you)" : ""}`}>{activeDelphi.moderator}</span>
                  {activeDelphi.participants.map((p) => {
                    const submitted = submitted_seats.includes(p);
                    const isMe = p === "human:0";
                    if (inBlindPhase && !revealAllowed && submitted && !isMe) {
                      return <span key={p} className="active-delphi-anon-chip" title="Submitted (identity blind until reveal)">Anon</span>;
                    }
                    return (
                      <span
                        key={p}
                        className={`active-delphi-seat-pill${submitted ? " submitted" : ""}${isMe ? " is-self" : ""}`}
                        title={`${p}${isMe ? " (you)" : ""}${submitted ? " — submitted" : " — pending"}`}
                      >
                        {p}{isMe && " (you)"}
                      </span>
                    );
                  })}
                </div>
              )}
              {showAggregate && aggregate_message_id !== null && (() => {
                const aggMsg = project?.messages?.find((m) => m.id === aggregate_message_id);
                const aggId = aggregate_message_id;
                const expanded = delphiAggregateExpanded[aggId] !== false;
                const revealAvailable = activeDelphi.phase === "ended";
                const unshuffleMap = currentRoundData?.unshuffle_map ?? {};
                // Substitute "Anonymous A:" → "real_seat:" when reveal toggle is
                // ON. Per spec §5 the unshuffle_map is populated at round-close
                // (rounds[].unshuffle_map = { anonymous_id → real_seat }).
                // The aggregate markdown body (built by collab::build_aggregate)
                // uses labels of the form `Anonymous <ID>:` — so the substitution
                // is a per-mapping `replaceAll` on the body string.
                const renderedBody = (() => {
                  if (!aggMsg) return null;
                  if (!revealAvailable || !delphiRealNamesRevealed) return aggMsg.body;
                  let body = aggMsg.body;
                  for (const [anon, real] of Object.entries(unshuffleMap)) {
                    // Replace both label forms ("Anonymous A:" and "Anonymous A" sans colon, in case markdown variants exist)
                    body = body.split(`Anonymous ${anon}:`).join(`${real}:`);
                    body = body.split(`Anonymous ${anon}`).join(real);
                  }
                  return body;
                })();
                return (
                  <div className="active-delphi-aggregate">
                    <div className="active-delphi-aggregate-header">
                      <span className="active-delphi-aggregate-label">
                        Round {activeDelphi.current_round} aggregate
                        {revealAvailable && delphiRealNamesRevealed && " · identities revealed"}
                      </span>
                      <button
                        type="button"
                        className="active-delphi-aggregate-toggle"
                        onClick={() => setDelphiAggregateExpanded((prev) => ({ ...prev, [aggId]: !expanded }))}
                        aria-expanded={expanded}
                        aria-controls={`delphi-aggregate-body-${aggId}`}
                        title={expanded ? "Collapse aggregate" : "Expand aggregate"}
                      >
                        {expanded ? "▾" : "▸"}
                      </button>
                    </div>
                    {expanded && (
                      <div
                        id={`delphi-aggregate-body-${aggId}`}
                        className="active-delphi-aggregate-body"
                        tabIndex={0}
                        role="region"
                        aria-label={`Round ${activeDelphi.current_round} aggregate body${revealAvailable && delphiRealNamesRevealed ? " — identities revealed" : ""}`}
                      >
                        {renderedBody !== null ? (
                          <pre className="active-delphi-aggregate-text">{renderedBody}</pre>
                        ) : (
                          <span className="active-delphi-aggregate-pending">
                            Loading aggregate (msg #{aggId})…
                          </span>
                        )}
                      </div>
                    )}
                    {revealAvailable && (
                      <div className="active-delphi-aggregate-reveal-row">
                        <button
                          type="button"
                          className="active-delphi-reveal-btn"
                          onClick={() => setDelphiRealNamesRevealed((v) => !v)}
                          title="Toggle anonymous IDs vs real seat names (only available after discussion ends)"
                          aria-pressed={delphiRealNamesRevealed}
                        >
                          {delphiRealNamesRevealed ? "🔒 Hide real names" : "🔓 Show real names"}
                        </button>
                        <span className="active-delphi-reveal-hint">
                          Unshuffle map archived in `.vaak/delphi-discussions.jsonl` for post-hoc audit.
                        </span>
                      </div>
                    )}
                  </div>
                );
              })()}
              {submitted_seats.length === 0 && inBlindPhase && !showCountOnly && (
                <div className="active-delphi-empty">
                  Waiting for first blind submission…
                </div>
              )}
              {/* SHA-DUI.9 — round-control buttons. Replaces non-clickable plaintext
                  <code> labels with real <button>s invoking Tauri commands directly
                  (bypasses MCP sidecar — works even when CC sidecars are stale).
                  Gate widened from `humanRole === "moderator"` to `humanRole !== "observer"`
                  per spec §6.6.1 human-authority bypass (any non-observer human can drive
                  round-control via Tauri). Adds Close Round button (missing from prior
                  panel) and converts End-as-Converged plaintext to a button. */}
              {humanRole !== "observer" && (activeDelphi.phase === "opening" || activeDelphi.phase === "submitting" || activeDelphi.phase === "reviewing") && (
                <div className="active-delphi-moderator-controls">
                  <span className="active-delphi-mod-hint">
                    {humanRole === "moderator" ? "Moderator controls:" : "Human-authority controls (spec §6.6.1):"}
                  </span>
                  {(activeDelphi.phase === "opening" || activeDelphi.phase === "reviewing") && (
                    <button
                      type="button"
                      className="active-delphi-mod-btn"
                      onClick={async () => {
                        try {
                          const { invoke } = await import("@tauri-apps/api/core");
                          await invoke("delphi_open_round_cmd", { dir: projectDir });
                          showToast(`Round opened.`, "success");
                        } catch (e) {
                          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                          showToast(`Couldn't open round — ${msg}`, "error");
                        }
                      }}
                      title="Open the next blind-submission round (Tauri direct, no MCP dependency)"
                    >
                      ▶ Open Round
                    </button>
                  )}
                  {activeDelphi.phase === "submitting" && (
                    <button
                      type="button"
                      className="active-delphi-mod-btn"
                      onClick={async () => {
                        try {
                          const { invoke } = await import("@tauri-apps/api/core");
                          await invoke("delphi_close_round_cmd", { dir: projectDir });
                          showToast(`Round closed; aggregate publishing.`, "success");
                        } catch (e) {
                          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                          showToast(`Couldn't close round — ${msg}`, "error");
                        }
                      }}
                      title="Close the current round and publish the anonymized aggregate"
                    >
                      ⏸ Close Round
                    </button>
                  )}
                  {activeDelphi.phase === "reviewing" && (
                    <button
                      type="button"
                      className="active-delphi-mod-btn active-delphi-mod-btn-converged"
                      onClick={async () => {
                        const confirmed = window.confirm(
                          `End Delphi #${activeDelphi.discussion_id} as CONVERGED?\n\nTopic: ${activeDelphi.topic}\n\nThis declares convergence and distributes any convergence reward (currently ${activeDelphi.convergence_reward_copper} copper).`,
                        );
                        if (!confirmed) return;
                        try {
                          const { invoke } = await import("@tauri-apps/api/core");
                          await invoke("delphi_end_cmd", { dir: projectDir, outcome: "converged" });
                          setActiveDelphi(null);
                          showToast(`Delphi #${activeDelphi.discussion_id} ended as converged.`, "success");
                        } catch (e) {
                          const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                          showToast(`Couldn't end as converged — ${msg}`, "error");
                        }
                      }}
                      title="End discussion declaring convergence (distributes convergence reward)"
                    >
                      ✓ End as Converged
                    </button>
                  )}
                </div>
              )}
              {/* SHA-DUI.10 — participant Submit form. Renders when human is a
                  participant in the current Delphi AND phase is `submitting`.
                  Invokes `delphi_submit_cmd` (Tauri direct, no MCP dependency).
                  Per spec §6.6.1 the human can always submit; this is the canonical
                  UI surface for blind submissions. Uncontrolled <form> + FormData
                  (no extra useState) to keep the diff small. */}
              {humanRole === "participant" && activeDelphi.phase === "submitting" && (
                <form
                  className="active-delphi-submit-form"
                  onSubmit={async (e) => {
                    e.preventDefault();
                    const form = e.currentTarget;
                    const formData = new FormData(form);
                    const content = ((formData.get("content") as string) ?? "").trim();
                    if (content.length === 0) {
                      showToast("Submission is empty — type something first.", "error");
                      return;
                    }
                    try {
                      const { invoke } = await import("@tauri-apps/api/core");
                      await invoke("delphi_submit_cmd", { dir: projectDir, content });
                      showToast(`Submission recorded (blind, round ${activeDelphi.current_round}).`, "success");
                      form.reset();
                    } catch (err) {
                      const msg = typeof err === "string" ? err : (err instanceof Error ? err.message : String(err));
                      showToast(`Couldn't submit — ${msg}`, "error");
                    }
                  }}
                >
                  <label className="active-delphi-submit-label" htmlFor="active-delphi-submit-textarea">
                    Blind submission — round {activeDelphi.current_round}
                  </label>
                  <textarea
                    id="active-delphi-submit-textarea"
                    name="content"
                    className="active-delphi-submit-textarea"
                    rows={4}
                    required
                    minLength={1}
                    placeholder="Your blind position on the topic. Identity hidden until discussion ends."
                  />
                  <div className="active-delphi-submit-row">
                    <span className="active-delphi-submit-hint">
                      Anonymity lifts when the discussion ends. Other participants cannot see your submission until the moderator closes the round.
                    </span>
                    <button type="submit" className="active-delphi-submit-btn">
                      ▲ Submit Blind
                    </button>
                  </div>
                </form>
              )}
              {/* Phase history strip — spec §7.1 item 7. Renders compact
                  timeline of CLOSED rounds (closed_at != null). Shows round
                  number, submission count, elapsed duration, link to the
                  aggregate message id. Skips the currently-open round. */}
              {(() => {
                const closedRounds = activeDelphi.rounds.filter((r) => r.closed_at !== null);
                if (closedRounds.length === 0) return null;
                return (
                  <div className="active-delphi-history" aria-label="Completed rounds history">
                    <span className="active-delphi-history-label">History</span>
                    <div className="active-delphi-history-strip">
                      {closedRounds.map((r) => {
                        const opened = new Date(r.opened_at).getTime();
                        const closed = r.closed_at ? new Date(r.closed_at).getTime() : 0;
                        const durationSecs = closed > opened ? Math.floor((closed - opened) / 1000) : 0;
                        const mm = Math.floor(durationSecs / 60);
                        const ss = durationSecs % 60;
                        const submittedCount = activeDelphi.participants.length - (r.non_submitters?.length ?? 0);
                        return (
                          <div key={r.number} className="active-delphi-history-pill" title={`Round ${r.number}: ${submittedCount}/${activeDelphi.participants.length} submitted, ${mm}m${ss}s elapsed, aggregate msg #${r.aggregate_message_id ?? "—"}`}>
                            <span className="active-delphi-history-round">R{r.number}</span>
                            <span className="active-delphi-history-meta">{submittedCount}/{activeDelphi.participants.length}</span>
                            <span className="active-delphi-history-time">{mm > 0 ? `${mm}m${ss}s` : `${ss}s`}</span>
                            {r.aggregate_message_id !== null && (
                              <span className="active-delphi-history-msg">#{r.aggregate_message_id}</span>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  </div>
                );
              })()}
            </div>
          );
        })()}

        {/* Economy Settings + Oxford debate triggers (human msgs 657 + 706).
            Both live in the rail below Discussion Mode; small profile so they
            don't compete with the primary panels. The Oxford button swaps to
            End mode when an active debate is detected (human msg 870). */}
        {projectDir && (
          <div className="economy-settings-trigger rail-section">
            {activeOxford ? (
              <button
                type="button"
                className="economy-settings-btn economy-settings-btn-destructive"
                onClick={async () => {
                  const summary = `${activeOxford.side_a.length} vs ${activeOxford.side_b.length}, moderator ${activeOxford.moderator}`;
                  const confirmed = window.confirm(
                    `End debate ${activeOxford.debate_id} (${summary})?\n\nPremise: ${activeOxford.premise}\n\nThis will mark the debate as abandoned and clear active state. No reward will be distributed.`,
                  );
                  if (!confirmed) return;
                  try {
                    const { invoke } = await import("@tauri-apps/api/core");
                    await invoke("oxford_end_cmd", { dir: projectDir });
                    setActiveOxford(null);
                    showToast(`Debate ${activeOxford.debate_id} ended.`, "success");
                  } catch (e) {
                    const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                    showToast(`Couldn't end debate — ${msg}`, "error");
                  }
                }}
                title={`End the active Oxford debate (debate ${activeOxford.debate_id}, moderator ${activeOxford.moderator})`}
              >
                <span className="economy-settings-icon" aria-hidden="true">⏹</span>
                <span>End Oxford Debate</span>
              </button>
            ) : (
              <button
                type="button"
                className="economy-settings-btn"
                onClick={() => setOxfordSetupOpen(true)}
                title="Start an Oxford-style debate (Phase A v1)"
              >
                <span className="economy-settings-icon" aria-hidden="true">⚖</span>
                <span>Start Oxford Debate</span>
              </button>
            )}
            {activeDelphi ? (
              <button
                type="button"
                className="economy-settings-btn economy-settings-btn-destructive"
                onClick={async () => {
                  const roundData = activeDelphi.current_round > 0
                    ? activeDelphi.rounds[activeDelphi.current_round - 1] ?? null
                    : null;
                  const nonSub = roundData?.non_submitters ?? [];
                  const submittedCount = activeDelphi.participants.length - nonSub.length;
                  const confirmed = window.confirm(
                    `End Delphi discussion ${activeDelphi.discussion_id}?\n\nTopic: ${activeDelphi.topic}\n\nRound ${activeDelphi.current_round} of ${activeDelphi.max_rounds}, ${submittedCount}/${activeDelphi.participants.length} submitted this round.\n\nThis abandons the discussion. No convergence reward will be distributed.`,
                  );
                  if (!confirmed) return;
                  try {
                    const { invoke } = await import("@tauri-apps/api/core");
                    await invoke("delphi_end_cmd", { dir: projectDir, outcome: "abandoned" });
                    setActiveDelphi(null);
                    showToast(`Delphi discussion ${activeDelphi.discussion_id} ended.`, "success");
                  } catch (e) {
                    const msg = typeof e === "string" ? e : (e instanceof Error ? e.message : String(e));
                    showToast(`Couldn't end discussion — ${msg}`, "error");
                  }
                }}
                title={`End the active Delphi discussion (#${activeDelphi.discussion_id}, moderator ${activeDelphi.moderator})`}
              >
                <span className="economy-settings-icon" aria-hidden="true">⏹</span>
                <span>End Delphi Discussion</span>
              </button>
            ) : (
              <button
                type="button"
                className="economy-settings-btn"
                onClick={() => setDelphiSetupOpen(true)}
                title="Start a Delphi discussion (blind submissions + anonymized aggregation)"
              >
                <span className="economy-settings-icon" aria-hidden="true">🔮</span>
                <span>Start Delphi Discussion</span>
              </button>
            )}
            <button
              type="button"
              className="economy-settings-btn"
              onClick={() => setEconomySettingsOpen(true)}
              title="Tune every economic constant live — saves to .vaak/economy.json, takes effect next tick"
            >
              <span className="economy-settings-icon" aria-hidden="true">⚙</span>
              <span>Economy Settings</span>
            </button>
          </div>
        )}

        {/* Decision Panel sidebar wrapper — places the panel into the right
            rail per architect msg 935. DecisionPanel.tsx renders the inner
            panel chrome (header + body); we wrap it in a section div so the
            float-right CSS at .decision-panel-section applies. */}
        {projectDir && project && (
          <div className="decision-panel-section rail-section">
            <DecisionPanel
              projectDir={projectDir}
              messages={project.messages}
              onPendingCountChange={setPendingDecisionCount}
              getRoleColor={getRoleColor}
            />
          </div>
        )}

        {/* Active Claims Section — auto-hide when empty per human msg 932
            (option B). Renders null when count===0 so the rail collapses to
            zero pixels when nothing is happening; reappears with the 150ms
            fade-in transition (collab.css §rail-section auto-hide) when a
            claim lands. The `claimsCollapsed` state still drives manual
            expand/collapse for non-empty state. */}
        {project && (() => {
          const claimsCount = project.claims?.length ?? 0;
          if (claimsCount === 0) return null;
          const collapsedEffective = claimsCollapsed ?? false;
          return (
          <CollapsibleSection
            id="claims-section"
            title="Active Claims"
            trailing={<span className="claims-section-count">({claimsCount})</span>}
            collapsed={collapsedEffective}
            onToggle={() => updateClaimsCollapsed(!collapsedEffective)}
            className="claims-section rail-section"
            headerTooltip={{ expand: "Expand active claims", collapse: "Collapse active claims" }}
          >
            <>
              {project.claims.map((claim: FileClaim) => {
                  const roleSlug = claim.role_instance.split(":")[0] || "";
                  const filesDisplay = claim.files.length > 2
                    ? `${claim.files[0]} (+${claim.files.length - 1} more)`
                    : claim.files.join(", ");
                  const aliveState = claim.alive_state;
                  const isStale = aliveState === "stale";
                  const isUnknown = aliveState === "unknown";
                  const dotClass = `claim-role-dot${isStale ? " stale" : ""}${isUnknown ? " unknown" : ""}`;
                  const labelSuffix = isStale ? " (reconnecting…)" : "";
                  const dotTitle = isStale
                    ? `${claim.role_instance} — last heartbeat stale (>120s); seat may be reconnecting or dead`
                    : isUnknown
                      ? `${claim.role_instance} — keepalive not yet observed; seat may be just-joined or pre-instrumentation`
                      : claim.role_instance;
                  return (
                    <div key={claim.role_instance} className="claim-card">
                      <div
                        className={dotClass}
                        style={{ background: getRoleColor(roleSlug) }}
                        title={dotTitle}
                        aria-label={dotTitle}
                      />
                      <span className="claim-role-label" style={{ color: getRoleColor(roleSlug) }}>
                        {claim.role_instance}{labelSuffix}
                      </span>
                      <div className="claim-info">
                        <div className="claim-files">{filesDisplay}</div>
                        <div className="claim-desc">{claim.description}</div>
                      </div>
                      <span className="claim-time">{formatRelativeTime(claim.claimed_at)}</span>
                    </div>
                  );
                })}
            </>
          </CollapsibleSection>
          );
        })()}

        {/* P1a Audience rail-section — auto-hide-empty per human msg 932 (B).
            count === 0 → render null. Mockup collab-layout-demo.html lines
            988-998. Count derives from sum of audiencePools.member_count;
            pools fetched lazily on first modal open, so this section only
            mounts after the human enables audience for a round. */}
        {(() => {
          const audienceMemberTotal = audiencePools.reduce((sum, p) => sum + (p.member_count ?? 0), 0);
          if (audienceMemberTotal === 0) return null;
          const collapsedEffective = audienceCollapsed ?? false;
          return (
          <CollapsibleSection
            id="audience-section"
            title="Audience"
            trailing={<span className="audience-section-count">({audienceMemberTotal})</span>}
            collapsed={collapsedEffective}
            onToggle={() => updateAudienceCollapsed(!collapsedEffective)}
            className="audience-section rail-section"
            headerTooltip={{ expand: "Expand audience section", collapse: "Collapse audience section" }}
          >
            <>
              {audiencePools.map((pool) => (
                <div key={pool.id} className="rail-item audience-section-pool">
                  <div className="rail-item-head">
                    <span className="rail-item-from">{pool.name}</span>
                    <span className="rail-item-meta">{pool.member_count} personas</span>
                  </div>
                </div>
              ))}
              <button
                type="button"
                className="rail-btn audience-section-open"
                onClick={() => setAudiencePanelOpen(true)}
              >
                Open Audience Panel
              </button>
            </>
          </CollapsibleSection>
          );
        })()}

        {/* P1b Replanning Requests rail-section — auto-hide-empty per human
            msg 932 (B). count === 0 → render null. Data source:
            twoControlsProtocol.floor.replanning_requests (same array
            AssemblyControls consumes; see useProtocolState.ts:43). */}
        {(() => {
          const replanningRequests = twoControlsProtocol?.floor?.replanning_requests ?? [];
          if (replanningRequests.length === 0) return null;
          const collapsedEffective = replanningCollapsed ?? false;
          return (
          <CollapsibleSection
            id="replanning-section"
            title="Replanning Requests"
            trailing={<span className="replanning-section-count">({replanningRequests.length})</span>}
            collapsed={collapsedEffective}
            onToggle={() => updateReplanningCollapsed(!collapsedEffective)}
            className="replanning-section rail-section"
            headerTooltip={{ expand: "Expand replanning requests", collapse: "Collapse replanning requests" }}
          >
            <>
              {replanningRequests.map((req, idx) => {
                const seatSlug = (req.seat ?? "").split(":")[0] || "";
                return (
                  <div key={`${req.seat ?? "anon"}-${idx}-${req.ts}`} className="rail-item replanning-section-item">
                    <div className="rail-item-head">
                      <span
                        className="rail-item-from"
                        style={{ color: seatSlug ? getRoleColor(seatSlug) : undefined }}
                      >
                        {req.seat ?? "unknown"}
                      </span>
                    </div>
                    {req.reason && <div className="rail-item-body">{req.reason}</div>}
                  </div>
                );
              })}
            </>
          </CollapsibleSection>
          );
        })()}

        </aside>
        {/* /P5-v2 Right Sidebar */}

        {/* Old Claude CLI banner replaced by Setup Checklist above roster */}

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
              // Wave 1.5 partial B1+B2+B3 per human msg 4264 priority #1:
              // derived values pulled from useMemo cache above, not recomputed per render.
              // IIFE still re-executes on parent re-render (keystroke etc.) but inner
              // O(N²) walks and Set allocations are cached. Full IIFE-to-useMemo lift
              // deferred to next focused work cycle.
              const activeCount = messageListDerivedCache.activeCount;
              const voteTallies = messageListDerivedCache.voteTallies;
              const voteProposalIds = messageListDerivedCache.voteProposalIds;
              const voteResponseIds = messageListDerivedCache.voteResponseIds;
              const totalCount = messageListDerivedCache.totalCount;
              const hasHiddenMessages = messageListDerivedCache.hasHiddenMessages;
              const visibleMessages = messageListDerivedCache.visibleMessages;

              // Phase 5 Layer 1 (human msg 1971): interleave per-turn currency
              // notices into the timeline "like the mic pass". Merge the Flow
              // Feed rows (already display-formatted in flowFeedRows) with the
              // board messages, sorted by ISO timestamp (lexical = chronological).
              // Gated on the inline-notices toggle AND currency being enabled.
              const inlineCurrencyOn =
                inlineCurrencyNotices && project?.config?.settings?.currency_enabled !== false;
              type TimelineItem =
                | { kind: "msg"; msg: BoardMessage; ts: string }
                | { kind: "cur"; line: { key: string; text: string; tier: CurrencyTier; seat?: string; at?: string }; ts: string };
              // Only SIGNIFICANT economic events interleave into the timeline
              // (human msg 2082: not "10 messages every turn about gold and
              // copper"). Routine earn/escrow-hold/passive/interest stay in the
              // sidebar Chitragupta feed; the timeline shows losses, disputes,
              // and pool-destroyed only.
              const inlineSignificantTiers = new Set<CurrencyTier>(["loss", "dispute", "destroyed"]);
              const timelineItems: TimelineItem[] = [
                ...visibleMessages.map((m): TimelineItem => ({ kind: "msg", msg: m, ts: m.timestamp })),
                ...(inlineCurrencyOn
                  ? flowFeedRows
                      .filter((l) => inlineSignificantTiers.has(l.tier))
                      .map((l): TimelineItem => ({ kind: "cur", line: l, ts: l.at ?? "" }))
                  : []),
              ].sort((a, b) => (a.ts < b.ts ? -1 : a.ts > b.ts ? 1 : 0));

              return (<>
              {hasHiddenMessages && (
                <button
                  className="load-earlier-btn"
                  onClick={() => setVisibleMsgLimit(prev => prev + MSG_PAGE_SIZE)}
                >
                  Load {Math.min(MSG_PAGE_SIZE, totalCount - visibleMsgLimit)} earlier messages ({totalCount - visibleMsgLimit} hidden)
                </button>
              )}
              {timelineItems.map((item) => {
              // Phase 5 Layer 1 — inline currency notice (mic-pass-style divider).
              if (item.kind === "cur") {
                const line = item.line;
                return (
                  <div
                    key={`cur-${line.key}`}
                    className={`currency-inline-notice currency-line--${line.tier}`}
                    role="note"
                  >
                    <span className="currency-inline-icon" aria-hidden="true">🪙</span>
                    <span
                      className="currency-inline-dot"
                      style={{ background: line.seat ? getRoleColor(line.seat.split(":")[0]) : "#888" }}
                      aria-hidden="true"
                    />
                    <span className="currency-inline-text">{line.text}</span>
                    <span className="currency-inline-time" title={line.at || ""}>
                      {line.at ? formatRelativeTime(line.at) : ""}
                    </span>
                  </div>
                );
              }
              const msg = item.msg;
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

              // Two-controls phase_toggled separator card — spec §72 (school-of-
              // fish visual transition). Full-width card marking the moment phase
              // flips between planning and execution. prefers-reduced-motion is
              // honored via @media query on the .phase-toggled-separator animation.
              //
              // Commit Q.B extends this card with a school-of-fish replanning
              // variant when reason starts with "replanning_accepted_by:" —
              // the card surfaces the requester (triggered_by) + the moderator
              // who accepted, per collaborative-proposal-workflow-spec-2026-
              // 05-15.md §Affordance C line 189.
              if (msg.type === "phase_toggled") {
                const newPhase = (msg.metadata?.new as string) ?? "execution";
                const oldPhase = (msg.metadata?.old as string) ?? "planning";
                const planPath = (msg.metadata?.plan_path as string | null) ?? null;
                const reason = (msg.metadata?.reason as string | null) ?? null;
                const triggeredBy = (msg.metadata?.triggered_by as string | null) ?? null;
                const toPlanning = newPhase === "planning";
                const isReplanningAccepted = reason?.startsWith("replanning_accepted_by:");
                const moderator = isReplanningAccepted
                  ? reason!.slice("replanning_accepted_by:".length)
                  : null;
                return (
                  <div
                    key={msg.id}
                    className={
                      `phase-toggled-separator${toPlanning ? " is-to-planning" : ""}` +
                      (isReplanningAccepted ? " is-replanning-accepted" : "")
                    }
                    role="separator"
                    aria-label={
                      isReplanningAccepted
                        ? `Replanning accepted by ${moderator}: phase changed from ${oldPhase} to ${newPhase}`
                        : `Phase changed from ${oldPhase} to ${newPhase}`
                    }
                  >
                    <span className="phase-toggled-icon" aria-hidden="true">
                      {toPlanning ? "✎" : "▷"}
                    </span>
                    <span className="phase-toggled-text">
                      {isReplanningAccepted ? (
                        <>
                          EXECUTION → PLANNING
                          {triggeredBy && (
                            <>
                              {" "}— replanning requested by{" "}
                              <strong>{triggeredBy}</strong>;{" "}
                            </>
                          )}
                          {!triggeredBy && " — "}
                          <strong>{moderator}</strong> accepted
                        </>
                      ) : (
                        <>
                          {toPlanning ? "PLANNING MODE" : "EXECUTION MODE"}
                          {planPath && !toPlanning && " — plan accepted:"}
                          {toPlanning && " — plan cleared"}
                        </>
                      )}
                    </span>
                    {planPath && !toPlanning && !isReplanningAccepted && (
                      <span className="phase-toggled-plan" title={planPath}>
                        {planPath.replace(/^.*\//, "")}
                      </span>
                    )}
                    <span className="phase-toggled-time" title={msg.timestamp}>
                      {formatRelativeTime(msg.timestamp)}
                    </span>
                  </div>
                );
              }

              // Commit S.A — review_intensity_changed separator card.
              // Renders when moderator/privileged adjusts the discipline level.
              if (msg.type === "review_intensity_changed") {
                const oldLevel = (msg.metadata?.old as number | null) ?? null;
                const newLevel = (msg.metadata?.new as number | null) ?? null;
                return (
                  <div
                    key={msg.id}
                    className="review-intensity-changed-card"
                    role="status"
                    aria-label={`Review intensity changed from ${oldLevel ?? '?'} to ${newLevel ?? '?'}`}
                  >
                    <span className="review-intensity-changed-icon" aria-hidden="true">🎚</span>
                    <span className="review-intensity-changed-text">
                      Review intensity: <strong>{oldLevel ?? '?'}</strong> → <strong>{newLevel ?? '?'}</strong>
                    </span>
                    <span className="review-intensity-changed-time" title={msg.timestamp}>
                      {formatRelativeTime(msg.timestamp)}
                    </span>
                  </div>
                );
              }

              // Commit A — planning_unattested warning badge (collab-proposal-
              // workflow-spec-2026-05-15.md §Extended-thinking attestation
              // lines 125-133). Compact one-line warning rendered in the
              // message stream below the originating message it correlates
              // to. Non-blocking surface — the originating message has
              // already landed; this is the team-visible "agent forgot
              // attestation" signal.
              if (msg.type === "planning_unattested") {
                const originatingId = (msg.metadata?.originating_message_id as number | null) ?? null;
                const originatingSeat = (msg.metadata?.originating_seat as string | null) ?? "unknown";
                return (
                  <div
                    key={msg.id}
                    className="planning-unattested-badge"
                    role="status"
                    aria-label={`Planning-phase message ${originatingId ?? ""} from ${originatingSeat} was sent without extended_thinking attestation`}
                  >
                    <span className="planning-unattested-icon" aria-hidden="true">⚠</span>
                    <span className="planning-unattested-text">
                      <strong>{originatingSeat}</strong> sent msg #{originatingId} without{" "}
                      <code>extended_thinking</code> attestation
                    </span>
                  </div>
                );
              }

              // Moderator-authority Item 5 follow-up — stale-moderator
              // recovery card. When the watchdog auto-promotes mic-passing
              // mode from moderator → rotation (reason: moderator_stale or
              // moderator_vacant), render a compact recovery notice so the
              // human knows the moderator slot was reclaimed by the team
              // safety net. Compact (no animation) since it's a recovery
              // event, not a school-of-fish state transition.
              if (msg.type === "mic_mechanism_promoted") {
                const fromMode = (msg.metadata?.from as string) ?? "moderator";
                const toMode = (msg.metadata?.to as string) ?? "rotation";
                const reason = (msg.metadata?.reason as string) ?? "";
                const reasonLabel =
                  reason === "moderator_stale" ? "moderator went stale" :
                  reason === "moderator_vacant" ? "moderator seat vacated" :
                  reason || "auto-recovery";
                return (
                  <div
                    key={msg.id}
                    className="mic-mechanism-promoted-card"
                    role="status"
                    aria-label={`Mic-passing mode auto-promoted from ${fromMode} to ${toMode}`}
                  >
                    <span className="mic-mechanism-promoted-icon" aria-hidden="true">⚙</span>
                    <span className="mic-mechanism-promoted-text">
                      Mic-passing auto-promoted: <strong>{fromMode}</strong> → <strong>{toMode}</strong> ({reasonLabel})
                    </span>
                    <span className="mic-mechanism-promoted-time" title={msg.timestamp}>
                      {formatRelativeTime(msg.timestamp)}
                    </span>
                  </div>
                );
              }

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
                    <MarkdownBody text={msg.body} className="discussion-event-body" />
                  </div>
                );
              }

              // Submissions render with a distinct visual style
              if (msg.type === "submission") {
                const fromRole = msg.from.split(":")[0];
                const { slug: fromSlug, instance: fromInstance } = parseSeatInstance(msg.from);
                const fromAvatarUrl = project?.config.roles[fromSlug]?.avatar_url || null;
                const fromTitle = project?.config.roles[fromSlug]?.title || fromSlug;
                return (
                  <div key={msg.id} className="submission-card" style={{ borderLeftColor: getRoleColor(fromRole) }}>
                    <div className="message-card-header">
                      <span className="message-card-id">#{msg.id}</span>
                      <Avatar slug={fromSlug} title={fromTitle} instance={fromInstance} avatarUrl={fromAvatarUrl} sizePx={20} className="message-card-avatar" />
                      <span className="message-card-from" style={{ color: getRoleColor(fromRole) }}>{msg.from}</span>
                      <span className="message-card-arrow">&rarr;</span>
                      <span className="message-card-to" style={{ color: getRoleColor(msg.to) }}>{msg.to}</span>
                      <span className="message-type-badge badge-submission">submission</span>
                      <span className="message-card-time" title={msg.timestamp}>{formatRelativeTime(msg.timestamp)}</span>
                      <button className="message-play-btn" onClick={(e) => { e.stopPropagation(); playMessage(msg.id, `${msg.subject || ""}. ${msg.body}`, fromRole); }} title={playingMsgId === msg.id ? "Stop" : "Play"}>{playingMsgId === msg.id ? "\u23F9" : "\u25B6"}</button>
                    </div>
                    {msg.subject && <div className="message-card-subject">{msg.subject}</div>}
                    <MarkdownBody text={msg.body} className="message-card-body" />
                  </div>
                );
              }

              // Interactive question card for human-targeted questions with choices
              if (msg.to === "human" && msg.type === "question" && msg.metadata?.choices?.length) {
                const answered = getAnswerForQuestion(msg.id, answerLookup);
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

              // Compact single-line divider for system mic events (human msg 2975).
              // Body (which contains the rotation strip) collapses behind <details>
              // disclosure — visible on expand for debugging. All data stays in
              // msg.body / msg.metadata; only render compacts.
              if (msg.type === "mic_landed") {
                const targetRole = msg.to.split(":")[0];
                const targetColor = getRoleColor(targetRole);
                const prevTurnType = typeof msg.metadata?.prev_turn_type === "string"
                  ? (msg.metadata.prev_turn_type as string)
                  : null;
                const prevSecs = typeof msg.metadata?.prev_expected_duration_secs === "number"
                  ? (msg.metadata.prev_expected_duration_secs as number)
                  : null;
                const prevDisplay = prevSecs !== null
                  ? prevSecs >= 60 ? `~${Math.round(prevSecs / 60)}min` : `~${prevSecs}s`
                  : "";
                const prevLabel = prevTurnType ? prevTurnType.charAt(0).toUpperCase() + prevTurnType.slice(1) : null;
                const prevSpeaker = typeof msg.metadata?.from_speaker === "string"
                  ? (msg.metadata.from_speaker as string)
                  : null;
                const floorSecs = typeof msg.metadata?.floor_time_seconds === "number"
                  ? (msg.metadata.floor_time_seconds as number)
                  : null;
                return (
                  <details
                    key={msg.id}
                    className="mic-event-divider mic-landed-divider"
                    style={{ borderLeftColor: targetColor }}
                  >
                    <summary title={msg.body || msg.timestamp}>
                      <span className="mic-event-icon" aria-hidden="true">🎙</span>
                      <span className="mic-event-label">→</span>
                      <span className="mic-event-target" style={{ color: targetColor }}>{msg.to}</span>
                      {floorSecs !== null && <span className="mic-event-meta">{floorSecs}s</span>}
                      {prevTurnType && (
                        <span className={`mic-event-prev turn-type-${prevTurnType}`}>
                          prev{prevSpeaker ? ` ${prevSpeaker}` : ""} {prevLabel}{prevDisplay ? ` ${prevDisplay}` : ""}
                        </span>
                      )}
                      <span className="mic-event-time" title={msg.timestamp}>{formatRelativeTime(msg.timestamp)}</span>
                    </summary>
                    {msg.body && <MarkdownBody text={msg.body} className="mic-event-body" />}
                  </details>
                );
              }

              if (msg.type === "mic_released") {
                const fromSpeaker = typeof msg.metadata?.from_speaker === "string"
                  ? (msg.metadata.from_speaker as string)
                  : msg.from;
                const fromRole = fromSpeaker.includes(":") ? fromSpeaker.split(":")[0] : fromSpeaker;
                const fromColor = getRoleColor(fromRole);
                const reason = typeof msg.metadata?.reason === "string" ? (msg.metadata.reason as string) : null;
                const idleSecs = typeof msg.metadata?.idle_secs === "number" ? (msg.metadata.idle_secs as number) : null;
                const reasonLabel = reason === "floor_stall" ? "stalled"
                  : reason === "max_floor_exceeded" ? "max floor"
                  : reason === "stall_threshold_exceeded" ? "stalled"
                  : reason ?? "released";
                return (
                  <details
                    key={msg.id}
                    className="mic-event-divider mic-released-divider"
                    style={{ borderLeftColor: fromColor }}
                  >
                    <summary title={msg.body || msg.timestamp}>
                      <span className="mic-event-icon" aria-hidden="true">🔇</span>
                      <span className="mic-event-label">released from</span>
                      <span className="mic-event-target" style={{ color: fromColor }}>{fromSpeaker}</span>
                      <span className="mic-event-meta mic-event-reason">{reasonLabel}{idleSecs !== null ? ` (${idleSecs}s idle)` : ""}</span>
                      <span className="mic-event-time" title={msg.timestamp}>{formatRelativeTime(msg.timestamp)}</span>
                    </summary>
                    {msg.body && <MarkdownBody text={msg.body} className="mic-event-body" />}
                  </details>
                );
              }

              // SHA-9: system-message visual distinction (human msg 1192 ask A).
              // System events aren't from a person \u2014 render with neutral
              // styling, no avatar, italic-muted body, "SYSTEM" pill instead of
              // a role name. Catches Oxford broadcasts (initiate/declare/end),
              // system buzz (SHA-5.3a reset-soft rejoin prompt), and any other
              // non-role-author event not already handled above (mic_landed /
              // mic_released have their own dedicated dividers).
              if (msg.from === "system" || msg.from.startsWith("system:")) {
                const systemTag = msg.from.includes(":") ? msg.from.split(":").slice(1).join(":") : null;
                // SHA-9.2: event-severity hierarchy (dev-challenger msg 1289
                // find D + architect msg 1291). Without tiering, an
                // oxford_speaker_declared directive (next-debater take the
                // floor \u2014 CRITICAL) renders identically to a keepalive tick
                // (INFORMATIONAL). Classify and assign a severity modifier
                // class so each tier gets distinct visual weight:
                //  - critical: bright amber accent + heavier border, demands
                //    eye attention (directives, speaker declarations)
                //  - state-change: blue accent, normal weight (initiations,
                //    endings, mic releases, resets \u2014 situational awareness)
                //  - informational: current muted-gray treatment (everything
                //    else \u2014 fallback)
                const subj = msg.subject ?? "";
                let severity: "critical" | "state-change" | "informational";
                if (
                  msg.type === "directive" ||
                  subj.includes("OxfordSpeakerDeclared") ||
                  subj.includes("OxfordDebateAssignment") ||
                  subj.includes("OxfordModeratorPrompt") ||
                  subj.includes("YOUR TURN")
                ) {
                  severity = "critical";
                } else if (
                  subj.includes("OxfordDebateInitiated") ||
                  subj.includes("OxfordDebateEnded") ||
                  subj.includes("OxfordTurnAutoYielded") ||
                  subj.includes("OxfordAutoOpened") ||
                  subj.includes("mic_released") ||
                  subj.includes("Soft reset")
                ) {
                  severity = "state-change";
                } else {
                  severity = "informational";
                }
                return (
                  <div key={msg.id} className={`system-event-card system-event-card-${severity}`}>
                    <div className="message-card-header">
                      <span className="message-card-id">#{msg.id}</span>
                      <span className={`system-event-pill system-event-pill-${severity}`} aria-label={`System event (${severity})`}>SYSTEM</span>
                      {systemTag && <span className="system-event-tag">{systemTag}</span>}
                      <span className="message-card-time" title={msg.timestamp}>{formatRelativeTime(msg.timestamp)}</span>
                      <button className="message-delete-btn" onClick={(e) => { e.stopPropagation(); handleDeleteMessage(msg.id); }} title="Delete message">&times;</button>
                    </div>
                    {msg.subject && <div className="system-event-subject">{msg.subject}</div>}
                    <MarkdownBody text={msg.body} className="system-event-body" />
                  </div>
                );
              }

              const fromRole = msg.from.split(":")[0];
              const borderColor = getRoleColor(fromRole);
              const { slug: fromSlug, instance: fromInstance } = parseSeatInstance(msg.from);
              const fromAvatarUrl = project?.config.roles[fromSlug]?.avatar_url || null;
              const fromTitle = project?.config.roles[fromSlug]?.title || fromSlug;
              return (
                <div
                  key={msg.id}
                  className="message-card"
                  style={{ borderLeftColor: borderColor }}
                >
                  <div className="message-card-header">
                    <span className="message-card-id">#{msg.id}</span>
                    <Avatar slug={fromSlug} title={fromTitle} instance={fromInstance} avatarUrl={fromAvatarUrl} sizePx={20} className="message-card-avatar" />
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
                  <MarkdownBody text={msg.body} className="message-card-body" />
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
                  {/* Change #1 (human msg 2262): per-message economic footer.
                      The real economic story is told here — on the message that
                      caused it — not in a separate sidebar. Human messages earn
                      no currency, so this is naturally null for them. */}
                  {msg.from !== "human" && (() => {
                    const econ = buildMsgEconomy(
                      economicByMsg.get(String(msg.id)),
                      disputeByMsg.get(String(msg.id)),
                    );
                    if (!econ) return null;
                    return (
                      <div className="message-econ-footer" role="note" aria-label="Economic activity for this message">
                        {econ.map((chip, i) => (
                          <span key={i} className={`econ-chip currency-line--${chip.tier}`}>{chip.text}</span>
                        ))}
                      </div>
                    );
                  })()}
                </div>
              );
            })}</>);
            })()
          )}
          <div ref={messagesEndCallbackRef} />
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
          {/* Change E (CollabTab restructure spec, architect msg 5238/5249/5259):
              human msg 5237 "we need to make the text box slightly bigger and
              that's only possible if we clean up a bunch of the UI consolidating
              things into tabs etc" — converted from single-line <input> to
              multi-line <textarea rows=3> so 3 lines of message text fit before
              scrolling. Enter still sends; Shift+Enter inserts a newline for
              multi-paragraph messages. Padding bumped 6px → 8px and font-size
              13px → 14px for the slightly-bigger feel without dominating the
              vertical space. */}
          <textarea
            className="compose-input compose-input-textarea"
            rows={3}
            value={msgBody}
            onChange={(e) => {
              setMsgBody(e.target.value);
              // Reset confirmation if the body changes — user may have
              // edited away the mic_to mention. They must confirm again.
              if (micToConfirmed) setMicToConfirmed(null);
              if (micToHintDismissed) setMicToHintDismissed(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                sendMessage();
              }
            }}
            placeholder={`Message${activeSection ? ` in #${sections.find(s => s.slug === activeSection)?.name || activeSection}` : ""}... (Enter to send · Shift+Enter for newline · /debate delphi [topic])`}
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

        {/* Slice 4 \u2014 mic_to hint UI. Spec \u00a74.3 click-to-confirm. */}
        {micToCandidate && !micToConfirmed && (
          <MicToHint
            candidate={micToCandidate}
            onConfirm={(seat) => setMicToConfirmed(seat)}
            onDismiss={() => setMicToHintDismissed(true)}
          />
        )}
        {micToConfirmed && (
          <div className="mic-to-hint mic-to-hint--confirmed" role="note">
            <span>\u2713 Will pass mic to <b>{micToConfirmed}</b> on send</span>
            <button type="button" onClick={() => setMicToConfirmed(null)} aria-label="Cancel mic transfer">\u00d7</button>
          </div>
        )}

        {/* Economy Settings modal (human msg 657 — live-tunable constants) */}
        <EconomySettingsModal
          open={economySettingsOpen}
          projectDir={projectDir || ""}
          onClose={() => setEconomySettingsOpen(false)}
        />

        {/* Oxford-debate setup modal (human msg 706 — Phase A UI trigger).
            onStarted: human msg 1090/1092 — surface a toast + seed activeOxford
            immediately on initiate so the UI gives same-tick feedback instead
            of waiting on the 2s poll tick (which made the Start click feel
            like nothing happened — "Still no UI changes" failure). */}
        <OxfordSetupModal
          open={oxfordSetupOpen}
          projectDir={projectDir || ""}
          activeSeats={
            (project?.sessions ?? [])
              .filter((b: SessionBinding) => b.status === "active" && !!b.role && b.role !== "human")
              .map((b: SessionBinding) => `${b.role}:${b.instance ?? 0}`)
          }
          onClose={() => setOxfordSetupOpen(false)}
          onStarted={(d) => {
            setActiveOxford({
              debate_id: d.debate_id,
              moderator: d.moderator,
              premise: d.premise,
              side_a: d.side_a,
              side_b: d.side_b,
              current_speaker: null,
              turn_count: 0,
              // SHA-10.5 — modal-side initial state populates phase
              // optimistically as opening_a (matching SHA-10.2 backend
              // auto-declare). Poll-side will overwrite within 2s with
              // the canonical value from active-oxford-debate.json.
              phase: "opening_a",
              phase_started_at: new Date().toISOString(),
            });
            showToast(
              `Oxford debate ${d.debate_id} started — moderator ${d.moderator}, ${d.side_a.length} vs ${d.side_b.length}.`,
              "success",
            );
          }}
        />

        {/* Delphi discussion setup modal (human msg 1939 — parity build). */}
        <DelphiSetupModal
          open={delphiSetupOpen}
          projectDir={projectDir || ""}
          activeSeats={
            (project?.sessions ?? [])
              .filter((b: SessionBinding) => b.status === "active" && !!b.role && b.role !== "human")
              .map((b: SessionBinding) => `${b.role}:${b.instance ?? 0}`)
          }
          onClose={() => setDelphiSetupOpen(false)}
          onStarted={(d) => {
            const nowIso = new Date().toISOString();
            setActiveDelphi({
              discussion_id: d.discussion_id,
              moderator: d.moderator,
              topic: d.topic,
              participants: d.participants,
              audience: [],
              max_rounds: d.max_rounds,
              convergence_criterion: "moderator",
              convergence_reward_copper: 0,
              phase: "opening",
              current_round: 0,
              phase_started_at: nowIso,
              blind_gate_active: false,
              blind_gate_strict: true,
              started_at: nowIso,
              submission_soft_floor_secs: DELPHI_DEFAULTS.SUBMISSION_SOFT_FLOOR_SECS,
              submission_hard_floor_secs: DELPHI_DEFAULTS.SUBMISSION_HARD_FLOOR_SECS,
              review_floor_secs: DELPHI_DEFAULTS.REVIEW_FLOOR_SECS,
              rounds: [],
            });
            showToast(
              `Delphi discussion ${d.discussion_id} started — moderator ${d.moderator}, ${d.participants.length} participants, ${d.max_rounds} rounds max.`,
              "success",
            );
          }}
        />

        {/* Human balance-adjust modal (replaces window.prompt per ui-arch msg 626) */}
        <AdjustBalanceModal
          open={adjustTarget !== null}
          seat={adjustTarget?.seat ?? ""}
          direction={adjustTarget?.direction ?? "credit"}
          onClose={() => setAdjustTarget(null)}
          onSubmit={async (amountCopper, reason) => {
            if (!adjustTarget) return;
            const { invoke } = await import("@tauri-apps/api/core");
            await invoke("currency_human_adjust_cmd", {
              dir: projectDir,
              seat: adjustTarget.seat,
              amountCopper,
              reason,
            });
          }}
        />

        {/* Role Briefing Modal */}
        {selectedRole && (
          <RoleBriefingModal
            projectDir={projectDir}
            roleSlug={selectedRole.slug}
            roleTitle={selectedRole.title}
            roleColor={getRoleColor(selectedRole.slug)}
            avatarUrl={project?.config.roles[selectedRole.slug]?.avatar_url || null}
            stats={project?.config.roles[selectedRole.slug]?.stats}
            // RoleStatus is role-level, not instance-specific — omit instance prop
            // so Avatar uses role-definition alt-text branch per F-EA-VACANT-SENTINEL-CLASS.
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

                {/* Step: Stats — character/stats system per spec §3 */}
                {WIZARD_STEPS[roleFormStep] === "Stats" && (
                  <div className="wizard-step-content">
                    <h3 className="wizard-step-heading">Cognitive budget</h3>
                    <p className="wizard-step-hint">
                      Six axes shape how this role engages with work. Each axis is 1–10. A score of 5 means "not your primary focus — flag for a specialist." A 9–10 means "your strongest voice." Calibration is empirical, not aspirational — what does this role actually do best?
                    </p>
                    <div className="role-form-field">
                      <label className="role-form-label">Avatar URL (HTTPS, optional)</label>
                      <input
                        className="role-form-input"
                        type="url"
                        value={roleFormAvatarUrl}
                        onChange={(e) => setRoleFormAvatarUrl(e.target.value)}
                        placeholder="https://example.com/avatar.png"
                        disabled={roleFormSaving}
                      />
                      <span className="role-form-hint">Falls back to the role's color initial when blank or load fails.</span>
                    </div>
                    <div className="stats-grid">
                      {STAT_AXES.map(({ key, label, short, hint }) => (
                        <div key={key} className="stat-row">
                          <div className="stat-row-header">
                            <span className="stat-row-short">{short}</span>
                            <span className="stat-row-label">{label}</span>
                            <span className="stat-row-value" aria-live="polite">{roleFormStats[key]}</span>
                          </div>
                          <input
                            type="range"
                            className="stat-row-slider"
                            min={1}
                            max={10}
                            step={1}
                            value={roleFormStats[key]}
                            onChange={(e) => setRoleFormStats({ ...roleFormStats, [key]: parseInt(e.target.value, 10) })}
                            disabled={roleFormSaving}
                            aria-label={`${label}: ${roleFormStats[key]} of 10`}
                            aria-valuetext={`${roleFormStats[key]} of 10 — ${hint}`}
                          />
                          <span className="stat-row-hint">{hint}</span>
                        </div>
                      ))}
                    </div>
                    {project && (
                      <details className="stats-compare">
                        <summary>Compare with team</summary>
                        <table className="stats-compare-table">
                          <thead>
                            <tr>
                              <th>Role</th>
                              {STAT_AXES.map(a => <th key={a.key} title={a.label}>{a.short}</th>)}
                            </tr>
                          </thead>
                          <tbody>
                            {Object.entries(project.config.roles)
                              .filter(([, role]) => (role as any).stats)
                              .sort(([a], [b]) => a.localeCompare(b))
                              .map(([slug, role]) => {
                                const s = (role as any).stats as RoleStats;
                                return (
                                  <tr key={slug}>
                                    <td className="stats-compare-name">{role.title}</td>
                                    {STAT_AXES.map(a => <td key={a.key}>{s[a.key]}</td>)}
                                  </tr>
                                );
                              })}
                            <tr className="stats-compare-current">
                              <td className="stats-compare-name">This role</td>
                              {STAT_AXES.map(a => <td key={a.key}>{roleFormStats[a.key]}</td>)}
                            </tr>
                          </tbody>
                        </table>
                      </details>
                    )}
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
