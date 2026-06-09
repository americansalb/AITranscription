# ui2/LATER.md — out-of-scope ideas and honest deferrals (decree §12)

Items discovered mid-build. Nothing here is shipped or promised.

## Deferrals (needed before cutover GO, §10 Phase 5)
- **Bundle font files** (Space Grotesk / Inter / JetBrains Mono, SIL OFL). tokens.css declares them with system fallbacks; actual .woff2 bundling not done — current render uses fallback stacks. Offline-clean either way.
- **Playwright smoke** (launch → feed → card → choose → mute) — §7 bar, not yet written.
- **3-platform CI build** — §3.1 acceptance criterion; needs a workflow file + runners.
- **Perf measurement against the real 5k-message board** (§7: initial render <1s, keystroke <16ms, 60fps scroll). Virtuoso + isolated composer slice make the architecture right; the numbers are unmeasured claims until profiled.
- **Coverage number**: 25 tests cover classify/digest/dock/liveness (the pure store logic). @vitest/coverage-v8 not installed, so the ≥80% figure is unquantified.

## Ideas (not authorized, parked)
- Per-discussion keys for *continuous* discussions: today all continuous-review rounds share one "discussion-active" key, so consecutive continuous discussions fold into one row whose verdict is the latest end-event. Fine for digesting ceremony; revisit if operators want one row per continuous topic.
- Day-row collapse for old R7 bursts (pre-approved lever from the IA table if Phase 2 measurement exceeds the ~10-row target).
- Seat-name tooltips → a roster popover in the top strip.
- `@target` autocomplete in the composer.
