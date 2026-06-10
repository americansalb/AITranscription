# ui2/LATER.md — out-of-scope ideas and honest deferrals (decree §12)

Items discovered mid-build. Nothing here is shipped or promised.

## Deferrals (needed before cutover GO, §10 Phase 5)
- ~~Bundle font files~~ DONE (second commit): @fontsource Space Grotesk 600 / Inter 400-600 / JetBrains Mono 400, imported in Ui2App — offline from clean clone.
- ~~Smoke test~~ DONE in spirit (second commit): `__tests__/smoke.test.tsx` walks launch → feed → card → choose → mute with mocked Tauri APIs (RTL/jsdom). A true Playwright-driving-the-real-webview run is still open — jsdom verifies wiring, not the webview.
- ~~3-platform CI~~ WORKFLOW ADDED (`.github/workflows/ui2-ci.yml`): npm build + ui2 tests on macOS/Windows/Linux + cargo check of the shell, per push. First green run unverified until pushed; the FULL bundled tauri build remains build.yml (manual/tags) — §3.1's strictest reading (bundle from clean clone in CI per push) is intentionally not burned on every commit.
- ~~Derivation perf~~ MEASURED (second commit): 5k-message board derives in ~64ms + 8ms reconcile (was 629ms — deriveDock was quadratic, fixed with a reply index; perf.test.ts holds a 250ms CI-safe bound). Paint-side numbers (keystroke <16ms, 60fps scroll) still unmeasured — need the real webview.
- **Coverage number**: 27 tests over classify/digest/dock/liveness + the smoke path. @vitest/coverage-v8 not installed, so the ≥80% figure is unquantified.

## Ideas (not authorized, parked)
- Per-discussion keys for *continuous* discussions: today all continuous-review rounds share one "discussion-active" key, so consecutive continuous discussions fold into one row whose verdict is the latest end-event. Fine for digesting ceremony; revisit if operators want one row per continuous topic.
- Day-row collapse for old R7 bursts (pre-approved lever from the IA table if Phase 2 measurement exceeds the ~10-row target).
- Seat-name tooltips → a roster popover in the top strip.
- `@target` autocomplete in the composer.
