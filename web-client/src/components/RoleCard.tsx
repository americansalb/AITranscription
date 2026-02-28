/**
 * RoleCard — displays a team role with status, provider selector, and controls.
 * Shared between project page and role management views.
 * Built with accessibility from day one: keyboard navigable, screen reader friendly.
 */

import type { RoleConfig } from "../lib/api";

const ROLE_COLORS: Record<string, string> = {
  manager: "var(--role-manager)",
  architect: "var(--role-architect)",
  developer: "var(--role-developer)",
  tester: "var(--role-tester)",
};

// FNV-1a hash for deterministic custom role colors (matches desktop)
const HASH_PALETTE = [
  "#e91e63", "#00bcd4", "#ff7043", "#8bc34a",
  "#7e57c2", "#26a69a", "#ec407a", "#42a5f5",
  "#ffa726", "#66bb6a", "#ef5350", "#ab47bc",
];

function hashSlug(slug: string): number {
  let hash = 2166136261;
  for (let i = 0; i < slug.length; i++) {
    hash ^= slug.charCodeAt(i);
    hash = Math.imul(hash, 16777619) & 0xffffffff;
  }
  return hash >>> 0;
}

export function getRoleColor(slug: string): string {
  const base = slug.split(":")[0];
  if (ROLE_COLORS[base]) return ROLE_COLORS[base];
  for (const [prefix, color] of Object.entries(ROLE_COLORS)) {
    if (base.startsWith(prefix)) return color;
  }
  return HASH_PALETTE[hashSlug(base) % HASH_PALETTE.length];
}

interface RoleCardProps {
  slug: string;
  role: RoleConfig;
  status?: "working" | "ready" | "vacant";
  onProviderChange?: (provider: string, model: string) => void;
  onStart?: () => void;
  onStop?: () => void;
  compact?: boolean;
}

const PROVIDER_MODELS: Record<string, Array<{ id: string; label: string }>> = {
  anthropic: [
    { id: "claude-opus-4-6", label: "Opus 4.6" },
    { id: "claude-sonnet-4-6", label: "Sonnet 4.6" },
    { id: "claude-haiku-4-5-20251001", label: "Haiku 4.5" },
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

export function RoleCard({ slug, role, status = "vacant", onProviderChange, onStart, onStop, compact }: RoleCardProps) {
  const color = getRoleColor(slug);
  const currentProvider = role.provider?.provider || "anthropic";
  const currentModel = role.provider?.model || "";
  const isRunning = status === "working" || status === "ready";

  return (
    <div
      className="card"
      style={{
        borderLeft: `3px solid ${color}`,
        padding: compact ? "var(--space-2) var(--space-3)" : "var(--space-3)",
      }}
      role="group"
      aria-label={`${role.title} role — ${status}`}
    >
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "var(--space-2)",
      }}>
        <div style={{ display: "flex", alignItems: "center", gap: "var(--space-2)" }}>
          <div className={`status-dot ${status}`} aria-label={`Status: ${status}`} />
          <span style={{
            fontWeight: "var(--weight-medium)",
            fontSize: compact ? "var(--text-sm)" : "var(--text-base)",
          }}>
            {role.title}
          </span>
        </div>

        <div style={{ display: "flex", gap: "var(--space-1)" }}>
          {isRunning ? (
            onStop && (
              <button
                className="btn btn-ghost"
                style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)", color: "var(--error)" }}
                onClick={onStop}
                aria-label={`Stop ${role.title} agent`}
              >
                Stop
              </button>
            )
          ) : (
            onStart && (
              <button
                className="btn btn-ghost"
                style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)", color: "var(--success)" }}
                onClick={onStart}
                aria-label={`Start ${role.title} agent`}
              >
                Start
              </button>
            )
          )}
        </div>
      </div>

      {!compact && role.description && (
        <div style={{
          fontSize: "var(--text-xs)",
          color: "var(--text-muted)",
          marginTop: "var(--space-1)",
          lineHeight: "var(--line-relaxed)",
        }}>
          {role.description}
        </div>
      )}

      {/* Provider selector */}
      {onProviderChange && (
        <div style={{
          marginTop: "var(--space-2)",
          display: "flex",
          gap: "var(--space-1)",
        }}>
          <select
            className="input"
            style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)", flex: 1 }}
            value={currentProvider}
            onChange={(e) => {
              const provider = e.target.value;
              const models = PROVIDER_MODELS[provider];
              onProviderChange(provider, models?.[0]?.id || "");
            }}
            aria-label={`AI provider for ${role.title}`}
          >
            <option value="anthropic">Claude</option>
            <option value="openai">GPT</option>
            <option value="google">Gemini</option>
          </select>
          <select
            className="input"
            style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)", flex: 1 }}
            value={currentModel}
            onChange={(e) => {
              onProviderChange(currentProvider, e.target.value);
            }}
            aria-label={`Model for ${role.title}`}
          >
            {(PROVIDER_MODELS[currentProvider] || []).map((m) => (
              <option key={m.id} value={m.id}>{m.label}</option>
            ))}
          </select>
        </div>
      )}

      {/* Tags */}
      {!compact && role.tags && role.tags.length > 0 && (
        <div style={{
          display: "flex",
          flexWrap: "wrap",
          gap: "var(--space-1)",
          marginTop: "var(--space-2)",
        }}>
          {role.tags.map((tag) => (
            <span key={tag} className="badge badge-accent">{tag}</span>
          ))}
        </div>
      )}
    </div>
  );
}
