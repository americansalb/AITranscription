/**
 * Vaaklite discussion-mode document workspace.
 *
 * Renders the document-drafting surface for a discussion-mode project:
 * a document list, a create-document form, and the section-rotation view
 * where the AI team drafts the document one section at a time.
 */

import { useEffect, useState } from "react";
import { useDocumentStore, useUIStore } from "../lib/stores";
import type {
  DocumentResponse,
  DocumentSectionInfo,
  ProjectResponse,
  SectionStatus,
} from "../lib/api";
import { getDocumentMarkdown } from "../lib/api";

const STATUS_LABEL: Record<SectionStatus, string> = {
  pending: "Pending",
  drafting: "Drafting",
  review_pending: "In review",
  accepted: "Accepted",
};

const STATUS_BADGE: Record<SectionStatus, string> = {
  pending: "",
  drafting: "badge-accent",
  review_pending: "badge-warning",
  accepted: "badge-success",
};

const PHASE_LABEL: Record<string, string> = {
  drafting: "Drafting",
  review: "Review",
  revision: "Revision",
  final: "Final",
};

export function DocumentWorkspace({
  projectId,
  project,
}: {
  projectId: string;
  project: ProjectResponse;
}) {
  const documents = useDocumentStore((s) => s.documents);
  const activeDocument = useDocumentStore((s) => s.activeDocument);
  const loading = useDocumentStore((s) => s.loading);
  const busy = useDocumentStore((s) => s.busy);
  const error = useDocumentStore((s) => s.error);
  const loadDocuments = useDocumentStore((s) => s.loadDocuments);
  const selectDocument = useDocumentStore((s) => s.selectDocument);
  const createDocument = useDocumentStore((s) => s.createDocument);
  const draftCurrent = useDocumentStore((s) => s.draftCurrent);
  const acceptSection = useDocumentStore((s) => s.acceptSection);
  const finalize = useDocumentStore((s) => s.finalize);
  const clear = useDocumentStore((s) => s.clear);
  const addToast = useUIStore((s) => s.addToast);

  const [showCreate, setShowCreate] = useState(false);
  const [newTitle, setNewTitle] = useState("");
  const [newTopic, setNewTopic] = useState("");

  useEffect(() => {
    loadDocuments(projectId);
    return () => clear();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  // Surface store errors as toasts
  useEffect(() => {
    if (error) addToast(error, "error");
  }, [error, addToast]);

  const handleCreate = async () => {
    if (!newTitle.trim()) return;
    const doc = await createDocument(projectId, newTitle.trim(), newTopic.trim());
    if (doc) {
      addToast(`Document "${doc.title}" created`, "success");
      setShowCreate(false);
      setNewTitle("");
      setNewTopic("");
    }
  };

  const handleDownload = async () => {
    if (!activeDocument) return;
    try {
      const md = await getDocumentMarkdown(projectId, activeDocument.id);
      const blob = new Blob([md.markdown], { type: "text/markdown;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${slugify(md.title)}.md`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch {
      addToast("Failed to download document", "error");
    }
  };

  return (
    <>
      <div className="page-header">
        <div>
          <h1 style={{ fontSize: "var(--text-xl)", fontWeight: "var(--weight-bold)" }}>
            {project.name}
          </h1>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: "var(--space-2)",
              marginTop: "var(--space-1)",
              fontSize: "var(--text-xs)",
              color: "var(--text-muted)",
            }}
          >
            <span className="badge badge-accent">Discussion</span>
            {project.template && <span>{project.template}</span>}
            <span>
              {"·"} {documents.length} document{documents.length === 1 ? "" : "s"}
            </span>
          </div>
        </div>
        <button className="btn btn-primary" onClick={() => setShowCreate(true)}>
          + New Document
        </button>
      </div>

      <div className="page-body" style={{ display: "flex", gap: "var(--space-4)", height: "100%" }}>
        {/* Left: document list */}
        <div style={{ width: 240, flexShrink: 0, overflowY: "auto" }}>
          <h2
            style={{
              fontSize: "var(--text-md)",
              fontWeight: "var(--weight-semibold)",
              marginBottom: "var(--space-3)",
            }}
          >
            Documents
          </h2>
          {loading && documents.length === 0 ? (
            <div className="loading-overlay" role="status" style={{ position: "static" }}>
              <div className="spinner" />
            </div>
          ) : documents.length === 0 ? (
            <div className="empty-state" style={{ padding: "var(--space-6) var(--space-3)" }}>
              <div className="empty-state-title" style={{ fontSize: "var(--text-sm)" }}>
                No documents yet
              </div>
              <div className="empty-state-desc" style={{ fontSize: "var(--text-xs)" }}>
                Create a document to start a drafting session.
              </div>
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
              {documents.map((doc) => (
                <button
                  key={doc.id}
                  className="card card-hover"
                  onClick={() => selectDocument(projectId, doc.id)}
                  style={{
                    cursor: "pointer",
                    textAlign: "left",
                    padding: "var(--space-3)",
                    borderColor:
                      activeDocument?.id === doc.id ? "var(--accent)" : "var(--border)",
                    background:
                      activeDocument?.id === doc.id ? "var(--accent-muted)" : undefined,
                  }}
                  aria-pressed={activeDocument?.id === doc.id}
                  aria-label={`Open document ${doc.title}`}
                >
                  <div
                    style={{
                      fontSize: "var(--text-sm)",
                      fontWeight: "var(--weight-medium)",
                      marginBottom: "var(--space-1)",
                    }}
                  >
                    {doc.title}
                  </div>
                  <span className={`badge ${doc.phase === "final" ? "badge-success" : "badge-accent"}`}>
                    {PHASE_LABEL[doc.phase] ?? doc.phase}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Right: active document */}
        <div style={{ flex: 1, minWidth: 0, overflowY: "auto" }}>
          {!activeDocument ? (
            <div className="empty-state" style={{ padding: "var(--space-8) var(--space-4)" }}>
              <div className="empty-state-icon">{"📄"}</div>
              <div className="empty-state-title">No document selected</div>
              <div className="empty-state-desc">
                Pick a document on the left, or create a new one to start a drafting
                session with your AI team.
              </div>
            </div>
          ) : (
            <ActiveDocumentView
              document={activeDocument}
              busy={busy}
              onDraft={() => draftCurrent(projectId)}
              onAccept={(idx) => acceptSection(projectId, idx)}
              onFinalize={() => finalize(projectId)}
              onDownload={handleDownload}
            />
          )}
        </div>
      </div>

      {/* Create document modal */}
      {showCreate && (
        <div
          className="modal-backdrop"
          onClick={(e) => {
            if (e.target === e.currentTarget) setShowCreate(false);
          }}
          role="dialog"
          aria-modal="true"
          aria-label="Create new document"
        >
          <div className="modal">
            <div className="modal-header">
              <h2 className="modal-title">New Document</h2>
              <button
                className="btn btn-ghost"
                onClick={() => setShowCreate(false)}
                aria-label="Close"
              >
                {"✕"}
              </button>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
              <div className="field">
                <label className="field-label" htmlFor="doc-title">
                  Title
                </label>
                <input
                  id="doc-title"
                  className="input"
                  type="text"
                  value={newTitle}
                  onChange={(e) => setNewTitle(e.target.value)}
                  placeholder="Product Vision Brief"
                  autoFocus
                  maxLength={200}
                />
              </div>
              <div className="field">
                <label className="field-label" htmlFor="doc-topic">
                  Topic / brief (optional)
                </label>
                <textarea
                  id="doc-topic"
                  className="input"
                  value={newTopic}
                  onChange={(e) => setNewTopic(e.target.value)}
                  placeholder="What should this document cover? The team uses this to frame each section."
                  style={{ minHeight: 80, resize: "vertical" }}
                  maxLength={5000}
                />
              </div>
              <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>
                Sections are auto-derived from the project's roles — one per team member.
              </div>
              <button
                className="btn btn-primary"
                onClick={handleCreate}
                disabled={!newTitle.trim() || busy}
                style={{ width: "100%", padding: "var(--space-3)" }}
              >
                {busy ? (
                  <>
                    <div className="spinner" style={{ width: 16, height: 16 }} /> Creating...
                  </>
                ) : (
                  "Create Document"
                )}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}

function ActiveDocumentView({
  document: doc,
  busy,
  onDraft,
  onAccept,
  onFinalize,
  onDownload,
}: {
  document: DocumentResponse;
  busy: boolean;
  onDraft: () => void;
  onAccept: (sectionIdx: number) => void;
  onFinalize: () => void;
  onDownload: () => void;
}) {
  const isFinal = doc.phase === "final";
  const isReview = doc.phase === "review";
  const markdown = isFinal ? doc.final_markdown : doc.rendered_markdown;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
      {/* Document header */}
      <div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--space-2)",
            marginBottom: "var(--space-1)",
          }}
        >
          <h2 style={{ fontSize: "var(--text-lg)", fontWeight: "var(--weight-bold)" }}>
            {doc.title}
          </h2>
          <span className={`badge ${isFinal ? "badge-success" : "badge-accent"}`}>
            {PHASE_LABEL[doc.phase] ?? doc.phase}
          </span>
        </div>
        {doc.topic && (
          <div style={{ fontSize: "var(--text-sm)", color: "var(--text-muted)" }}>{doc.topic}</div>
        )}
      </div>

      {/* Section rotation */}
      <div>
        <h3
          style={{
            fontSize: "var(--text-sm)",
            fontWeight: "var(--weight-semibold)",
            color: "var(--text-secondary)",
            marginBottom: "var(--space-2)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          Sections
        </h3>
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
          {doc.sections.map((section) => (
            <SectionRow
              key={section.id}
              section={section}
              isCurrent={doc.current_section_idx === section.idx && doc.phase === "drafting"}
              busy={busy}
              onDraft={onDraft}
              onAccept={() => onAccept(section.idx)}
            />
          ))}
        </div>
      </div>

      {/* Finalize / download actions */}
      <div style={{ display: "flex", gap: "var(--space-2)", flexWrap: "wrap" }}>
        {!isFinal && (
          <button
            className="btn btn-primary"
            onClick={onFinalize}
            disabled={busy}
            title={
              isReview
                ? "Lock the document — all sections drafted"
                : "Finalize the document at its current state"
            }
          >
            Finalize Document
          </button>
        )}
        <button className="btn btn-ghost" onClick={onDownload} disabled={busy}>
          {"↓"} Download .md
        </button>
      </div>

      {/* Markdown preview */}
      <div>
        <h3
          style={{
            fontSize: "var(--text-sm)",
            fontWeight: "var(--weight-semibold)",
            color: "var(--text-secondary)",
            marginBottom: "var(--space-2)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          {isFinal ? "Final Document" : "Live Preview"}
        </h3>
        <pre
          style={{
            margin: 0,
            padding: "var(--space-4)",
            background: "var(--bg-secondary)",
            border: "1px solid var(--border)",
            borderRadius: "var(--radius-md)",
            fontSize: "var(--text-sm)",
            lineHeight: 1.6,
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            fontFamily: "var(--font-mono, monospace)",
            color: "var(--text-primary)",
          }}
          aria-label="Document markdown preview"
        >
          {markdown || "_(empty — no sections drafted yet)_"}
        </pre>
      </div>
    </div>
  );
}

function SectionRow({
  section,
  isCurrent,
  busy,
  onDraft,
  onAccept,
}: {
  section: DocumentSectionInfo;
  isCurrent: boolean;
  busy: boolean;
  onDraft: () => void;
  onAccept: () => void;
}) {
  return (
    <div
      className="card"
      style={{
        padding: "var(--space-3)",
        borderLeft: isCurrent ? "3px solid var(--accent)" : "3px solid transparent",
        background: isCurrent ? "var(--accent-muted)" : undefined,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--space-2)",
          flexWrap: "wrap",
        }}
      >
        <span style={{ fontSize: "var(--text-sm)", fontWeight: "var(--weight-medium)" }}>
          {section.idx + 1}. {section.title}
        </span>
        <span className={`badge ${STATUS_BADGE[section.status]}`}>
          {STATUS_LABEL[section.status]}
        </span>
        {section.assigned_role && (
          <span style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>
            {section.assigned_role}
          </span>
        )}
        {isCurrent && (
          <span
            style={{ fontSize: "var(--text-xs)", color: "var(--accent)" }}
            title="This section holds the mic"
          >
            {"🎤"} current
          </span>
        )}
        <div style={{ marginLeft: "auto", display: "flex", gap: "var(--space-1)" }}>
          {isCurrent && section.status === "drafting" && (
            <button
              className="btn btn-primary"
              style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)" }}
              onClick={onDraft}
              disabled={busy}
            >
              {busy ? (
                <div className="spinner" style={{ width: 12, height: 12 }} />
              ) : (
                "Generate draft"
              )}
            </button>
          )}
          {section.status === "review_pending" && (
            <button
              className="btn btn-ghost"
              style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)", color: "var(--success)" }}
              onClick={onAccept}
              disabled={busy}
            >
              {"✓"} Accept
            </button>
          )}
        </div>
      </div>
      {section.body && (
        <div
          style={{
            marginTop: "var(--space-2)",
            fontSize: "var(--text-sm)",
            color: "var(--text-secondary)",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
          }}
        >
          {section.body}
        </div>
      )}
    </div>
  );
}

function slugify(title: string): string {
  return (
    title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 60) || "document"
  );
}
