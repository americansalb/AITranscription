# Section (f) — Platform Contract

**Author:** platform-engineer:0
**Reviewer:** evil-architect:0 (adversarial)
**Status:** DRAFT v0.2 — 2026-04-20 (addresses F1–F6 from evil-architect:0 msg 1544)
**Parent doc:** `.vaak/specs/pipeline-contract.md` (sections a/b/c by architect:0, d by tester:1, e by ux-engineer:0, g by architect:0+evil-architect:0)

This section defines cross-OS platform behavior the pipeline depends on. It answers: *what does "works on this OS" mean, and what breaks silently if it isn't true?*

---

## f.0 Active-target scope

The pipeline currently targets **Windows x86_64 only** (`x86_64-pc-windows-msvc`). macOS and Linux are knowledge targets, not build targets. Every requirement below applies to Windows by default. POSIX requirements apply when macOS/Linux are added to the build.

Evidence: `desktop/src-tauri/Cargo.toml` + build CI.

---

## f.1 File locking

### f.1.1 Contract

Every read or write to `.vaak/board.jsonl` MUST hold `board.lock`. Every read or write to `.vaak/discussion.json` MUST hold `discussion.lock`. The unlocked variant (`write_discussion_state_unlocked`, `vaak-mcp.rs:2251`) is callable ONLY from inside a `with_discussion_lock` scope.

### f.1.2 Platform semantics

| OS | API | Semantics | Concurrent-access behavior |
|---|---|---|---|
| Windows | `LockFileEx` via `fs2::FileExt::lock_exclusive` | **Mandatory** | `ERROR_SHARING_VIOLATION` (Win32 err 32) → `ErrorKind::PermissionDenied` |
| macOS | `flock` via `fs2::FileExt::lock_exclusive` | **Advisory** | Silent interleave risk if a writer bypasses the lock |
| Linux | `flock` via `fs2::FileExt::lock_exclusive` | **Advisory** | Same as macOS |

### f.1.3 Requirements

- **R-f1.1:** All lock calls MUST go through `fs2::FileExt` (not `std::fs::File::try_lock_exclusive` or `LockFileEx` direct). This abstracts over OS lock semantics.
- **R-f1.2:** There MUST be exactly one lock file per `.vaak/` resource. Do not introduce a second lock for the same file.
- **R-f1.3:** Any new `.vaak/` file added post-contract MUST declare its lock file in this section before the PR merges.

### f.1.4 Known history

Per tester:1 msg 1402, the prior `project_lock_unify_deferred.md` memory claimed a Tauri/MCP split on `discussion.json`. Code-read verified matching locks per resource. `pr-lock-unify` (T6 follow-up) is a lock-audit ticket that may close as obsolete.

---

## f.2 Watchdog clock source

### f.2.1 Contract

The auto-skip watchdog in `auto_skip_stale_pipeline_stage` MUST use a monotonic clock (`std::time::Instant`). Wall-clock (`SystemTime` / ISO-string epoch math) is forbidden for the destructive-path timer.

### f.2.2 Platform divergence (why this matters)

| OS | Monotonic source | Wall-clock source | Drift under NTP/sleep |
|---|---|---|---|
| Windows | `QueryPerformanceCounter` | W32Time service | Wall can step ≥1s silently under MaxPosPhaseCorrection settings |
| macOS | `CLOCK_MONOTONIC` | `timed` daemon | Smaller but nonzero wall steps |
| Linux | `CLOCK_MONOTONIC` | `systemd-timesyncd` | Same class as macOS |

### f.2.3 Requirements

- **R-f2.1:** `pipeline_stage_started_at` MUST be stored as an `Instant`-anchored duration per session, not as an ISO timestamp.
- **R-f2.2:** Stage-age comparisons MUST compute `Instant::now().saturating_duration_since(started_at)`, never `ISO_parse(now) - ISO_parse(stored_hb)`.
- **R-f2.3:** The stall-warning path (Item B of `pr-pipeline-gate-observability`) already uses the correct monotonic pattern at `vaak-mcp.rs:381-470`. The auto-skip destructive path MUST adopt the same pattern. Tracked as T2 `pr-watchdog-monotonic-clock`.
- **R-f2.4 (sidecar-restart safety, per F1):** `Instant` is per-process — a sidecar crash+respawn resets the `Instant` base. When the watchdog logic encounters a stage whose `Instant`-anchor was lost (sidecar restart detected via cache-layer continuity in f.4.3 without matching in-memory `Instant`), it MUST restart the timer at 0 and NOT auto-skip. False-negative (late skip) is safer than false-positive (killing a live agent whose sidecar just respawned).

### f.2.4 Test requirement

Per tester:1 msg 1499, the test MUST inject a mocked `Clock` trait to simulate wall-clock stepping without requiring a real NTP event. Forward-step and backward-step cases both required.

### f.2.5 Threshold provenance (cross-ref section d)

The default 300s watchdog threshold is DEFERRED to section (d) (tester:1's threshold-provenance work). This section only pins the CLOCK SOURCE requirement; it does not pin the THRESHOLD VALUE.

---

## f.3 Sidecar lifecycle

### f.3.1 Contract

An MCP sidecar's lifecycle is bounded below by its Claude Code parent terminal. The sidecar MUST exit cleanly when the parent exits. The sidecar MUST remain active as long as the parent is alive AND the agent is expected to participate in collab.

### f.3.2 Platform divergence

**Windows:**
- Claude Code spawns the MCP sidecar as a child process. Default `CREATE_BREAKAWAY_FROM_JOB=false` → sidecar joins parent's Job Object.
- **Parent exits → Job Object auto-reaps children.** Sidecar dies automatically. No explicit exit code path needed.
- **Parent alive + sidecar idle → Claude Code's internal idle-detector may reap the sidecar via Job Object.** This is the observed drop-out failure mode for dev-challenger:0 (4/26 stages auto-skipped, 100% same role).
- No `SIGCHLD` / `waitpid` pattern exists on Windows; lifecycle relies on Job Object inheritance.

**Unix (macOS, Linux):**
- A parent-process monitor thread at `vaak-mcp.rs:9144-9160` polls `getppid()` every 2s and exits when the parent dies. This is the **inverse** direction (parent-dies → child-exits cleanly), not the idle-kill direction.
- Claude Code may still idle-kill a Unix child, but the Unix `SIGCHLD` handler gives sidecars a signal path to announce exit that Windows does not have.

### f.3.3 Requirements

- **R-f3.1:** Agents participating in collab MUST re-enter `project_wait` after every broadcast to keep the sidecar blocked in a syscall (preventing Claude Code's idle-detector from reaping). This is the T1 briefing-rule fix.
- **R-f3.2:** The contract does NOT require sidecars survive user-initiated terminal close. A user closing Claude Code and reopening it is a new session; identity continuity is addressed in section f.4.
- **R-f3.3:** The Unix parent-monitor thread SHOULD NOT be ported to Windows under current default spawn semantics (Job Object inheritance auto-reaps). If `CREATE_BREAKAWAY_FROM_JOB=true` is ever adopted (e.g., Tauri-spawn-and-orphan pattern), this requirement is re-evaluated and a Windows parent-monitor equivalent may become necessary.

### f.3.4 Observability

Drop-out events are currently detectable only by the 300s watchdog (section f.2) firing an auto-skip. The contract requires no additional lifecycle instrumentation; if the team adds it later, it belongs in section (d) observability.

---

## f.4 Session-identity source chain

### f.4.1 Contract

Every MCP sidecar instance MUST resolve a stable `session_id` via the priority chain defined in `vaak-mcp.rs:7478-7528` (`get_session_id`). The chain's purpose is to preserve identity across sidecar restarts under the same Claude Code parent.

### f.4.2 Priority chain (CANONICAL, with justification per F3)

Priority ordering rationale: **higher-priority sources are those that Claude Code or the project has direct control over** (and can assert stable semantics for); **lower-priority sources are environmental fallbacks** (best-effort inference from terminal host).

1. `CLAUDE_SESSION_ID` env var — **reserved for future Claude Code integration.** If Claude Code propagates this, it becomes the single source of truth (Claude Code owns the session lifecycle, can guarantee stability). Not currently set.
2. `WT_SESSION` env var — Windows Terminal per-tab UUID. Not currently propagated to the sidecar (see f.4.4). Stable per-tab within a Windows Terminal instance; does NOT survive closing+reopening a tab.
3. `ITERM_SESSION_ID` env var — iTerm2 on macOS. Similar lifetime to WT_SESSION.
4. `TERM_SESSION_ID` env var — generic terminal emulators. Weakest of the env tier (some terminals don't set it, some regenerate per child-spawn).
5. Windows console window handle (`HWND` via `GetConsoleWindow`) — Windows-only fallback. Stable while the console window exists; new window = new HWND.
6. Unix TTY path (`/dev/pts/N`) — POSIX fallback. Stable while the TTY exists; re-used after close.
7. **Generate fallback hash** — `hash(hostname, parent_pid, cwd, user)` as `format!("{}-{:016x}", hostname, hasher.finish())`. Last-resort; collides when parent_pid recycles (see f.4.3 cache-layer concerns).

**When T2 `VAAK_TERMINAL_ID` lands (f.4.6), it slots in as priority 0** (ABOVE `CLAUDE_SESSION_ID`) because it is Tauri-managed and designed specifically for terminal-restart survival — a property no current source guarantees.

### f.4.3 Cache layer

Independent of the chain above, `cache_session_id` / `read_cached_session_id` (`vaak-mcp.rs:7556-7599`) persist the resolved id to `%APPDATA%\Vaak\session-cache\{ppid}.txt` (Windows) or `~/.vaak/session-cache/{ppid}.txt` (POSIX). The cache survives sidecar crash-and-respawn under the same Claude Code parent PID.

### f.4.4 Empirical state (observed as of 2026-04-20, NOT a contract requirement)

**This subsection records observed behavior, not specified behavior. It is time-sensitive and MUST be periodically re-verified per the amendment procedure (f.9). If Claude Code or Tauri integration changes how env is propagated, this subsection becomes stale and the spec MUST be re-validated.**

Live-env verification in platform-engineer:0 msg 1435 (reproducible via `env | grep -iE "(VAAK|CLAUDE|WT|ITERM|TERM)"` inside a running sidecar):
- No sidecar on this machine has any identity env var set beyond the three `CLAUDE_CODE_*` vars: `CLAUDECODE=1`, `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_EXECPATH`.
- All 7 agent sessions resolve to the fallback-hash tier (tier 7), session_id format `{hostname}-{16hex}`.
- Claude Code does NOT currently propagate `WT_SESSION` or any session env to the spawned sidecar.

**Recommended re-verification cadence:** at every Claude Code major-version upgrade, and whenever Tauri spawn configuration changes.

### f.4.5 Requirements

- **R-f4.1:** The priority order above is canonical. Reordering requires updating this section.
- **R-f4.2:** Adding a new source requires a platform-engineer seat review to verify the source's restart-survival characteristics across Windows/macOS/Linux targets.
- **R-f4.3:** The cache layer MUST NOT rely on parent PID alone for correctness across Claude Code restarts. It is a within-parent-lifetime optimization only.
- **R-f4.4:** Cache files MUST be mtime-TTL'd to prevent stale-PPID-collision (T9 `pr-session-cache-cleanup`, ~15 LOC). Default TTL: 7 days. Low priority.

### f.4.6 Future extension (NOT in current contract)

T2 `pr-identity-vaak-terminal-id` (platform-engineer:0 msg 1396 + 1435) proposes adding `VAAK_TERMINAL_ID` as priority-0 ABOVE `CLAUDE_SESSION_ID`, with Tauri generating a UUID and injecting it at terminal spawn. This survives Claude Code restarts (the one case the cache layer cannot handle). Requires empirical verification on Windows Terminal / cmd.exe / PowerShell console host that env survives user-initiated terminal close+reopen, OR a Tauri-side always-inject-at-spawn pattern that sidesteps the question.

T2 is NOT part of the current contract. Adding it requires a section-f amendment.

---

## f.5 Terminal-host environment

### f.5.1 Contract

The sidecar inherits its env from Claude Code's child-spawn. The pipeline MUST NOT assume any specific env var is present beyond `PATH`, `HOME`/`USERPROFILE`, and the three `CLAUDE_CODE_*` vars that Claude Code sets today.

### f.5.2 Platform divergence

- **Windows Terminal** (`wt.exe`) injects `WT_SESSION`, `WT_PROFILE_ID`. Not inherited by Claude Code child-spawn as of 2026-04-20.
- **Windows console host** (`cmd.exe`, PowerShell in conhost.exe) does NOT inject a session env. Console window handle (`HWND`) is the fallback identity.
- **macOS Terminal.app + iTerm2** inject `TERM_SESSION_ID` and `ITERM_SESSION_ID` respectively; propagation to Claude Code children not empirically verified.
- **Linux gnome-terminal + tmux** inject `TERM`, `TMUX`, `COLORTERM`; no session identity env is standard.

### f.5.3 Requirements

- **R-f5.1:** Any new feature that assumes an env var MUST check presence + fall back gracefully.
- **R-f5.2:** Env-based identity sources (section f.4) are best-effort, NOT guaranteed.

---

## f.6 State persistence (three layers, per F5)

### f.6.1 Contract

The pipeline depends on three distinct persistence layers, each with its own OS-path rules, locking requirements, and migration needs:

- **Layer 1 — frontend user-prefs:** `localStorage` via `loadSetting`/`saveSetting` in React components. Tauri WebView user-data directory.
- **Layer 2 — team-scope state:** `.vaak/project.json` (roles, settings.discussion_mode, participant list, etc.). File-locked per f.1.
- **Layer 3 — per-role runtime state:** `.vaak/sessions.json` (bindings, heartbeats, activity). File-locked per f.1.

### f.6.2 Platform paths (Layer 1 only; Layers 2 and 3 live inside project dir)

| OS | Layer 1 (WebView user-data) | Layer 2 / 3 (.vaak/) |
|---|---|---|
| Windows | `%LOCALAPPDATA%\Vaak\EBWebView\` | project-relative `.vaak/` (user-chosen dir) |
| macOS | `~/Library/Application Support/Vaak/WebView/` | project-relative `.vaak/` |
| Linux | `~/.config/Vaak/WebView/` or equivalent | project-relative `.vaak/` |

### f.6.3 Requirements (Layer 1)

- **R-f6.1:** Settings-key renames MUST ship a migration in `loadSetting` that reads the old key, writes the new key, returns the new value on first load. Preserves user state across upgrade.
- **R-f6.2:** Changing the Tauri WebView data-directory (via `tauri.conf.json`) WILL wipe user settings. Any such change MUST include an export/import path announced before release.
- **R-f6.3:** A central `SETTINGS_KEYS` const SHOULD enumerate every Layer-1 key. Tests assert keys remain readable after each PR.

### f.6.4 Requirements (Layer 2 — `.vaak/project.json`)

- **R-f6.4:** Schema changes to `project.json` MUST be backward-compatible via `#[serde(default)]` on new fields OR ship a migration in the Rust deserializer.
- **R-f6.5:** `project.json` write path MUST hold `project.lock` OR the team's unified `.vaak/` lock (whichever f.1 declares). Do NOT add a third lock file for `project.json`.
- **R-f6.6:** A central `PROJECT_SCHEMA_VERSION` field SHOULD gate schema migrations for forward-compat.

### f.6.5 Requirements (Layer 3 — `.vaak/sessions.json`)

- **R-f6.7:** `sessions.json` writes MUST hold the appropriate f.1 lock.
- **R-f6.8:** Binding-schema changes (new fields on `Binding` struct) MUST be backward-compatible via `#[serde(default)]`.
- **R-f6.9:** Stale binding cleanup policy (auto-expire bindings whose `last_heartbeat` is > N minutes old) is currently implicit; the cleanup threshold SHOULD be documented in this subsection when the team next revisits.

---

## f.7 Accessibility (cross-ref section e UI rendering contract)

### f.7.1 Contract

The app targets Windows users, including users with Windows High Contrast Mode enabled and/or Windows Narrator / third-party screen readers (JAWS, NVDA). Accessibility is load-bearing per `CLAUDE.md` directives.

### f.7.2 Platform-specific a11y requirements

- **R-f7.1 — `forced-colors` compliance:** CSS MUST handle `@media (forced-colors: active)` explicitly for any element with custom color, border, focus, or background. Default browser `forced-color-adjust: auto` applies to standard elements; custom UIs require explicit handling.
- **R-f7.2 — Windows UIA compatibility:** React ARIA attributes must translate cleanly through WebView2's UIA bridge. Custom dropdowns, virtualized lists, portal-rendered modals need manual UIA verification.
- **R-f7.3 — Keyboard-only navigation:** Tab order MUST be logical. `tabIndex={activeTab === "X" ? 0 : -1}` conditional patterns MUST have a regression test (T11c).
- **R-f7.4 — Focus visibility in HC:** focus rings via `outline` / `box-shadow` SHOULD set `outline-style: solid` + allow `forced-color-adjust` defaults.

### f.7.3 Test requirements

Split by runner (tester:1 msg 1499):

- **T11a + T11c** in vitest: DOM-level axe-core + `user.tab()` keyboard-nav tests. Blocks UI PRs in CI.
- **T11b + T11d** in Playwright: `forced-colors` rendering + Windows Narrator smoke. Non-blocking, runs nightly.

### f.7.4 Cross-refs

- CLAUDE.md "screen-reader mode" = output-layer contract (Claude describes visuals). NOT the same as Windows Narrator. See platform-engineer:0 msg 1435 FINDING 1.
- Tauri's "screen-reader window" = one of 4 app windows. Distinct render tree; MUST be enumerated in T11 test matrix.

---

## f.8 Cross-references to other sections

- **Section (a) advancement / (b) gate / (c) send-pattern:** pipeline behavioral contract. Platform section assumes these are spec-complete.
- **Section (c') `end_of_stage` type contract:** developer:0-drafted. Platform section relies on boolean-true invariant.
- **Section (d) threshold + observation:** tester:1-drafted. Section f.2 declares clock-source; section (d) declares threshold VALUE.
- **Section (e) UI rendering contract:** ux-engineer:0-drafted. Section f.7 declares platform-specific a11y; section (e) declares visual/UI-tree invariants.
- **Section (g) population-level SLO:** architect:0 + evil-architect:0. Platform section does not declare population SLOs.

---

## f.9 Amendment procedure

Any change to this section MUST:
1. Be reviewed by evil-architect:0 adversarially (per no-self-certification from evil-architect:0 msg 1430 ATTACK 7).
2. Cite the empirical source (code line + commit SHA, or live-runtime measurement).
3. Update any downstream sections that cross-reference the changed requirement.

**Reviewer fallback (per F6):** if evil-architect:0 seat has been vacant for > 24h at the time an amendment is proposed, a **2-of-3 peer review substitutes** — architect:0 + one other seat who is not the amendment author. The fallback review MUST be explicit ("in the absence of evil-architect:0, reviewing per f.9 fallback procedure") and recorded on the board.

Additions (new OS targets, new identity sources, new lock files, new persistence layers) require a platform-engineer review AND an architect sign-off for scope impact.

**Empirical-state re-verification:** f.4.4 is time-sensitive and MUST be re-verified at every Claude Code major-version upgrade and whenever Tauri spawn configuration changes. A routine re-verification (quick `env` grep inside a live sidecar + comparison to recorded results) is a valid amendment.

---

**End of section (f).**
