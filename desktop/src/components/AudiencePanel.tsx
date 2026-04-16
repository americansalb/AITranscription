/**
 * AudiencePanel.tsx — Audience pool configuration & live status panel
 *
 * Renders inline within the discussion controls area of CollabTab.
 * Two modes:
 *   - Setup mode (before/between discussions): pool selection, persona config, cost estimate
 *   - Live mode (during active discussion): persona status chips, audience state indicator
 *
 * Calls Tauri commands: list_audience_pools, get_audience_pool, save_audience_pool, delete_audience_pool
 */

import { useState, useEffect, useCallback } from "react";

// ==================== Types ====================

export interface AudiencePersona {
  id: string;           // unique within pool, e.g. "senior-backend-dev"
  name: string;         // display name, e.g. "Senior Backend Dev"
  background: string;   // 1-2 sentence persona background
  provider: "anthropic" | "openai" | "groq";
  model: string;        // e.g. "claude-haiku-4-5-20251001", "gpt-4o-mini", "llama-3.3-70b"
  values?: string | string[];  // core values — string (comma-separated) or array
  style?: string;       // communication style, e.g. "direct and technical"
}

export interface AudiencePool {
  id: string;           // slug, e.g. "software-dev", "custom-1"
  name: string;         // display name
  description: string;  // short description
  builtin: boolean;     // true for presets (general, software-dev)
  personas: AudiencePersona[];
}

export interface AudiencePoolMeta {
  id: string;
  name: string;
  description: string;
  builtin: boolean;
  persona_count: number;
}

/** Live status of a persona during an active discussion */
export type PersonaStatus = "listening" | "thinking" | "voted" | "responded" | "error";

export interface LivePersonaState {
  persona_id: string;
  status: PersonaStatus;
  last_message_id?: number;
}

/** Provider config for cost estimation */
interface ProviderInfo {
  key: "anthropic" | "openai" | "groq";
  label: string;
  color: string;
  icon: string;
  available: boolean;          // true if API key is configured
  cost_per_1k_tokens: number;  // rough estimate for cost display
}

// ==================== Constants ====================

const PROVIDERS: ProviderInfo[] = [
  { key: "anthropic", label: "Anthropic", color: "#d4a574", icon: "A", available: false, cost_per_1k_tokens: 0.00025 },
  { key: "openai",    label: "OpenAI",    color: "#74aa9c", icon: "O", available: false, cost_per_1k_tokens: 0.00015 },
  { key: "groq",      label: "Groq",      color: "#f55036", icon: "G", available: false, cost_per_1k_tokens: 0.00005 },
];

const COST_LABELS = ["$", "$$", "$$$"];

function getCostTier(provider: string): number {
  if (provider === "groq") return 0;
  if (provider === "openai") return 1;
  return 2;
}

function estimateRoundCost(personas: AudiencePersona[], avgTokensPerRound: number = 500): string {
  let total = 0;
  for (const p of personas) {
    const info = PROVIDERS.find(pr => pr.key === p.provider);
    if (info) total += info.cost_per_1k_tokens * (avgTokensPerRound / 1000);
  }
  if (total < 0.01) return "<$0.01";
  return `~$${total.toFixed(2)}`;
}

// ==================== Props ====================

interface AudiencePanelProps {
  /** Whether a discussion is currently active */
  discussionActive: boolean;
  /** Current audience gating state from discussion.json */
  audienceState: string;  // "listening" | "voting" | "qa" | "commenting" | "open"
  /** Project directory path for Tauri commands */
  projectDir: string;
  /** Live persona statuses (updated by parent from board polling) */
  liveStatuses?: LivePersonaState[];
  /** Callback when audience size changes (for parent to track) */
  onPoolChange?: (pool: AudiencePool | null) => void;
}

// ==================== Component ====================

export function AudiencePanel({
  discussionActive,
  audienceState,
  projectDir,
  liveStatuses = [],
  onPoolChange,
}: AudiencePanelProps) {
  // Panel state
  const [expanded, setExpanded] = useState(false);
  const [pools, setPools] = useState<AudiencePoolMeta[]>([]);
  const [selectedPoolId, setSelectedPoolId] = useState<string>("general");
  const [activePool, setActivePool] = useState<AudiencePool | null>(null);
  const [audienceSize, setAudienceSize] = useState(5);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Modal state for pool editor
  const [editorOpen, setEditorOpen] = useState(false);
  const [editingPool, setEditingPool] = useState<AudiencePool | null>(null);
  const [editingPersonaIdx, setEditingPersonaIdx] = useState<number>(0);

  // Provider availability (checked on mount)
  // Provider availability — will be populated when we add API key detection
  const [providerAvail] = useState<Record<string, boolean>>({
    anthropic: false, openai: false, groq: false,
  });

  // ---- Tauri command wrappers ----

  const invokeCmd = useCallback(async (cmd: string, args: Record<string, unknown> = {}) => {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke(cmd, { projectDir, ...args });
  }, [projectDir]);

  // Load pool list on mount
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        setLoading(true);
        const list = await invokeCmd("list_audience_pools") as AudiencePoolMeta[];
        if (!cancelled) setPools(list);
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [invokeCmd]);

  // Load selected pool details when selection changes
  useEffect(() => {
    if (!selectedPoolId) return;
    let cancelled = false;
    (async () => {
      try {
        const pool = await invokeCmd("get_audience_pool", { poolId: selectedPoolId }) as AudiencePool;
        if (!cancelled) {
          setActivePool(pool);
          onPoolChange?.(pool);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => { cancelled = true; };
  }, [selectedPoolId, invokeCmd, onPoolChange]);

  // ---- Handlers ----

  const handleSizeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setAudienceSize(Number(e.target.value));
  };

  const handleSavePool = async () => {
    if (!editingPool) return;
    try {
      await invokeCmd("save_audience_pool", {
        poolId: editingPool.id,
        pool: JSON.stringify(editingPool),
      });
      setEditorOpen(false);
      // Refresh list
      const list = await invokeCmd("list_audience_pools") as AudiencePoolMeta[];
      setPools(list);
      setSelectedPoolId(editingPool.id);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleDeletePool = async (poolId: string) => {
    try {
      await invokeCmd("delete_audience_pool", { poolId });
      const list = await invokeCmd("list_audience_pools") as AudiencePoolMeta[];
      setPools(list);
      if (selectedPoolId === poolId) {
        setSelectedPoolId(list[0]?.id || "");
        setActivePool(null);
      }
    } catch (e) {
      setError(String(e));
    }
  };

  const handleAddPersona = () => {
    if (!editingPool) return;
    const newPersona: AudiencePersona = {
      id: `persona-${Date.now()}`,
      name: "New Persona",
      background: "",
      provider: "groq",
      model: "llama-3.3-70b-versatile",
    };
    setEditingPool({
      ...editingPool,
      personas: [...editingPool.personas, newPersona],
    });
    setEditingPersonaIdx(editingPool.personas.length);
  };

  const handleRemovePersona = (idx: number) => {
    if (!editingPool) return;
    const updated = editingPool.personas.filter((_, i) => i !== idx);
    setEditingPool({ ...editingPool, personas: updated });
    if (editingPersonaIdx >= updated.length) {
      setEditingPersonaIdx(Math.max(0, updated.length - 1));
    }
  };

  const handleUpdatePersona = (idx: number, field: keyof AudiencePersona, value: string) => {
    if (!editingPool) return;
    const updated = [...editingPool.personas];
    updated[idx] = { ...updated[idx], [field]: value };
    setEditingPool({ ...editingPool, personas: updated });
  };

  // ---- Audience state badge ----

  const AUDIENCE_STATE_LABELS: Record<string, { label: string; color: string }> = {
    listening: { label: "Silent", color: "var(--collab-text-muted)" },
    voting:    { label: "Voting", color: "var(--collab-warning)" },
    qa:        { label: "Q&A", color: "var(--collab-accent)" },
    commenting:{ label: "Commenting", color: "var(--collab-success)" },
    open:      { label: "Open Floor", color: "#22d3ee" },
  };

  const stateInfo = AUDIENCE_STATE_LABELS[audienceState] || AUDIENCE_STATE_LABELS.listening;

  // ---- Active personas (sliced to audienceSize) ----

  const activePersonas = activePool?.personas.slice(0, audienceSize) || [];

  // ---- Render ----

  return (
    <div className="audience-panel" role="region" aria-label="Audience pool configuration">
      {/* Header — always visible */}
      <button
        className="audience-panel-header"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        aria-controls="audience-panel-body"
      >
        <span className="audience-panel-icon">&#x1F465;</span>
        <span className="audience-panel-title">
          Audience
          {activePool && (
            <span className="audience-panel-count">
              {audienceSize} / {activePool.personas.length}
            </span>
          )}
        </span>
        {discussionActive && (
          <span
            className="audience-state-badge"
            style={{ color: stateInfo.color, borderColor: stateInfo.color }}
          >
            {stateInfo.label}
          </span>
        )}
        <span className="audience-panel-cost">
          {activePersonas.length > 0 ? `${estimateRoundCost(activePersonas)}/round` : ""}
        </span>
        <span className={`audience-panel-chevron ${expanded ? "expanded" : ""}`}>
          &#x25B6;
        </span>
      </button>

      {/* Body — expandable */}
      {expanded && (
        <div className="audience-panel-body" id="audience-panel-body">
          {error && (
            <div className="audience-panel-error" role="alert">
              {error}
              <button onClick={() => setError(null)} aria-label="Dismiss error">&times;</button>
            </div>
          )}

          {/* Pool selector + size slider */}
          <div className="audience-config-row">
            <label className="audience-config-label">
              Pool
              <select
                className="audience-config-select"
                value={selectedPoolId}
                onChange={(e) => setSelectedPoolId(e.target.value)}
                disabled={discussionActive}
              >
                {pools.map(p => (
                  <option key={p.id} value={p.id}>
                    {p.name} ({p.persona_count})
                  </option>
                ))}
              </select>
            </label>

            <label className="audience-config-label">
              Size: {audienceSize}
              <input
                type="range"
                className="audience-size-slider"
                min={1}
                max={activePool?.personas.length || 25}
                value={audienceSize}
                onChange={handleSizeChange}
                disabled={discussionActive}
              />
            </label>

            {!discussionActive && (
              <button
                className="audience-customize-btn"
                onClick={() => {
                  setEditingPool(activePool ? { ...activePool, personas: [...activePool.personas] } : null);
                  setEditingPersonaIdx(0);
                  setEditorOpen(true);
                }}
                disabled={!activePool}
              >
                Customize
              </button>
            )}
          </div>

          {/* Provider toggles */}
          <div className="audience-providers">
            {PROVIDERS.map(p => (
              <span
                key={p.key}
                className={`audience-provider-badge ${providerAvail[p.key] ? "available" : "unavailable"}`}
                style={{ borderColor: p.color }}
                title={`${p.label}: ${providerAvail[p.key] ? "API key configured" : "No API key"}`}
              >
                <span className="audience-provider-icon" style={{ color: p.color }}>{p.icon}</span>
                <span className="audience-provider-dot" style={{ background: providerAvail[p.key] ? "#22c55e" : "#71717a" }} />
              </span>
            ))}
          </div>

          {/* Persona chips */}
          <div className="audience-persona-chips">
            {activePersonas.map((persona) => {
              const live = liveStatuses.find(ls => ls.persona_id === persona.id);
              const statusClass = live ? `persona-${live.status}` : "persona-listening";
              const providerColor = PROVIDERS.find(p => p.key === persona.provider)?.color || "#888";
              return (
                <span
                  key={persona.id}
                  className={`audience-persona-chip ${statusClass}`}
                  title={`${persona.name}\n${persona.background}${persona.values ? `\nValues: ${Array.isArray(persona.values) ? persona.values.join(", ") : persona.values}` : ""}${persona.style ? `\nStyle: ${persona.style}` : ""}\nProvider: ${persona.provider} (${persona.model})`}
                >
                  <span className="persona-chip-provider" style={{ color: providerColor }}>
                    {persona.provider[0].toUpperCase()}
                  </span>
                  <span className="persona-chip-name">{persona.name}</span>
                  <span className="persona-chip-cost">{COST_LABELS[getCostTier(persona.provider)]}</span>
                </span>
              );
            })}
            {activePersonas.length === 0 && !loading && (
              <span className="audience-empty">No personas in pool</span>
            )}
            {loading && <span className="audience-loading">Loading...</span>}
          </div>
        </div>
      )}

      {/* Pool Editor Modal */}
      {editorOpen && editingPool && (
        <div className="audience-editor-overlay" onClick={() => setEditorOpen(false)}>
          <div className="audience-editor-modal" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Pool editor">
            <div className="audience-editor-header">
              <h3>Edit Pool: {editingPool.name}</h3>
              <button className="audience-editor-close" onClick={() => setEditorOpen(false)}>&times;</button>
            </div>

            <div className="audience-editor-body">
              {/* Left: persona list */}
              <div className="audience-editor-list">
                <div className="audience-editor-list-header">
                  <span>Personas ({editingPool.personas.length})</span>
                  <button className="audience-editor-add-btn" onClick={handleAddPersona}>+ Add</button>
                </div>
                {editingPool.personas.map((p, idx) => {
                  const providerColor = PROVIDERS.find(pr => pr.key === p.provider)?.color || "#888";
                  return (
                    <div
                      key={p.id}
                      className={`audience-editor-persona-item ${idx === editingPersonaIdx ? "selected" : ""}`}
                      onClick={() => setEditingPersonaIdx(idx)}
                    >
                      <span className="persona-item-provider" style={{ color: providerColor }}>
                        {p.provider[0].toUpperCase()}
                      </span>
                      <span className="persona-item-name">{p.name}</span>
                      <button
                        className="persona-item-remove"
                        onClick={(e) => { e.stopPropagation(); handleRemovePersona(idx); }}
                        title="Remove persona"
                      >
                        &times;
                      </button>
                    </div>
                  );
                })}
              </div>

              {/* Right: selected persona editor */}
              {editingPool.personas[editingPersonaIdx] && (
                <div className="audience-editor-detail">
                  <label className="audience-editor-field">
                    Name
                    <input
                      type="text"
                      value={editingPool.personas[editingPersonaIdx].name}
                      onChange={(e) => handleUpdatePersona(editingPersonaIdx, "name", e.target.value)}
                    />
                  </label>
                  <label className="audience-editor-field">
                    Background
                    <textarea
                      rows={3}
                      value={editingPool.personas[editingPersonaIdx].background}
                      onChange={(e) => handleUpdatePersona(editingPersonaIdx, "background", e.target.value)}
                      placeholder="1-2 sentences describing this persona's perspective..."
                    />
                  </label>
                  <label className="audience-editor-field">
                    Provider
                    <select
                      value={editingPool.personas[editingPersonaIdx].provider}
                      onChange={(e) => handleUpdatePersona(editingPersonaIdx, "provider", e.target.value)}
                    >
                      {PROVIDERS.map(p => (
                        <option key={p.key} value={p.key}>{p.label}</option>
                      ))}
                    </select>
                  </label>
                  <label className="audience-editor-field">
                    Model
                    <input
                      type="text"
                      value={editingPool.personas[editingPersonaIdx].model}
                      onChange={(e) => handleUpdatePersona(editingPersonaIdx, "model", e.target.value)}
                      placeholder="e.g. claude-haiku-4-5-20251001"
                    />
                  </label>
                  <label className="audience-editor-field">
                    Style
                    <input
                      type="text"
                      value={editingPool.personas[editingPersonaIdx].style || ""}
                      onChange={(e) => handleUpdatePersona(editingPersonaIdx, "style", e.target.value)}
                      placeholder="e.g. direct and technical"
                    />
                  </label>
                  <label className="audience-editor-field">
                    Values (comma-separated)
                    <input
                      type="text"
                      value={(() => {
                        const v = editingPool.personas[editingPersonaIdx].values;
                        if (!v) return "";
                        return Array.isArray(v) ? v.join(", ") : v;
                      })()}
                      onChange={(e) => {
                        if (!editingPool) return;
                        const updated = [...editingPool.personas];
                        updated[editingPersonaIdx] = {
                          ...updated[editingPersonaIdx],
                          values: e.target.value,
                        };
                        setEditingPool({ ...editingPool, personas: updated });
                      }}
                      placeholder="e.g. reliability, simplicity, performance"
                    />
                  </label>
                </div>
              )}
            </div>

            <div className="audience-editor-footer">
              {editingPool && !editingPool.builtin && (
                <button
                  className="audience-editor-delete"
                  onClick={() => { handleDeletePool(editingPool.id); setEditorOpen(false); }}
                >
                  Delete Pool
                </button>
              )}
              <span style={{ flex: 1 }} />
              <button className="audience-editor-cancel" onClick={() => setEditorOpen(false)}>Cancel</button>
              <button className="audience-editor-save" onClick={handleSavePool}>Save Pool</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
