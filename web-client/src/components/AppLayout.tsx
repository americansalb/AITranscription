import { NavLink, useNavigate } from "react-router-dom";
import { useAuthStore, useProjectStore, useUIStore } from "../lib/stores";
import { useEffect, useState, useCallback } from "react";

export function AppLayout({ children }: { children: React.ReactNode }) {
  const user = useAuthStore((s) => s.user);
  const logout = useAuthStore((s) => s.logout);
  const projects = useProjectStore((s) => s.projects);
  const loadProjects = useProjectStore((s) => s.loadProjects);
  const sidebarOpen = useUIStore((s) => s.sidebarOpen);
  const toggleSidebar = useUIStore((s) => s.toggleSidebar);
  const navigate = useNavigate();

  const [theme, setTheme] = useState<"dark" | "light">(() => {
    return (localStorage.getItem("vaak_theme") as "dark" | "light") || "dark";
  });

  const toggleTheme = useCallback(() => {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    localStorage.setItem("vaak_theme", next);
    document.documentElement.classList.toggle("light", next === "light");
    document.documentElement.classList.toggle("dark", next === "dark");
  }, [theme]);

  // Apply theme on mount
  useEffect(() => {
    document.documentElement.classList.toggle("light", theme === "light");
    document.documentElement.classList.toggle("dark", theme === "dark");
  }, [theme]);

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  return (
    <div className="app-layout">
      <nav
        className={`sidebar ${sidebarOpen ? "" : "collapsed"}`}
        role="navigation"
        aria-label="Main navigation"
      >
        <div style={{
          padding: "var(--space-4)",
          borderBottom: "1px solid var(--border)",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}>
          {sidebarOpen && (
            <span style={{ fontWeight: "var(--weight-bold)", fontSize: "var(--text-md)" }}>
              Vaak
            </span>
          )}
          <button
            className="btn btn-ghost"
            onClick={toggleSidebar}
            aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
            title={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
          >
            {sidebarOpen ? "\u2190" : "\u2192"}
          </button>
        </div>

        {sidebarOpen && (
          <>
            <div style={{ padding: "var(--space-3) var(--space-4)", flex: 1, overflowY: "auto" }}>
              <div style={{ marginBottom: "var(--space-4)" }}>
                <div style={{
                  fontSize: "var(--text-xs)",
                  color: "var(--text-muted)",
                  textTransform: "uppercase",
                  letterSpacing: "0.05em",
                  marginBottom: "var(--space-2)",
                  fontWeight: "var(--weight-semibold)",
                }}>
                  Projects
                </div>
                <NavLink
                  to="/"
                  className={({ isActive }) =>
                    `btn btn-ghost ${isActive ? "active" : ""}`
                  }
                  style={{ width: "100%", justifyContent: "flex-start", marginBottom: "var(--space-1)" }}
                >
                  All Projects
                </NavLink>
                {projects.map((p) => (
                  <NavLink
                    key={p.id}
                    to={`/project/${p.id}`}
                    className={({ isActive }) =>
                      `btn btn-ghost ${isActive ? "active" : ""}`
                    }
                    style={{ width: "100%", justifyContent: "flex-start", marginBottom: "var(--space-1)" }}
                  >
                    {p.name}
                  </NavLink>
                ))}
              </div>

              <div style={{ marginTop: "var(--space-4)" }}>
                <div style={{
                  fontSize: "var(--text-xs)",
                  color: "var(--text-muted)",
                  textTransform: "uppercase",
                  letterSpacing: "0.05em",
                  marginBottom: "var(--space-2)",
                  fontWeight: "var(--weight-semibold)",
                }}>
                  Account
                </div>
                <NavLink
                  to="/billing"
                  className={({ isActive }) =>
                    `btn btn-ghost ${isActive ? "active" : ""}`
                  }
                  style={{ width: "100%", justifyContent: "flex-start", marginBottom: "var(--space-1)" }}
                >
                  Billing & Usage
                </NavLink>
                <NavLink
                  to="/settings"
                  className={({ isActive }) =>
                    `btn btn-ghost ${isActive ? "active" : ""}`
                  }
                  style={{ width: "100%", justifyContent: "flex-start" }}
                >
                  Settings
                </NavLink>
              </div>
            </div>

            <div style={{
              padding: "var(--space-3) var(--space-4)",
              borderTop: "1px solid var(--border)",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}>
              <span style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>
                {user?.email}
              </span>
              <button
                className="btn btn-ghost"
                onClick={toggleTheme}
                aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
                title={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
                style={{ fontSize: "var(--text-sm)", padding: "var(--space-1)" }}
              >
                {theme === "dark" ? "\u2600\uFE0F" : "\uD83C\uDF19"}
              </button>
              <button
                className="btn btn-ghost"
                onClick={() => { logout(); navigate("/login"); }}
                aria-label="Log out"
                title="Log out"
              >
                Logout
              </button>
            </div>
          </>
        )}
      </nav>

      <main className="main-content" role="main">
        {children}
      </main>
    </div>
  );
}
