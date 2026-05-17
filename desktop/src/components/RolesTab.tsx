// Roles tab R1 (Roster view skeleton) per .vaak/design-notes/roles-tab-spec-2026-05-17.md
// v2 §9 R1 + human msg 4499 ("where is the role manager and the place to upload avatars?").
//
// R1 scope: read-only grid of role cards consuming current project.json data via the
// same watch_project_dir IPC + localStorage projectDir pattern CollabTab uses. Click
// stub for now — full editor modal is R2; team view is R3; history is R4; stats radar
// is R5; rotation-strip tooltip is R6.
import { useEffect, useState } from "react";
import type { ParsedProject, RoleConfig } from "../lib/collabTypes";
import { Avatar } from "./Avatar";

const PROJECT_DIR_STORAGE_KEY = "vaak_collab_project_dir";

function loadPersistedDir(): string {
  try { return localStorage.getItem(PROJECT_DIR_STORAGE_KEY) || ""; } catch { return ""; }
}

interface RoleCardData {
  slug: string;
  role: RoleConfig;
  avatarUrl: string | null;
}

export function RolesTab() {
  const [project, setProject] = useState<ParsedProject | null>(null);
  const [projectDir] = useState(loadPersistedDir);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Load project.json roles from the same dir CollabTab watches.
  useEffect(() => {
    if (!projectDir || !window.__TAURI__) return;
    let cancelled = false;
    setLoading(true);
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
        if (!cancelled && result) setProject(result);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [projectDir]);

  if (!projectDir) {
    return (
      <div className="roles-tab-empty">
        <p>No project loaded. Open a project in the Collab tab first.</p>
      </div>
    );
  }
  if (loading && !project) return <div className="roles-tab-empty">Loading roles…</div>;
  if (error) return <div className="roles-tab-empty" role="alert">Error loading roles: {error}</div>;
  if (!project) return <div className="roles-tab-empty">No project data yet.</div>;

  const roles: RoleCardData[] = Object.entries(project.config.roles || {}).map(([slug, role]) => ({
    slug,
    role,
    avatarUrl: ((role as unknown as { avatar_url?: string }).avatar_url) || null,
  }));

  if (roles.length === 0) {
    return (
      <div className="roles-tab-empty">
        <p>No roles defined for this project yet.</p>
        <p>Use the Collab tab's "Add Role" wizard to create roles.</p>
      </div>
    );
  }

  return (
    <div className="roles-tab">
      <div className="roles-tab-header">
        <h2 className="roles-tab-title">Roles</h2>
        <p className="roles-tab-subtitle">
          {roles.length} role{roles.length === 1 ? "" : "s"} — click to edit in Collab tab wizard.
          Dedicated editor modal coming in R2.
        </p>
      </div>
      <div className="roles-grid">
        {roles.map(({ slug, role, avatarUrl }) => (
          // Phase 2.B Part 2 per ui-architect:1 msg 4656: shared <Avatar>.
          // Role-definition surface — no instance prop, alt text omits :${instance}.
          <article key={slug} className="role-card" tabIndex={0}>
            <div className="role-card-avatar">
              <Avatar
                slug={slug}
                title={role.title}
                avatarUrl={avatarUrl}
                sizePx={48}
                className="role-card-avatar-img"
              />
            </div>
            <div className="role-card-body">
              <h3 className="role-card-title">{role.title || slug}</h3>
              <p className="role-card-slug">{slug}</p>
              <p className="role-card-description">{role.description || "No description."}</p>
              <div className="role-card-meta">
                <span className="role-card-instances">{role.max_instances}× instance{role.max_instances === 1 ? "" : "s"}</span>
                {role.custom && <span className="role-card-custom-badge">custom</span>}
                {avatarUrl && <span className="role-card-avatar-badge">avatar</span>}
              </div>
            </div>
          </article>
        ))}
      </div>
    </div>
  );
}
