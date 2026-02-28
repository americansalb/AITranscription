/**
 * FileClaimsPanel — Shows which agents are working on which files.
 * Prevents conflicts and provides visibility into team activity.
 */

import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/api";
import type { FileClaim } from "../lib/api";

interface FileClaimsPanelProps {
  projectId: string;
}

export function FileClaimsPanel({ projectId }: FileClaimsPanelProps) {
  const [claims, setClaims] = useState<FileClaim[]>([]);
  const [loading, setLoading] = useState(false);
  const [expanded, setExpanded] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const data = await api.getFileClaims(projectId);
      setClaims(data);
    } catch {
      // Claims API may not exist yet — silently ignore
      setClaims([]);
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => {
    refresh();
    // Refresh every 30 seconds
    const interval = setInterval(refresh, 30000);
    return () => clearInterval(interval);
  }, [refresh]);

  if (claims.length === 0 && !loading) return null;

  return (
    <div style={{ marginBottom: "var(--space-3)" }}>
      <button
        className="btn btn-ghost"
        style={{
          width: "100%",
          justifyContent: "space-between",
          fontSize: "var(--text-xs)",
          padding: "var(--space-1) 0",
          color: "var(--text-muted)",
        }}
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        aria-label={`File claims: ${claims.length} active`}
      >
        <span>File Claims ({claims.length})</span>
        <span>{expanded ? "\u25B2" : "\u25BC"}</span>
      </button>

      {expanded && (
        <div style={{
          padding: "var(--space-2)",
          background: "var(--bg-tertiary)",
          borderRadius: "var(--radius-sm)",
          fontSize: "var(--text-xs)",
        }}>
          {loading ? (
            <div style={{ color: "var(--text-muted)", textAlign: "center" }}>Loading...</div>
          ) : (
            claims.map((claim, i) => (
              <div
                key={`${claim.role}-${claim.instance}-${i}`}
                style={{
                  padding: "var(--space-1) 0",
                  borderBottom: i < claims.length - 1 ? "1px solid var(--border)" : undefined,
                }}
              >
                <div style={{ fontWeight: "var(--weight-medium)", color: "var(--text-secondary)" }}>
                  {claim.role}:{claim.instance}
                  <span style={{ color: "var(--text-muted)", marginLeft: "var(--space-1)" }}>
                    {claim.description}
                  </span>
                </div>
                <div style={{ color: "var(--text-muted)", marginTop: "2px" }}>
                  {claim.files.map((f) => (
                    <span key={f} style={{
                      display: "inline-block",
                      padding: "0 var(--space-1)",
                      marginRight: "var(--space-1)",
                      background: "var(--bg-primary)",
                      borderRadius: "var(--radius-sm)",
                      fontFamily: "monospace",
                    }}>
                      {f}
                    </span>
                  ))}
                </div>
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}
