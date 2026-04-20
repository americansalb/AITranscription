import { useCallback, useEffect, useState } from "react";
import "./BuildIdentityFooter.css";

/**
 * BuildIdentityFooter — displays the three build SHAs (sidecar, host, ui)
 * so the user can prove which code version is actually running.
 *
 * Per tech-leader msg 655 (round 2) ruling: dev-challenger's composite-view
 * rebuttal wins. Collapsed default when all three SHAs match; expanded
 * "MIXED" form when they diverge. Dirty builds get an asterisk suffix.
 *
 * Data model (matches developer's `get_build_info` from 469f5bc + p2-v2):
 *   - host: BuildComponentInfo — compile-time baked into the Tauri host
 *   - sidecar: BuildComponentInfo | BuildComponentError — probed via
 *     `vaak-mcp --build-info` subprocess. On failure the object carries
 *     `error: "binary_missing" | "probe_failed" | "malformed_output" |
 *     "probe_timeout" | "probe_exited_nonzero"` (snake_case per
 *     tech-leader msg 703 + architect msg 715) plus a `detail?` string.
 *     The component renders the error string verbatim, so pre-p2-v2
 *     binaries emitting space-separated strings still display cleanly.
 *   - ui: BuildComponentInfo — NOT returned by Tauri; injected here from
 *     Vite `import.meta.env.VITE_GIT_*` vars at frontend build time.
 *
 * Click the footer to force-refresh (bypasses the 30s Tauri-side cache via
 * the `invalidate_build_info_cache` command landing in p2-v2).
 */

export interface BuildComponentInfo {
  sha: string;
  dirty: boolean;
  subject?: string;
  commit_date?: string;
  tag?: string | null;
  built_at?: string;
  /** Present only when the probe for this component failed. */
  error?: string;
  /** Free-form diagnostic (truncated stdout/stderr on parse failure). */
  detail?: string;
}

export interface BuildInfoResponse {
  host: BuildComponentInfo;
  sidecar: BuildComponentInfo;
  /** `ui` is injected client-side from Vite env; Tauri does not know it. */
  ui?: BuildComponentInfo;
}

interface BuildIdentityFooterProps {
  /** Test injection for probing. Default invokes `get_build_info`. */
  fetchBuildInfo?: () => Promise<BuildInfoResponse | null>;
  /** Test injection for cache invalidation. Default invokes `invalidate_build_info_cache`. */
  invalidateCache?: () => Promise<void>;
}

const SHORT_SHA_LEN = 7;

function uiBuildInfo(): BuildComponentInfo {
  const env = (import.meta as unknown as { env?: Record<string, string | boolean | undefined> }).env ?? {};
  const sha = (env.VITE_GIT_SHA as string) || "unknown";
  const dirty = env.VITE_GIT_DIRTY === true || env.VITE_GIT_DIRTY === "true";
  const tag = (env.VITE_GIT_TAG as string) || null;
  const commit_date = (env.VITE_GIT_COMMIT_DATE as string) || undefined;
  const subject = (env.VITE_GIT_SUBJECT as string) || undefined;
  const built_at = (env.VITE_BUILT_AT as string) || undefined;
  return { sha, dirty, tag, commit_date, subject, built_at };
}

function shortSha(c: BuildComponentInfo): string {
  const head = (c.sha || "unknown").slice(0, SHORT_SHA_LEN);
  return c.dirty ? `${head}*` : head;
}

function hasError(c: BuildComponentInfo): boolean {
  return Boolean(c.error) || !c.sha || c.sha === "unknown";
}

function allMatch(info: BuildInfoResponse): boolean {
  const ui = info.ui;
  if (!ui) return false;
  if (hasError(info.sidecar) || hasError(info.host) || hasError(ui)) return false;
  return (
    info.sidecar.sha === info.host.sha &&
    info.host.sha === ui.sha &&
    info.sidecar.dirty === info.host.dirty &&
    info.host.dirty === ui.dirty
  );
}

function formatTooltip(c: BuildComponentInfo, label: string): string {
  if (c.error) {
    const parts = [`${label}: ${c.error}`];
    if (c.detail) parts.push(c.detail);
    return parts.join("\n");
  }
  const parts: string[] = [`${label}: ${(c.sha || "unknown").slice(0, 12)}${c.dirty ? " (dirty)" : ""}`];
  if (c.tag) parts.push(`tag ${c.tag}`);
  if (c.subject) parts.push(`— ${c.subject}`);
  if (c.commit_date) parts.push(`committed ${c.commit_date}`);
  if (c.built_at) parts.push(`built ${c.built_at}`);
  return parts.join("\n");
}

async function defaultFetchBuildInfo(): Promise<BuildInfoResponse | null> {
  if (typeof window === "undefined" || !(window as unknown as { __TAURI__?: unknown }).__TAURI__) {
    return null;
  }
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const resp = await invoke<BuildInfoResponse>("get_build_info");
    return { ...resp, ui: uiBuildInfo() };
  } catch {
    return null;
  }
}

async function defaultInvalidateCache(): Promise<void> {
  if (typeof window === "undefined" || !(window as unknown as { __TAURI__?: unknown }).__TAURI__) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke<void>("invalidate_build_info_cache");
  } catch {
    // Command may not be registered in pre-p2-v2 binaries — silently swallow.
  }
}

export function BuildIdentityFooter({
  fetchBuildInfo = defaultFetchBuildInfo,
  invalidateCache = defaultInvalidateCache,
}: BuildIdentityFooterProps) {
  const [info, setInfo] = useState<BuildInfoResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);

  const runFetch = useCallback(() => {
    let cancelled = false;
    setLoading(true);
    setFailed(false);
    fetchBuildInfo()
      .then((result) => {
        if (cancelled) return;
        if (result) setInfo(result);
        else setFailed(true);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [fetchBuildInfo]);

  useEffect(() => {
    const cancel = runFetch();
    return cancel;
  }, [runFetch]);

  const handleRefresh = async () => {
    await invalidateCache();
    runFetch();
  };

  if (loading) {
    return (
      <div className="build-identity-footer" aria-live="polite" aria-label="Build identity">
        <span className="build-identity-label">build</span>
        <span className="build-identity-loading">…</span>
      </div>
    );
  }

  if (failed || !info) {
    return (
      <div className="build-identity-footer build-identity-footer-unknown" aria-label="Build identity unavailable">
        <span className="build-identity-label">build</span>
        <button
          type="button"
          className="build-identity-unknown"
          title="get_build_info command unavailable — likely an older binary. Click to retry."
          onClick={handleRefresh}
        >
          unknown
        </button>
      </div>
    );
  }

  const ui = info.ui ?? uiBuildInfo();
  const fullInfo: BuildInfoResponse = { ...info, ui };
  const unified = allMatch(fullInfo);
  const anyDirty = fullInfo.sidecar.dirty || fullInfo.host.dirty || ui.dirty;
  const anyError = hasError(fullInfo.sidecar) || hasError(fullInfo.host) || hasError(ui);

  if (unified) {
    const tagLabel = fullInfo.sidecar.tag ?? null;
    const display = tagLabel ?? shortSha(fullInfo.sidecar);
    const tooltip = [
      formatTooltip(fullInfo.sidecar, "sidecar"),
      formatTooltip(fullInfo.host, "host"),
      formatTooltip(ui, "ui"),
      "Click to refresh.",
    ].join("\n\n");
    return (
      <div
        className="build-identity-footer"
        aria-label={`Build ${display}${anyDirty ? ", dirty working tree" : ""}. Click to refresh.`}
      >
        <span className="build-identity-label">build</span>
        <button type="button" className="build-identity-sha" title={tooltip} onClick={handleRefresh}>
          {display}
        </button>
      </div>
    );
  }

  return (
    <div
      className={`build-identity-footer build-identity-footer-mixed${anyError ? " build-identity-footer-error" : ""}`}
      aria-label="Build identity MIXED — sidecar, host, and ui are not from the same commit. Click to refresh."
      role="alert"
    >
      <span className="build-identity-label">build</span>
      <button
        type="button"
        className="build-identity-mixed-tag"
        title={anyError ? "Probe error — click to retry." : "Binaries are from different commits — rebuild to realign. Click to refresh."}
        onClick={handleRefresh}
      >
        {anyError ? "ERROR" : "MIXED"}
      </button>
      <span className="build-identity-triplet">
        <span className="build-identity-component" title={formatTooltip(fullInfo.sidecar, "sidecar")}>
          sidecar=
          <span className={`build-identity-component-sha${hasError(fullInfo.sidecar) ? " build-identity-component-err" : ""}`}>
            {hasError(fullInfo.sidecar) ? fullInfo.sidecar.error ?? "?" : shortSha(fullInfo.sidecar)}
          </span>
        </span>
        <span className="build-identity-component" title={formatTooltip(fullInfo.host, "host")}>
          host=
          <span className={`build-identity-component-sha${hasError(fullInfo.host) ? " build-identity-component-err" : ""}`}>
            {hasError(fullInfo.host) ? fullInfo.host.error ?? "?" : shortSha(fullInfo.host)}
          </span>
        </span>
        <span className="build-identity-component" title={formatTooltip(ui, "ui")}>
          ui=
          <span className={`build-identity-component-sha${hasError(ui) ? " build-identity-component-err" : ""}`}>
            {hasError(ui) ? ui.error ?? "?" : shortSha(ui)}
          </span>
        </span>
      </span>
    </div>
  );
}
