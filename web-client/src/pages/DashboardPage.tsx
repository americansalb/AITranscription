import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useProjectStore, useUIStore } from "../lib/stores";

const PROJECT_TEMPLATES = [
  { id: "software-dev", name: "Software Development", desc: "Manager, Architect, Developer, Tester — code review pipeline", icon: "\uD83D\uDCBB" },
  { id: "research", name: "Research & Analysis", desc: "Researcher, Analyst, Reviewer — literature review and synthesis", icon: "\uD83D\uDD2C" },
  { id: "content", name: "Content Creation", desc: "Author, Editor, QA — content production pipeline", icon: "\u270D\uFE0F" },
  { id: "debate", name: "Structured Debate", desc: "Moderator, Debaters, Audience — Oxford/Delphi discussions", icon: "\uD83C\uDFDB\uFE0F" },
];

export function DashboardPage() {
  const projects = useProjectStore((s) => s.projects);
  const loading = useProjectStore((s) => s.loading);
  const createProject = useProjectStore((s) => s.createProject);
  const addToast = useUIStore((s) => s.addToast);
  const navigate = useNavigate();

  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState("");
  const [selectedTemplate, setSelectedTemplate] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const handleCreate = async () => {
    if (!newName.trim()) return;
    setCreating(true);
    try {
      await createProject(newName.trim(), selectedTemplate || undefined);
      addToast(`Project "${newName}" created`, "success");
      setShowCreate(false);
      setNewName("");
      setSelectedTemplate(null);
    } catch {
      addToast("Failed to create project", "error");
    } finally {
      setCreating(false);
    }
  };

  return (
    <>
      <div className="page-header">
        <h1 style={{ fontSize: "var(--text-xl)", fontWeight: "var(--weight-bold)" }}>Projects</h1>
        <button className="btn btn-primary" onClick={() => setShowCreate(true)}>
          + New Project
        </button>
      </div>

      <div className="page-body">
        {loading && projects.length === 0 ? (
          <div className="loading-overlay" role="status">
            <div className="spinner" />
            <span>Loading projects...</span>
          </div>
        ) : projects.length === 0 ? (
          <div className="empty-state" role="status">
            <div className="empty-state-icon">{"\uD83D\uDE80"}</div>
            <div className="empty-state-title">No projects yet</div>
            <div className="empty-state-desc">
              Create your first project to start collaborating with AI teams.
              Pick a template or start from scratch.
            </div>
            <button className="btn btn-primary" onClick={() => setShowCreate(true)}>
              Create Your First Project
            </button>
          </div>
        ) : (
          <div style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))",
            gap: "var(--space-4)",
          }}>
            {projects.map((p) => (
              <button
                key={p.id}
                className="card card-hover"
                onClick={() => navigate(`/project/${p.id}`)}
                style={{
                  cursor: "pointer",
                  textAlign: "left",
                  border: "1px solid var(--border)",
                }}
                aria-label={`Open project ${p.name}`}
              >
                <div style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)", marginBottom: "var(--space-2)" }}>
                  {p.name}
                </div>
                <div style={{ fontSize: "var(--text-sm)", color: "var(--text-muted)" }}>
                  {Object.keys(p.roles).length} roles
                </div>
                <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)", marginTop: "var(--space-2)" }}>
                  Created {new Date(p.created_at).toLocaleDateString()}
                </div>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Create project modal */}
      {showCreate && (
        <div
          className="modal-backdrop"
          onClick={(e) => { if (e.target === e.currentTarget) setShowCreate(false); }}
          role="dialog"
          aria-modal="true"
          aria-label="Create new project"
        >
          <div className="modal">
            <div className="modal-header">
              <h2 className="modal-title">New Project</h2>
              <button
                className="btn btn-ghost"
                onClick={() => setShowCreate(false)}
                aria-label="Close"
              >
                {"\u2715"}
              </button>
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
              <div className="field">
                <label className="field-label" htmlFor="project-name">Project Name</label>
                <input
                  id="project-name"
                  className="input"
                  type="text"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  placeholder="My AI Team Project"
                  autoFocus
                  maxLength={100}
                />
              </div>

              <div>
                <div className="field-label" style={{ marginBottom: "var(--space-2)" }}>
                  Template (optional)
                </div>
                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "var(--space-2)" }}>
                  {PROJECT_TEMPLATES.map((t) => (
                    <button
                      key={t.id}
                      className={`card ${selectedTemplate === t.id ? "" : "card-hover"}`}
                      onClick={() => setSelectedTemplate(selectedTemplate === t.id ? null : t.id)}
                      style={{
                        cursor: "pointer",
                        textAlign: "left",
                        borderColor: selectedTemplate === t.id ? "var(--accent)" : "var(--border)",
                        background: selectedTemplate === t.id ? "var(--accent-muted)" : undefined,
                        padding: "var(--space-3)",
                      }}
                      aria-pressed={selectedTemplate === t.id}
                    >
                      <div style={{ fontSize: "var(--text-md)", marginBottom: "var(--space-1)" }}>{t.icon}</div>
                      <div style={{ fontSize: "var(--text-sm)", fontWeight: "var(--weight-medium)" }}>{t.name}</div>
                      <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)", marginTop: "var(--space-1)" }}>
                        {t.desc}
                      </div>
                    </button>
                  ))}
                </div>
              </div>

              <button
                className="btn btn-primary"
                onClick={handleCreate}
                disabled={!newName.trim() || creating}
                style={{ width: "100%", padding: "var(--space-3)" }}
              >
                {creating ? (
                  <><div className="spinner" style={{ width: 16, height: 16 }} /> Creating...</>
                ) : (
                  "Create Project"
                )}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
