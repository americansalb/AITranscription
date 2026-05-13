# CollabTab.tsx UX Audit — for UI Architect Briefing

Author: ux-engineer:0
Date: 2026-05-13
Status: read-only audit; output for UI architect to act on post-restart

## Scope and method

`desktop/src/components/CollabTab.tsx` is the main Collaborate-tab implementation today. 5087 lines, 127 onClick handlers, 41 aria attributes, 34 catch-with-console.error patterns, 5 native `alert()` calls. This audit catalogues UX-lens issues a UI architect would want to know on day one. Architectural review of the monolith is architect-lane (their separate read); this is purely UX surface.

## Top finding — silent-failure pattern is pervasive

The View-button bug the human reported on 2026-05-13 (msg 276) is one instance of a recurring pattern across the file. Catch-blocks call `console.error` and return without surfacing anything to the user. I shipped a toast for `handleViewAgent` in commit `8f2b97a`; **the same fix applies to at least 25 other handlers** in this file.

Audit-flagged silent-failure sites (line numbers and what fails invisibly):

- **1321** create section
- **1338** switch section
- **1542** launch team member (CRITICAL — agent spawn fails, no signal)
- **1582** send interrupt
- **1626** add roster slot
- **1695, 1708** save / delete role group
- **1744** import roles
- **1796** buzz agent (has fallback to board message, so partial)
- **1812, 1842** add / remove roster slot
- **1882** set workflow type
- **1900** set discussion mode
- **1919** toggle Assembly Line (CRITICAL — assembly mode silently fails to enable/disable)

And ~17 more catches following the same pattern. **Every one of these makes the user think the button is broken.** The Toast component (`./Toast.tsx`) is already in scope after my view-button fix added the import — the UI architect should sweep through and replace every `console.error` + return with `showToast(message, "error")` + return. Mechanical, one-line per site.

## Native alert() calls are an anti-pattern

Lines 3303, 3336, 3345, 3349, 3407 — all in the macOS / Node.js install flow. Native `alert()` is modal, blocking, unstyled, and looks like a 1998 web page on a Tauri desktop app. Replace with toasts or inline status messages. None of these should ship to v2.

## Accessibility coverage is partial

127 onClicks, 41 aria attributes — that's ~32% coverage at best, and aria-attribute count includes role= and aria-describedby etc., not all aria-label. Many interactive elements likely have no accessible name. Screen-reader users (who include this project's primary user per CLAUDE.md "Mode: Screen Reader") will hit unnamed buttons frequently. UI architect should run a focused screen-reader sweep on v2 to ensure every interactive element has an accessible name.

## Component-size impact

5087 lines in one file with 80+ useState hooks (per architect's earlier meditation note) carries practical UX cost:

- Cold-load time is longer than necessary
- Hot module reload in development is slow
- Bug surface area is concentrated — a bad render in one section affects all
- The state graph is implicit; you can't tell from outside which states drive what UI

This is the structural problem CollaborateV2 was started to address. The UI architect's v2 work should aggressively de-monolith — small components, each owning a slice of UI state. The brick view from the assembly spec, the rotation header, the phase pill, the moderator panel — each should be its own component with its own file, not folded into a giant CollabTab.

## Specific UX issues observed (not full inventory)

These are spot-checks during the audit, not a complete sweep:

1. **Roster has three view modes** (grid/list/chip) stored in state at line 646 — three different visual treatments for the same data. Either one of them is the right answer (consolidate) or the user is meant to switch contextually (document why). Currently it's likely a feature-accumulation artifact.

2. **Workflow types** at line 69 hardcode three options (Full Review, Quick Feature, Bug Fix) with their own color/desc. If the v2 plan introduces `phase` (per my msg 264 + `phase-pill-spec-2026-05-13.md`), workflow types may collide with or duplicate phase semantics. UI architect should reconcile.

3. **AssemblyBanner removed comment at line 6-9** notes deprecation but ProtocolPanel imported separately at line 10. UI surface for assembly is now split across two components — ProtocolPanel.tsx and the rest of CollabTab. Worth checking that the user mental model still makes sense.

4. **Three locations have similar roster-rendering logic** — line 3097, 3462, plus the chip-mode branches at 3108 and 3482. Duplicated render paths drift over time. Extract a `<RosterCard>` component.

5. **handleBuzz at line 1761** has a sophisticated fallback (terminal buzz → board message if terminal fails) but the user sees the same visual confirmation either way (setBuzzedKey for 1.5s). User can't tell which path succeeded. Surfacing "terminal buzzed" vs "board message sent" would set the right expectation about whether the agent will actually respond.

## What's notable that's GOOD

- ToastProvider is wired (main.tsx line 52), so the toast pattern is available cheaply
- ErrorBoundary wraps the main App, so a render crash in CollabTab doesn't blank-screen the whole window
- Aria attributes exist on the roster view mode buttons (lines 2929-2940), which is the right pattern — UI architect should propagate it
- Persistence via localStorage with size cap (line 592 MAX_DRAFT_LENGTH = 50000) is solid practice — keep this in v2

## Recommended priorities for UI architect

If I were ordering work on this codebase (UI architect will reorder based on their own priorities):

1. **Silent-failure sweep** — toast every catch in CollabTab.tsx. Single-day effort, immediate UX win. Same pattern, ~25 sites.
2. **Native alert() replacement** — five sites, half-day effort. Eliminates the worst visual eyesores.
3. **Decompose the monolith** — start a v2-component-per-feature pattern (rotation header, phase pill, moderator panel, message composer, roster card). Inform every new component pattern by what landed in `CollaborateV2/CollaborateV2App.tsx`.
4. **Accessibility sweep** — every onClick gets an accessible name. Tied to the v2 decomp because per-component review is easier than 5087-line sweep.
5. **Reconcile overlapping mode systems** — workflow types vs phase pill vs assembly state vs discussion modes. The user shouldn't have to mentally diff four overlapping vocabularies.

## Out of scope of this audit

- Visual design / aesthetic choices (colors, spacing, typography) — that's the UI architect's lane to set
- Component decomposition strategy (which slices, in what order) — architect-lane
- Backend changes — none implied here
- New features — pure existing-UI audit

## How to use this document

UI architect post-restart: read alongside architect's read-only-audit (their item 4 in msg 299) and the `CollaborateV2App.tsx` phase plan. The trio gives architectural framing + UX-lens detail + existing-v2-plan as a starting picture. From there, propose a sequence; the team will react.
