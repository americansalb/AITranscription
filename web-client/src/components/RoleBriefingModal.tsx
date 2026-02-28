/**
 * RoleBriefingModal — View and edit role briefings.
 * Briefings are the job descriptions that agents read when they join.
 */

import { useCallback, useEffect, useState } from "react";
import { useUIStore } from "../lib/stores";
import { useFocusTrap } from "../hooks/useFocusTrap";
import * as api from "../lib/api";

interface RoleBriefingModalProps {
  projectId: string;
  roleSlug: string;
  roleTitle: string;
  onClose: () => void;
}

export function RoleBriefingModal({ projectId, roleSlug, roleTitle, onClose }: RoleBriefingModalProps) {
  const addToast = useUIStore((s) => s.addToast);
  const [briefing, setBriefing] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [editing, setEditing] = useState(false);

  const closeHandler = useCallback(() => onClose(), [onClose]);
  const modalRef = useFocusTrap(true, closeHandler);

  useEffect(() => {
    api.getRoleBriefing(projectId, roleSlug)
      .then((res) => {
        setBriefing(res.briefing);
        setLoading(false);
      })
      .catch(() => {
        setBriefing("(No briefing found)");
        setLoading(false);
      });
  }, [projectId, roleSlug]);

  const handleSave = async () => {
    setSaving(true);
    try {
      await api.updateRoleBriefing(projectId, roleSlug, briefing);
      addToast("Briefing saved", "success");
      setEditing(false);
    } catch (e) {
      addToast(e instanceof api.ApiError ? e.userMessage : "Failed to save briefing", "error");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="modal-backdrop"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
      role="dialog"
      aria-modal="true"
      aria-label={`Briefing for ${roleTitle}`}
    >
      <div className="modal" ref={modalRef} style={{ maxWidth: 640 }}>
        <div className="modal-header">
          <h2 className="modal-title">{roleTitle} Briefing</h2>
          <button
            className="btn btn-ghost"
            onClick={onClose}
            aria-label="Close"
          >
            {"\u2715"}
          </button>
        </div>

        {loading ? (
          <div style={{ padding: "var(--space-4)", textAlign: "center" }}>
            <div className="spinner" />
            <div style={{ marginTop: "var(--space-2)", color: "var(--text-muted)" }}>Loading briefing...</div>
          </div>
        ) : editing ? (
          <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
            <textarea
              className="input"
              value={briefing}
              onChange={(e) => setBriefing(e.target.value)}
              style={{
                minHeight: 300,
                fontFamily: "monospace",
                fontSize: "var(--text-sm)",
                resize: "vertical",
              }}
              aria-label="Briefing content"
            />
            <div style={{ display: "flex", gap: "var(--space-2)" }}>
              <button className="btn btn-primary" onClick={handleSave} disabled={saving}>
                {saving ? "Saving..." : "Save"}
              </button>
              <button className="btn btn-ghost" onClick={() => setEditing(false)}>Cancel</button>
            </div>
          </div>
        ) : (
          <div>
            <div
              style={{
                padding: "var(--space-3)",
                background: "var(--bg-tertiary)",
                borderRadius: "var(--radius-sm)",
                fontSize: "var(--text-sm)",
                color: "var(--text-secondary)",
                whiteSpace: "pre-wrap",
                maxHeight: 400,
                overflowY: "auto",
                lineHeight: 1.6,
              }}
            >
              {briefing}
            </div>
            <div style={{ marginTop: "var(--space-3)" }}>
              <button className="btn btn-secondary" onClick={() => setEditing(true)}>
                Edit Briefing
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
