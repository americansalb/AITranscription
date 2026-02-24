/**
 * SettingsPage — Account settings: profile, password, BYOK API keys.
 */

import { useState } from "react";
import { useAuthStore, useUIStore } from "../lib/stores";
import * as api from "../lib/api";

export function SettingsPage() {
  const user = useAuthStore((s) => s.user);
  const addToast = useUIStore((s) => s.addToast);

  const [fullName, setFullName] = useState(user?.full_name || "");
  const [savingName, setSavingName] = useState(false);

  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [savingPassword, setSavingPassword] = useState(false);
  const [passwordError, setPasswordError] = useState("");

  // BYOK keys (only visible for BYOK tier)
  const [anthropicKey, setAnthropicKey] = useState("");
  const [openaiKey, setOpenaiKey] = useState("");
  const [googleKey, setGoogleKey] = useState("");
  const [savingKeys, setSavingKeys] = useState(false);

  const handleSaveName = async () => {
    setSavingName(true);
    try {
      await api.updateProfile({ full_name: fullName.trim() || null });
      addToast("Profile updated", "success");
    } catch (e) {
      addToast(e instanceof api.ApiError ? e.userMessage : "Failed to update profile", "error");
    } finally {
      setSavingName(false);
    }
  };

  const handleChangePassword = async () => {
    setPasswordError("");
    if (newPassword.length < 8) {
      setPasswordError("Password must be at least 8 characters");
      return;
    }
    if (newPassword !== confirmPassword) {
      setPasswordError("Passwords do not match");
      return;
    }
    setSavingPassword(true);
    try {
      await api.changePassword(currentPassword, newPassword);
      addToast("Password changed", "success");
      setCurrentPassword("");
      setNewPassword("");
      setConfirmPassword("");
    } catch (e) {
      addToast(e instanceof api.ApiError ? e.userMessage : "Failed to change password", "error");
    } finally {
      setSavingPassword(false);
    }
  };

  const handleSaveKeys = async () => {
    setSavingKeys(true);
    try {
      await api.updateApiKeys({
        anthropic: anthropicKey.trim() || undefined,
        openai: openaiKey.trim() || undefined,
        google: googleKey.trim() || undefined,
      });
      addToast("API keys saved", "success");
      // Clear the fields after save (keys are write-only for security)
      setAnthropicKey("");
      setOpenaiKey("");
      setGoogleKey("");
    } catch (e) {
      addToast(e instanceof api.ApiError ? e.userMessage : "Failed to save API keys", "error");
    } finally {
      setSavingKeys(false);
    }
  };

  return (
    <>
      <div className="page-header">
        <h1 style={{ fontSize: "var(--text-xl)", fontWeight: "var(--weight-bold)" }}>Settings</h1>
      </div>

      <div className="page-body" style={{ maxWidth: 600 }}>
        {/* Profile section */}
        <section style={{ marginBottom: "var(--space-6)" }}>
          <h2 style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)", marginBottom: "var(--space-3)" }}>
            Profile
          </h2>
          <div className="card" style={{ padding: "var(--space-4)" }}>
            <div className="field" style={{ marginBottom: "var(--space-3)" }}>
              <label className="field-label" htmlFor="settings-email">Email</label>
              <input
                id="settings-email"
                className="input"
                value={user?.email || ""}
                disabled
                style={{ opacity: 0.6 }}
                aria-describedby="email-note"
              />
              <div id="email-note" style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)", marginTop: "var(--space-1)" }}>
                Email cannot be changed
              </div>
            </div>

            <div className="field" style={{ marginBottom: "var(--space-3)" }}>
              <label className="field-label" htmlFor="settings-name">Display Name</label>
              <input
                id="settings-name"
                className="input"
                value={fullName}
                onChange={(e) => setFullName(e.target.value)}
                placeholder="Your name"
                maxLength={100}
              />
            </div>

            <div style={{ display: "flex", alignItems: "center", gap: "var(--space-2)" }}>
              <button
                className="btn btn-primary"
                onClick={handleSaveName}
                disabled={savingName}
              >
                {savingName ? "Saving..." : "Save"}
              </button>
              <span className="badge badge-accent">{user?.tier || "free"} plan</span>
            </div>
          </div>
        </section>

        {/* Change Password section */}
        <section style={{ marginBottom: "var(--space-6)" }}>
          <h2 style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)", marginBottom: "var(--space-3)" }}>
            Change Password
          </h2>
          <div className="card" style={{ padding: "var(--space-4)" }}>
            <div className="field" style={{ marginBottom: "var(--space-3)" }}>
              <label className="field-label" htmlFor="current-password">Current Password</label>
              <input
                id="current-password"
                className="input"
                type="password"
                value={currentPassword}
                onChange={(e) => setCurrentPassword(e.target.value)}
                autoComplete="current-password"
              />
            </div>

            <div className="field" style={{ marginBottom: "var(--space-3)" }}>
              <label className="field-label" htmlFor="new-password">New Password</label>
              <input
                id="new-password"
                className="input"
                type="password"
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
                autoComplete="new-password"
                minLength={8}
              />
            </div>

            <div className="field" style={{ marginBottom: "var(--space-3)" }}>
              <label className="field-label" htmlFor="confirm-password">Confirm New Password</label>
              <input
                id="confirm-password"
                className="input"
                type="password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                autoComplete="new-password"
              />
            </div>

            {passwordError && (
              <div role="alert" style={{
                color: "var(--error)",
                fontSize: "var(--text-sm)",
                marginBottom: "var(--space-2)",
              }}>
                {passwordError}
              </div>
            )}

            <button
              className="btn btn-primary"
              onClick={handleChangePassword}
              disabled={savingPassword || !currentPassword || !newPassword}
            >
              {savingPassword ? "Changing..." : "Change Password"}
            </button>
          </div>
        </section>

        {/* BYOK API Keys — always visible so any user can upgrade to BYOK */}
        {(
          <section style={{ marginBottom: "var(--space-6)" }}>
            <h2 style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)", marginBottom: "var(--space-3)" }}>
              API Keys
            </h2>
            <div className="card" style={{ padding: "var(--space-4)" }}>
              <div style={{
                fontSize: "var(--text-sm)",
                color: "var(--text-secondary)",
                marginBottom: "var(--space-3)",
                padding: "var(--space-2)",
                background: "var(--bg-tertiary)",
                borderRadius: "var(--radius-sm)",
              }}>
                Your API keys are stored encrypted. For security, saved keys are never displayed — only replaced.
              </div>

              <div className="field" style={{ marginBottom: "var(--space-3)" }}>
                <label className="field-label" htmlFor="key-anthropic">Anthropic API Key</label>
                <input
                  id="key-anthropic"
                  className="input"
                  type="password"
                  value={anthropicKey}
                  onChange={(e) => setAnthropicKey(e.target.value)}
                  placeholder="sk-ant-..."
                  autoComplete="off"
                />
              </div>

              <div className="field" style={{ marginBottom: "var(--space-3)" }}>
                <label className="field-label" htmlFor="key-openai">OpenAI API Key</label>
                <input
                  id="key-openai"
                  className="input"
                  type="password"
                  value={openaiKey}
                  onChange={(e) => setOpenaiKey(e.target.value)}
                  placeholder="sk-..."
                  autoComplete="off"
                />
              </div>

              <div className="field" style={{ marginBottom: "var(--space-3)" }}>
                <label className="field-label" htmlFor="key-google">Google AI API Key</label>
                <input
                  id="key-google"
                  className="input"
                  type="password"
                  value={googleKey}
                  onChange={(e) => setGoogleKey(e.target.value)}
                  placeholder="AIza..."
                  autoComplete="off"
                />
              </div>

              <button
                className="btn btn-primary"
                onClick={handleSaveKeys}
                disabled={savingKeys || (!anthropicKey.trim() && !openaiKey.trim() && !googleKey.trim())}
              >
                {savingKeys ? "Saving..." : "Save API Keys"}
              </button>
            </div>
          </section>
        )}
      </div>
    </>
  );
}
