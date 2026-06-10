# ui2/LATER.md — out-of-scope ideas and honest deferrals (decree §12)

Items discovered mid-build. Nothing here is shipped or promised.

## Deferrals (needed before cutover GO, §10 Phase 5)
- ~~Bundle font files~~ DONE (second commit): @fontsource Space Grotesk 600 / Inter 400-600 / JetBrains Mono 400, imported in Ui2App — offline from clean clone.
- ~~Smoke test~~ DONE twice over: `__tests__/smoke.test.tsx` (jsdom wiring check) AND `e2e/ui2.spec.ts` — Playwright driving REAL Chromium against the production bundle, walking the five-step path plus an experience-first-mute scenario. Recorded limitation (register ②): Tauri IPC is mocked at `window.__TAURI_INTERNALS__`, so the Rust commands and WebView2-specific paint are not exercised; everything above that boundary is the shipped code.
- ~~3-platform CI~~ WORKFLOW ADDED (`.github/workflows/ui2-ci.yml`): npm build + ui2 tests on macOS/Windows/Linux + cargo check of the shell, per push. First green run unverified until pushed; the FULL bundled tauri build remains build.yml (manual/tags) — §3.1's strictest reading (bundle from clean clone in CI per push) is intentionally not burned on every commit.
- ~~Derivation perf~~ MEASURED (second commit): 5k-message board derives in ~15ms + reconcile (was 629ms — deriveDock was quadratic, fixed with a reply index; perf.test.ts holds a 250ms CI-safe bound).
- ~~Paint-side perf (register ③)~~ MEASURED in real Chromium on the production bundle (e2e/ui2.spec.ts): 5k board initial render **183ms** (§7 bar: <1s) · **0** long tasks while typing 60 chars (keystroke→paint proxy) · scroll **60.9fps** (§7 bar: 60fps). Bars asserted in CI (e2e job). WebView2-native numbers remain unmeasured — same engine family, but record the caveat at cutover.
- **Coverage number**: 27 tests over classify/digest/dock/liveness + the smoke path. @vitest/coverage-v8 not installed, so the ≥80% figure is unquantified.

- **actions/checkout@v4 Node-20 deprecation** (CI run annotation, evil-architect msg 312): forced Node-24 from 2026-06-16 — bump to checkout@v5 across workflows before then. Non-blocking today.
- **Concurrent non-Oxford discussions share one identity slot** (dev-challenger msg 309 residual 2): lifecycle records carry no discussion id, so perfect attribution is impossible UI-side. Sequential case is fixed; the concurrent fix is engine-side metadata = a §8 STOP-and-card item.

## §7 bars vs CI gates (recorded decision, Review #22 — for the Phase 5 cutover card)
| §7 bar | CI gate | Gap handling |
|---|---|---|
| initial render < 1s (5k board) | gated 1:1 (`<1000ms`) | none |
| keystroke→paint < 16ms | 50ms-longtask proxy, ≤1 | 16–49ms stalls invisible to the gate; true bar attested per release on real hardware |
| scroll 60fps | `>50` | shared-runner variance allowance; 60fps verified per release on real hardware (2026-06-10: 60.3/60.6/60.8/60.9 across 3 machines) |

Threshold edits require a register entry — never silent (msg 335 lesson, reaffirmed msgs 367/368/369).

- **Mock-rot guard** (msg 365): e2e/tauriMock.ts hand-maintains the ParsedProject shape vs collab.rs:542. Cross-reference added in the mock header; the durable fix is a Rust `#[test]` that serializes a sample ParsedProject as the fixture the e2e consumes.

## Ideas (not authorized, parked)
- Per-discussion keys for *continuous* discussions: today all continuous-review rounds share one "discussion-active" key, so consecutive continuous discussions fold into one row whose verdict is the latest end-event. Fine for digesting ceremony; revisit if operators want one row per continuous topic.
- Day-row collapse for old R7 bursts (pre-approved lever from the IA table if Phase 2 measurement exceeds the ~10-row target).
- Seat-name tooltips → a roster popover in the top strip.
- `@target` autocomplete in the composer.
