/**
 * AddRoleModal — Simplified role creation for the web client.
 * Desktop has a 7-step wizard; this provides a streamlined single-form experience.
 */

import { useCallback, useState } from "react";
import { useFocusTrap } from "../hooks/useFocusTrap";
import { useUIStore } from "../lib/stores";
import * as api from "../lib/api";

const ROLE_TEMPLATES = [
  { slug: "developer", title: "Developer", desc: "Implements features, fixes bugs, writes code", tags: ["code_write", "code_review"] },
  { slug: "architect", title: "Architect", desc: "Designs system architecture and technical strategy", tags: ["code_review", "assign_tasks"] },
  { slug: "tester", title: "Tester", desc: "Writes and runs tests, validates implementations", tags: ["code_write", "code_review"] },
  { slug: "researcher", title: "Researcher", desc: "Deep research, literature review, fact-checking", tags: ["web_search", "code_review"] },
  { slug: "security", title: "Security Auditor", desc: "Security reviews, vulnerability scanning", tags: ["code_review", "web_search"] },
  { slug: "writer", title: "Tech Writer", desc: "Documentation, guides, API references", tags: ["code_write", "code_review"] },
  { slug: "qa-lead", title: "QA Lead", desc: "Test strategy, coverage analysis, quality gates", tags: ["code_review", "assign_tasks"] },
  { slug: "devops", title: "DevOps", desc: "CI/CD, infrastructure, deployment pipelines", tags: ["code_write", "shell_exec"] },
];

interface AddRoleModalProps {
  projectId: string;
  existingSlugs: string[];
  onClose: () => void;
  onCreated: () => void;
}

export function AddRoleModal({ projectId, existingSlugs, onClose, onCreated }: AddRoleModalProps) {
  const addToast = useUIStore((s) => s.addToast);
  const closeHandler = useCallback(() => onClose(), [onClose]);
  const modalRef = useFocusTrap(true, closeHandler);

  const [selectedTemplate, setSelectedTemplate] = useState<string | null>(null);
  const [slug, setSlug] = useState("");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [maxInstances, setMaxInstances] = useState(1);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

  const handleTemplateSelect = (tmpl: typeof ROLE_TEMPLATES[0]) => {
    setSelectedTemplate(tmpl.slug);
    // Auto-fill from template if fields are empty
    if (!slug) setSlug(tmpl.slug);
    if (!title) setTitle(tmpl.title);
    if (!description) setDescription(tmpl.desc);
  };

  const handleCreate = async () => {
    setError("");
    const cleanSlug = slug.trim().toLowerCase().replace(/[^a-z0-9-]/g, "-");

    if (!cleanSlug) {
      setError("Slug is required");
      return;
    }
    if (existingSlugs.includes(cleanSlug)) {
      setError(`Role "${cleanSlug}" already exists`);
      return;
    }
    if (!title.trim()) {
      setError("Title is required");
      return;
    }

    setCreating(true);
    try {
      await api.createRole(projectId, cleanSlug, {
        title: title.trim(),
        description: description.trim(),
        maxInstances,
        tags: [],
        permissions: [],
        provider: { provider: "anthropic", model: "claude-sonnet-4-6" },
      });
      addToast(`Role "${title}" created`, "success");
      onCreated();
      onClose();
    } catch (e) {
      addToast(e instanceof api.ApiError ? e.userMessage : "Failed to create role", "error");
    } finally {
      setCreating(false);
    }
  };

  return (
    <div
      className="modal-backdrop"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
      role="dialog"
      aria-modal="true"
      aria-label="Add role"
    >
      <div className="modal" ref={modalRef} style={{ maxWidth: 520 }}>
        <div className="modal-header">
          <h2 className="modal-title">Add Role</h2>
          <button className="btn btn-ghost" onClick={onClose} aria-label="Close">
            {"\u2715"}
          </button>
        </div>

        {/* Template quick-select */}
        <div style={{ marginBottom: "var(--space-3)" }}>
          <div className="field-label" style={{ marginBottom: "var(--space-2)" }}>Quick Start from Template</div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "var(--space-1)" }}>
            {ROLE_TEMPLATES.map((tmpl) => (
              <button
                key={tmpl.slug}
                className={`card ${selectedTemplate === tmpl.slug ? "" : "card-hover"}`}
                onClick={() => handleTemplateSelect(tmpl)}
                style={{
                  cursor: "pointer",
                  textAlign: "left",
                  padding: "var(--space-2)",
                  borderColor: selectedTemplate === tmpl.slug ? "var(--accent)" : "var(--border)",
                  background: selectedTemplate === tmpl.slug ? "var(--accent-muted)" : undefined,
                  opacity: existingSlugs.includes(tmpl.slug) ? 0.5 : 1,
                }}
                disabled={existingSlugs.includes(tmpl.slug)}
                aria-pressed={selectedTemplate === tmpl.slug}
              >
                <div style={{ fontSize: "var(--text-sm)", fontWeight: "var(--weight-medium)" }}>{tmpl.title}</div>
                <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>{tmpl.desc}</div>
              </button>
            ))}
          </div>
        </div>

        {/* Custom fields */}
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
          <div style={{ display: "flex", gap: "var(--space-2)" }}>
            <div className="field" style={{ flex: 1 }}>
              <label className="field-label" htmlFor="role-slug">Slug</label>
              <input
                id="role-slug"
                className="input"
                value={slug}
                onChange={(e) => setSlug(e.target.value)}
                placeholder="developer"
                maxLength={50}
              />
            </div>
            <div className="field" style={{ flex: 1 }}>
              <label className="field-label" htmlFor="role-title">Title</label>
              <input
                id="role-title"
                className="input"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="Developer"
                maxLength={100}
              />
            </div>
          </div>

          <div className="field">
            <label className="field-label" htmlFor="role-desc">Description</label>
            <textarea
              id="role-desc"
              className="input"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="What does this role do?"
              style={{ minHeight: 60, resize: "vertical" }}
              maxLength={500}
            />
          </div>

          <div className="field" style={{ maxWidth: 120 }}>
            <label className="field-label" htmlFor="role-instances">Max Instances</label>
            <input
              id="role-instances"
              className="input"
              type="number"
              value={maxInstances}
              onChange={(e) => setMaxInstances(Math.max(1, Math.min(10, Number(e.target.value))))}
              min={1}
              max={10}
            />
          </div>

          {error && (
            <div role="alert" style={{ color: "var(--error)", fontSize: "var(--text-sm)" }}>
              {error}
            </div>
          )}

          <button
            className="btn btn-primary"
            onClick={handleCreate}
            disabled={creating || !slug.trim() || !title.trim()}
            style={{ width: "100%" }}
          >
            {creating ? "Creating..." : "Create Role"}
          </button>
        </div>
      </div>
    </div>
  );
}
