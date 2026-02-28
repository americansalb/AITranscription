import { useState, useEffect, useCallback, useRef } from "react";
import { isMacOS } from "../lib/platform";
import "./MacPermissionWizard.css";

interface MacPermissions {
  automation: boolean;
  accessibility: boolean;
  screen_recording: boolean;
  platform: string;
}

interface PermissionRow {
  key: keyof Omit<MacPermissions, "platform">;
  name: string;
  description: string;
  settingsPane: string;
}

const PERMISSIONS: PermissionRow[] = [
  {
    key: "accessibility",
    name: "Accessibility",
    description: "Paste text into other apps and simulate keyboard input",
    settingsPane: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
  },
  {
    key: "screen_recording",
    name: "Screen Recording",
    description: "Capture screen for AI screen reader",
    settingsPane: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
  },
  {
    key: "automation",
    name: "Automation (Terminal)",
    description: "Launch and manage AI team members",
    settingsPane: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation",
  },
];

interface MacPermissionWizardProps {
  onClose: () => void;
}

export function MacPermissionWizard({ onClose }: MacPermissionWizardProps) {
  const [perms, setPerms] = useState<MacPermissions | null>(null);
  const [micGranted, setMicGranted] = useState<boolean | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const checkPermissions = useCallback(async () => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<MacPermissions>("check_macos_permissions");
        setPerms(result);
      }
    } catch {
      // Non-critical — keep showing current state
    }

    // Check microphone via browser API
    try {
      const status = await navigator.permissions.query({ name: "microphone" as PermissionName });
      setMicGranted(status.state === "granted");
    } catch {
      // permissions.query may not support "microphone" on all platforms
      // Fall back to assuming unknown
      setMicGranted(null);
    }
  }, []);

  // Initial check + 3-second polling
  useEffect(() => {
    checkPermissions();
    pollRef.current = setInterval(checkPermissions, 3000);
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [checkPermissions]);

  // Auto-close when all permissions are granted
  const allTauriGranted = perms
    ? perms.accessibility && perms.automation && perms.screen_recording
    : false;
  const allGranted = allTauriGranted && micGranted === true;

  useEffect(() => {
    if (allGranted) {
      // Brief delay to show the all-green state before closing
      const timeout = setTimeout(() => {
        localStorage.setItem("vaak_macos_wizard_dismissed", "1");
        onClose();
      }, 1500);
      return () => clearTimeout(timeout);
    }
  }, [allGranted, onClose]);

  // Escape to close
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        handleDismiss();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  const handleDismiss = () => {
    localStorage.setItem("vaak_macos_wizard_dismissed", "1");
    onClose();
  };

  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      handleDismiss();
    }
  };

  const openSystemSettings = async (pane: string) => {
    try {
      if (window.__TAURI__) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("open_macos_settings", { paneUrl: pane });
      }
    } catch {
      // Fallback: try opening generic Privacy & Security
      try {
        if (window.__TAURI__) {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("open_macos_settings", {
            paneUrl: "x-apple.systempreferences:com.apple.preference.security",
          });
        }
      } catch {
        // Last resort — do nothing
      }
    }
  };

  const requestMicrophone = async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      // Got permission — stop the stream immediately
      stream.getTracks().forEach(t => t.stop());
      setMicGranted(true);
    } catch {
      setMicGranted(false);
    }
  };

  const grantedCount =
    (micGranted === true ? 1 : 0) +
    (perms?.accessibility ? 1 : 0) +
    (perms?.screen_recording ? 1 : 0) +
    (perms?.automation ? 1 : 0);

  const totalCount = 4;

  return (
    <div className="mac-wizard-overlay" onClick={handleBackdropClick}>
      <div className="mac-wizard-modal" role="dialog" aria-label="macOS Setup">
        <div className="mac-wizard-header">
          <div className="mac-wizard-header-left">
            <span className="mac-wizard-icon">
              <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
                <path d="M10 2C5.58 2 2 5.58 2 10s3.58 8 8 8 8-3.58 8-8-3.58-8-8-8zm0 14.4A6.4 6.4 0 1 1 10 3.6a6.4 6.4 0 0 1 0 12.8zm-.8-4h1.6v1.6H9.2v-1.6zm0-6.4h1.6v4.8H9.2V6z" fill="currentColor"/>
              </svg>
            </span>
            <h2 className="mac-wizard-title">macOS Setup</h2>
          </div>
          <button className="mac-wizard-close-btn" onClick={handleDismiss} aria-label="Close">
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path d="M1 1L13 13M13 1L1 13" stroke="currentColor" strokeWidth="2" strokeLinecap="round"/>
            </svg>
          </button>
        </div>

        <p className="mac-wizard-subtitle">
          Vaak needs a few permissions to work properly on macOS.
        </p>

        <div className="mac-wizard-permissions" role="list">
          {/* Microphone row — handled via browser API */}
          <div
            className={`mac-wizard-row ${micGranted === true ? "granted" : ""}`}
            role="listitem"
          >
            <div className="mac-wizard-row-icon">
              {micGranted === true ? (
                <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                  <circle cx="9" cy="9" r="8" fill="#34c759"/>
                  <path d="M5 9.5l2.5 2.5L13 6" stroke="#fff" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/>
                </svg>
              ) : (
                <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                  <circle cx="9" cy="9" r="8" stroke="#888" strokeWidth="1.5" fill="none"/>
                </svg>
              )}
            </div>
            <div className="mac-wizard-row-content">
              <div className="mac-wizard-row-name">Microphone</div>
              <div className="mac-wizard-row-desc">Record voice for transcription</div>
            </div>
            <div className="mac-wizard-row-action">
              {micGranted === true ? (
                <span className="mac-wizard-status granted">Granted</span>
              ) : (
                <button className="mac-wizard-settings-btn" onClick={requestMicrophone}>
                  Request Access
                </button>
              )}
            </div>
          </div>

          {/* Tauri permission rows */}
          {PERMISSIONS.map((perm) => {
            const isGranted = perms ? perms[perm.key] : false;
            return (
              <div
                key={perm.key}
                className={`mac-wizard-row ${isGranted ? "granted" : ""}`}
                role="listitem"
              >
                <div className="mac-wizard-row-icon">
                  {isGranted ? (
                    <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                      <circle cx="9" cy="9" r="8" fill="#34c759"/>
                      <path d="M5 9.5l2.5 2.5L13 6" stroke="#fff" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/>
                    </svg>
                  ) : (
                    <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                      <circle cx="9" cy="9" r="8" stroke="#888" strokeWidth="1.5" fill="none"/>
                    </svg>
                  )}
                </div>
                <div className="mac-wizard-row-content">
                  <div className="mac-wizard-row-name">{perm.name}</div>
                  <div className="mac-wizard-row-desc">{perm.description}</div>
                </div>
                <div className="mac-wizard-row-action">
                  {isGranted ? (
                    <span className="mac-wizard-status granted">Granted</span>
                  ) : (
                    <button
                      className="mac-wizard-settings-btn"
                      onClick={() => openSystemSettings(perm.settingsPane)}
                      aria-label={`Open ${perm.name} settings in System Settings`}
                    >
                      Open System Settings
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>

        <div className="mac-wizard-footer" aria-live="polite">
          <span className="mac-wizard-progress">
            {grantedCount} of {totalCount} permissions granted
          </span>
          <div className="mac-wizard-footer-actions">
            <button className="mac-wizard-refresh-btn" onClick={checkPermissions}>
              Refresh Status
            </button>
            {allGranted ? (
              <span className="mac-wizard-all-granted">All set!</span>
            ) : (
              <button className="mac-wizard-dismiss-btn" onClick={handleDismiss}>
                Continue Anyway
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * Hook to determine if the permission wizard should be shown.
 * Returns [shouldShow, setShow] — only true on macOS, first launch, when permissions are missing.
 */
export function useMacPermissionWizard(): [boolean, (v: boolean) => void] {
  const [show, setShow] = useState(false);

  useEffect(() => {
    if (!isMacOS()) return;
    if (!window.__TAURI__) return;
    if (localStorage.getItem("vaak_macos_wizard_dismissed") === "1") return;

    // Check if any permission is missing
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const perms = await invoke<MacPermissions>("check_macos_permissions");
        if (!perms.accessibility || !perms.automation || !perms.screen_recording) {
          setShow(true);
        }
      } catch {
        // If we can't check, don't show
      }
    })();
  }, []);

  return [show, setShow];
}
