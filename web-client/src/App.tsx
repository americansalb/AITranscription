import { useEffect } from "react";
import { Routes, Route, Navigate, useLocation } from "react-router-dom";
import { useAuthStore } from "./lib/stores";
import { LoginPage } from "./pages/LoginPage";
import { DashboardPage } from "./pages/DashboardPage";
import { ProjectPage } from "./pages/ProjectPage";
import { BillingPage } from "./pages/BillingPage";
import { SettingsPage } from "./pages/SettingsPage";
import { AppLayout } from "./components/AppLayout";
import { ToastContainer } from "./components/Toast";
import { ErrorBoundary } from "./components/ErrorBoundary";

/** Move focus to main content on route changes for screen reader users (fixes C3) */
function FocusOnNavigate() {
  const location = useLocation();
  useEffect(() => {
    // Small delay to let the new route render before focusing
    const timer = setTimeout(() => {
      const main = document.querySelector<HTMLElement>("[role='main']");
      if (main) {
        main.setAttribute("tabindex", "-1");
        main.focus({ preventScroll: true });
      }
    }, 50);
    return () => clearTimeout(timer);
  }, [location.pathname]);
  return null;
}

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const user = useAuthStore((s) => s.user);
  const loading = useAuthStore((s) => s.loading);

  if (loading) {
    return (
      <div className="loading-overlay" role="status" aria-label="Loading">
        <div className="spinner" />
        <span>Loading...</span>
      </div>
    );
  }

  if (!user) return <Navigate to="/login" replace />;
  return <>{children}</>;
}

export function App() {
  const loadUser = useAuthStore((s) => s.loadUser);

  useEffect(() => {
    loadUser();
  }, [loadUser]);

  return (
    <ErrorBoundary>
      <FocusOnNavigate />
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/signup" element={<LoginPage signup />} />
        <Route
          path="/*"
          element={
            <ProtectedRoute>
              <AppLayout>
                <ErrorBoundary>
                  <Routes>
                    <Route path="/" element={<DashboardPage />} />
                    <Route path="/project/:projectId" element={<ProjectPage />} />
                    <Route path="/billing" element={<BillingPage />} />
                    <Route path="/settings" element={<SettingsPage />} />
                  </Routes>
                </ErrorBoundary>
              </AppLayout>
            </ProtectedRoute>
          }
        />
      </Routes>
      <ToastContainer />
    </ErrorBoundary>
  );
}
