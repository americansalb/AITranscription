import { useState, FormEvent } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useAuthStore } from "../lib/stores";

export function LoginPage({ signup = false }: { signup?: boolean }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [fullName, setFullName] = useState("");
  const authLogin = useAuthStore((s) => s.login);
  const authSignup = useAuthStore((s) => s.signup);
  const loading = useAuthStore((s) => s.loading);
  const error = useAuthStore((s) => s.error);
  const user = useAuthStore((s) => s.user);
  const navigate = useNavigate();

  // Redirect if already logged in
  if (user) {
    navigate("/", { replace: true });
    return null;
  }

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (signup) {
      await authSignup(email, password, fullName || undefined);
    } else {
      await authLogin(email, password);
    }
    // Store handles navigation via user state change
  };

  const isValid = email.includes("@") && password.length >= 8;

  return (
    <div style={{
      minHeight: "100vh",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      padding: "var(--space-4)",
      background: "var(--bg-primary)",
    }}>
      <div style={{ width: "100%", maxWidth: 400 }}>
        <div style={{ textAlign: "center", marginBottom: "var(--space-8)" }}>
          <h1 style={{ fontSize: "var(--text-2xl)", fontWeight: "var(--weight-bold)", marginBottom: "var(--space-2)" }}>
            {signup ? "Create Account" : "Welcome Back"}
          </h1>
          <p style={{ color: "var(--text-muted)", fontSize: "var(--text-sm)" }}>
            {signup
              ? "Sign up to start collaborating with AI teams"
              : "Log in to your Vaak account"}
          </p>
        </div>

        <form onSubmit={handleSubmit} className="card" style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
          {error && (
            <div role="alert" style={{
              padding: "var(--space-3)",
              background: "var(--error-muted)",
              color: "var(--error)",
              borderRadius: "var(--radius-sm)",
              fontSize: "var(--text-sm)",
            }}>
              {error}
            </div>
          )}

          {signup && (
            <div className="field">
              <label className="field-label" htmlFor="fullName">Full Name</label>
              <input
                id="fullName"
                className="input"
                type="text"
                value={fullName}
                onChange={(e) => setFullName(e.target.value)}
                placeholder="Your name (optional)"
                autoComplete="name"
              />
            </div>
          )}

          <div className="field">
            <label className="field-label" htmlFor="email">Email</label>
            <input
              id="email"
              className={`input ${error ? "input-error" : ""}`}
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="you@example.com"
              required
              autoComplete="email"
              aria-describedby={error ? "login-error" : undefined}
            />
          </div>

          <div className="field">
            <label className="field-label" htmlFor="password">Password</label>
            <input
              id="password"
              className={`input ${error ? "input-error" : ""}`}
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="At least 8 characters"
              required
              minLength={8}
              autoComplete={signup ? "new-password" : "current-password"}
            />
            {signup && (
              <span className="field-hint">
                Must be 8+ characters with uppercase, lowercase, and a number
              </span>
            )}
          </div>

          <button
            type="submit"
            className="btn btn-primary"
            disabled={loading || !isValid}
            style={{ width: "100%", padding: "var(--space-3)" }}
          >
            {loading ? (
              <><div className="spinner" style={{ width: 16, height: 16 }} /> Loading...</>
            ) : (
              signup ? "Create Account" : "Log In"
            )}
          </button>

          <p style={{ textAlign: "center", fontSize: "var(--text-sm)", color: "var(--text-muted)" }}>
            {signup ? (
              <>Already have an account? <Link to="/login">Log in</Link></>
            ) : (
              <>Don&apos;t have an account? <Link to="/signup">Sign up</Link></>
            )}
          </p>
        </form>
      </div>
    </div>
  );
}
