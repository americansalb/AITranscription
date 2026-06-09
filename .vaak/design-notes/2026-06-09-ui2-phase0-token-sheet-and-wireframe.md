# UI2 Phase 0 — Token Sheet + Annotated Wireframe

**Owner:** ui-architect:0 · **Date:** 2026-06-09 · **Governing doc:** "One Window" decree (board msg 210), §4, §5, §7
**Status:** DELIVERABLE — awaiting Phase 0 gate (human approval via one decision card, routed by relay)
**Scope:** design only. No code exists. `desktop/src/ui2/` is claimed but empty.

---

## 1. Design direction (one sentence)

An instrument, not a feed: dark-first operator console where the only *persistently* saturated color on screen belongs to decisions awaiting the human. (`--status-warn` is also saturated but appears only when something is genuinely wrong — an instrument's warning lamp, dark by default.)

---

## 2. Color tokens — exactly 6 named colors (§5.2)

Every component derives from these. Zero hex literals in components. Variants (muted/raised/hover) are derived via fixed opacity steps on these six, never new hexes.

| Token | Role | Dark (default) | Light |
|---|---|---|---|
| `--ink` | All text. Muted text = `--ink` @ 62% opacity | `#E8EAED` | `#1A1C1E` |
| `--surface` | Window background | `#141619` | `#FAFAF8` |
| `--surface-2` | Raised: cards, dock, drawer | `#1D2024` | `#FFFFFF` |
| `--line` | Borders, dividers, collapsed-row rules | `#2A2E33` | `#E3E1DC` |
| `--accent` | **Decisions ONLY.** Dock border, active card, option buttons, decision rows in feed. Appears nowhere else — not links, not focus, not branding | `#E0A93E` (amber) | `#8A6516` |
| `--status-warn` | Genuine failures only: dead seat, failed build, error rows. Healthy liveness is expressed with `--ink` dot shapes (see §6 annotation 2), NOT a green — no permanently-lit saturated status color exists | `#D96A4B` | `#B04A2E` |

(Revision per evil-architect adversarial review, msg 222: an earlier draft carried a 7th color, `--status-ok` green. Removed — it both broke the 4–6 budget and contradicted the §1 saturation rule. Healthy states are now shape-encoded in ink.)

Rationale for amber accent: it reads "attention required" without the alarm semantics of red (red stays reserved for `--status-warn` genuine errors). Contrast checked (sRGB blend, WCAG relative luminance): `--accent` on `--surface-2` dark = 7.9:1; `--ink` on `--surface` dark = 13.2:1; muted ink 62% ≈ 6.3:1 (corrected from an earlier wrong 7.4:1 — recomputed per msg 222). All ≥ WCAG AA for text sizes used; cd-accessibility seat can re-verify at Phase 2.

Focus ring: `--ink` 2px outline + 2px offset (never `--accent` — focus must be visible on a decision card whose border is already amber).

## 3. Type scale (§5.2)

Three faces, all bundled in the app binary (no network fetch — desktop must work offline from clean clone):

- **Heading/verdict face:** Space Grotesk (SIL OFL) — used ONLY for card titles, verdict lines, and the top-strip project name. Restraint enforced by token: only `--text-lg` and `--text-xl` may use it.
- **Body face:** Inter (SIL OFL) with `system-ui` fallback — everything else.
- **Mono face:** JetBrains Mono (SIL OFL) with `ui-monospace` fallback — IDs, hashes, raw JSONL, Engine Room metadata. Never in the Signal Feed (§5.4: jargon is Engine-Room-only, and so is its typeface).

| Token | Size/line | Face | Use |
|---|---|---|---|
| `--text-xs` | 12/16 | body | digest-row metadata, timestamps |
| `--text-sm` | 13/18 | body | collapsed digest rows, Engine Room rows |
| `--text-base` | 14/21 | body | feed messages, card option labels |
| `--text-lg` | 17/22 | heading | card titles, discussion digest headers |
| `--text-xl` | 22/28 | heading | verdict lines, gate banners (rare) |

Weights: 400/500/600 only. No 700+ — density stays calm.

## 4. Spacing + radii (§5.2)

- **Spacing scale (4px base):** `--s1` 4 · `--s2` 8 · `--s3` 12 · `--s4` 16 · `--s5` 24 · `--s6` 32 · `--s7` 48. Feed row vertical rhythm = `--s3`; card internal padding = `--s4`; pane gutters = `--s5`.
- **Radii:** `--r-ctl` 4px (buttons, inputs, filter chips) · `--r-card` 8px (cards, digest rows) · `--r-panel` 12px (dock, Engine Room drawer) · `--r-full` 999px (liveness dots, mute pill).
- **Elevation:** no drop shadows in dark theme — raised surfaces are expressed by `--surface-2` + `--line` border. Light theme gets one 1-step shadow token for cards only.
- **Motion:** exactly three animations exist (§5.3): card arrival (180ms ease-out translate+fade), mute-state change (120ms cross-fade on top strip), collapse/expand (150ms height). All gated behind `prefers-reduced-motion`.

## 5. State store pick (§3.4 — required one-paragraph justification)

**Zustand.** One store, one direction: Tauri events/disk reads → store actions → selectors → components. Justification: it is already the team's proven pattern (web-client SPA, 5 stores, shipped and audited), has no provider-tree ceremony so the single-window shell stays flat, its selector model gives us the per-slice subscription granularity that the §7 keystroke→paint <16ms bar demands (composer state lives in its own slice; timeline components subscribe only to the message slice — the CollabTab per-keystroke full-timeline re-render bug is structurally impossible), and it is trivially testable without rendering (≥80% store coverage bar, §7). Redux Toolkit adds ceremony with no offsetting benefit at this scale; React context alone re-renders too coarsely — it is the bug class we're escaping.

## 6. Annotated wireframe of §4

```
┌────────────────────────────────────────────────────────────────────┐
│ TOP STRIP  h:48px, --surface-2, border-bottom --line               │
│ [Vaak · AITranscription]   [●●●●○●●●]              [ ⏸ MUTE ALL ]  │
│  --text-lg heading face     liveness dots            pill, --r-full │
│                             8px --r-full, four states (see          │
│                             annotation 2): working=filled ink ·     │
│                             warm-zombie=hollow ink@62% ·            │
│                             dead=--status-warn · vacant=hollow      │
│                             ink@30%. DERIVED from the session       │
│                             records agents write (§3.5 — no         │
│                             parallel tracker). Hover = seat name    │
│                             + last_working_at age. Mute active:     │
│                             pill fills --ink, caption "room muted". │
├───────────────────────────────────────────────┬────────────────────┤
│ SIGNAL FEED  (default + only default view)    │ DECISION DOCK      │
│ scrolls; virtualized (windowed) from day one  │ w:320px fixed,     │
│                                               │ --surface-2,       │
│ ① relay message — expanded, full body         │ --r-panel, border  │
│    --text-base, author chip "relay"           │ --accent when a    │
│                                               │ card is active;    │
│ ② ▸ ⚙ 14 engine events · expand               │ --line when empty  │
│    collapsed digest row: --text-sm, muted     │                    │
│    ink, --r-card, 1-click expand, per-row,    │ [ACTIVE CARD]      │
│    NEVER sticky — resets collapsed on launch  │  title --text-lg   │
│                                               │  options = full-   │
│ ③ ▣ DECISION #126 …  (inline mirror of dock   │  width buttons,    │
│    card, --accent left rule 3px)              │  label = action +  │
│                                               │  one-line          │
│ ④ ▸ 🗩 Delphi #12 · closed · verdict: KEEP     │  consequence       │
│    kernel, freeze mechanism · 8/8 · expand    │  ("card #125       │
│    digest card (§4.5): ONE row per            │  pattern is        │
│    discussion, lifecycle updates IN PLACE     │  canon"), always   │
│    (no new row per phase/round). Correlation  │  incl. "other".    │
│    caveat rendered in the tally line:         │  --accent border,  │
│    "8 seats · 1 model — weight accordingly"   │  --r-card          │
│    (msg 90 commitment, §4.4)                  │                    │
│                                               │ [QUEUED CARDS]     │
│ Feed renders expanded ONLY: to-human msgs,    │  greyed (38%       │
│ relay posts, decision cards, final verdict    │  opacity), each    │
│ digests. Everything else → collapsed digest   │  shows "blocked    │
│ rows. (Exact inclusion rules = ux-engineer's  │  by #126" — the    │
│ IA decision table, the other half of this     │  msg-104/122       │
│ gate.)                                        │  silent-block      │
│                                               │  failure made      │
│                                               │  VISIBLE (§4.2)    │
│                                               │                    │
│                                               │ [RESOLVED] last 3: │
│                                               │  "✓ #125 — chose B │
│                                               │  · 21:40" --text-xs│
├───────────────────────────────────────────────┴────────────────────┤
│ COMPOSER  h:auto, min 44px. One input, @target autocomplete,       │
│ [Send]. Composer state = isolated store slice: typing re-renders   │
│ NOTHING outside this bar (§7: keystroke→paint <16ms)               │
├────────────────────────────────────────────────────────────────────┤
│ ▸ ENGINE ROOM  drawer, closed by default, opens to 55vh overlay    │
│ Full unabridged board. Filter chips: [seat ▾][type ▾][discussion ▾]│
│ + [raw JSONL] toggle (mono face). Mono timestamps. Muted traffic   │
│ lands here untouched — audit-complete, attention-empty (§4.3/4.4)  │
└────────────────────────────────────────────────────────────────────┘
```

Annotations — behavioral commitments the wireframe encodes:

1. **Accent discipline:** amber appears in exactly three places above — dock border (when active), active/queued card chrome, inline decision rows (③). Nothing else. If a future PR uses `--accent` outside the decision system, that is a review-blockable token violation.
2. **Liveness is derived (§3.5) — and cognition ≠ connection (revised per evil-architect msg 222, HIGH):** an earlier draft derived the dot from `last_alive_at_ms` alone; that is the recorded warm-zombie bug (2026-06-04: keepalive keeps the heartbeat fresh on a cognitively dead seat — a green dot on a zombie lies). The dot encodes two distinct facts, each read from its own single source in the session records agents already write:
   - **working** (`last_working_at` fresh) → filled `--ink` dot
   - **warm zombie** (`last_alive_at_ms` fresh, `last_working_at` stale) → hollow `--ink` @62% — present-but-not-working is *visibly distinct*, never rendered as healthy
   - **dead** (`last_alive_at_ms` stale) → `--status-warn` filled
   - **vacant** (no binding) → hollow `--ink` @30% — structural, not an error
   No UI-side heartbeat tracker; no parallel state. One source per fact. Freshness thresholds to be fixed at Phase 2 and stated on that gate card.
3. **Collapse is the default state on every launch (§4.1):** expansion is per-row, in-memory only, never persisted. localStorage is not a source of truth anywhere on this surface.
4. **One discussion = one living row (§4.5):** Delphi #12 must render as row ④ exactly — card, verdict line, expand. Phase-machine chrome, speaker timers, per-turn system rows have no UI.
5. **Mute (§4.3) is experience-first:** the feed filter flips locally and immediately; the standing board directive is posted as a side effect. Agent non-compliance cannot break the human's silence — non-compliant posts land in the Engine Room only.
6. **Keyboard + screen reader (§5.3, launch requirement):** tab order = strip → feed → dock → composer → drawer. Feed is `role="feed"` with `aria-label` per row type ("collapsed digest, 14 engine events, press Enter to expand"); dock is `role="region" aria-live="polite"` so a new card is announced; mute is `aria-pressed`. Full SR pass is a Phase 3 gate item with cd-accessibility if seated.

## 7. Budget posture (§7)

This token system is deliberately small so the budgets hold: 6 colors + derived steps ≈ 40 lines of CSS custom properties; tokens file + per-component co-located styles keeps every sheet far under the 500-line cap. Estimated shell cost: tokens ~60 lines, layout grid ~120 lines. Headroom against the 6,000-line total is the point.

## 8. Open items for the Phase 0 card (not blockers to reviewing this sheet)

- ~~ux-engineer seat is VACANT / IA table has no owner~~ **RESOLVED 2026-06-09:** ui-architect:0 absorbed the IA decision table after the human removed the ux-engineer seat (msg 225; team review 3–0 agree, msg 239). Delivered as the companion doc: `2026-06-09-ui2-phase0-ia-decision-table.md`. Both Phase 0 halves now exist.
- Light-theme `--status-warn` shade needs a contrast re-check once real surfaces exist (flagged for cd-accessibility, seat currently vacant).
