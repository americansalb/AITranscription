# Vaaklite v1 — Spec

Owner: architect:0
Date locked: 2026-05-19
Trigger: human msg 5730
Budget: 40 hours autonomous; team executes without human questions per human directive

## Problem statement

Human directive (msg 5730 verbatim):
> "I want you guys to create a very basic standalone webservice that is like a Vaaklite where its not used for coding but discussion document creation etc. UI must be super clean and intutive and simple to use. Have roles, assembly mode, role creation sessions, user accounts for session persistence etc. Dont ask me questions build it to be fully functional and then push to github and merge without asking me anything because i will not be near the computer. You have 40 hours max"

## Architect-lane semantics decision (resolving dev-challenger msg 5731 flag 1)

Vaaklite = **AI-team-driven document drafting**. Agents collaboratively draft a markdown document using the same assembly-rotation pattern Vaak uses for code, but output = document, not codebase. Human picks topic + roles; team drafts; document persists per session.

Other plausible reads (collaborative editor a la Google Docs; threaded discussion with documented outcomes) explicitly rejected — agent-driven drafting is closer to existing Vaak primitives and 40h-feasible.

## Stack decision (resolving flag 2)

ADAPT existing assets. NO greenfield.

- `web-service/` — FastAPI + SQLAlchemy 2.0 + Alembic + LiteLLM. 51 routes, 77+ tests, PostgreSQL via asyncpg. JWT auth, bcrypt, signup/login/refresh/profile/BYOK. Agent runtime exists.
- `web-client/` — React 18 + Vite + Zustand + react-router-dom. 26 source files, 5000 lines, 246KB JS (73KB gzipped). 5 Zustand stores (auth/project/message/discussion/ui). WCAG 2.1 AA. WebSocket auth.

Reuse JWT auth verbatim, agent runtime verbatim, role briefing generator verbatim. Greenfield = not feasible in 40h.

## Assembly model for documents (resolving flag 3)

Section-rotation. The document has N sections; each section is assigned to one role for drafting. Mic = `section_idx`. Phases:
- **drafting** — current role drafts their assigned section
- **review** — other roles review the just-drafted section
- **revision** — current role revises based on review feedback (optional, may skip)
- **final** — section locked; mic rotates to next role+section

Reuses existing AssemblyControls semantics with `section_idx` instead of `current_speaker`.

## "Fully functional" definition (resolving flag 4)

7-item smoke (tester:0 expands edge cases before Hour-13 per msg 5733):
1. User can sign up
2. User can log in
3. User can create a project with `mode: "discussion"`
4. User can configure 2-4 roles from preset (moderator + writer + reviewer + audience)
5. User can start a drafting session on a topic
6. Agents (mock LLM acceptable for v1) take turns drafting sections
7. Final document persists + is downloadable as markdown

"Fully functional" = these 7 items pass end-to-end smoke. NOT 100% coverage. NOT all edge cases. Tester:0's expansion fills the boundary.

## Push target + merge (resolving flag 5)

- Branch: `feature/vaaklite-v1`
- Push to origin
- Open PR
- Auto-merge to main when CI green

NOT direct-to-main without CI. NOT force-push. Human authorized autonomous merge per msg 5730 + [[feedback_human_authority_vs_agent_audit]].

## Sequencing (resolving flag 6)

Two independent pushes:
- **Push #1**: existing `feature/strict-turn-discipline` branch (per human msg 5725 — they want to download current work). Happens FIRST, ~5 min.
- **Push #2**: new `feature/vaaklite-v1` branch when 40h chain completes. Happens at Hour-37+.

Don't conflate.

## 40h budget allocation

| Hours | Work |
|---|---|
| 1-4 | Schema: extend `Project.mode = "discussion" \| "coding"`, Alembic migration, default to existing "coding" for backward-compat |
| 5-12 | Discussion role presets (moderator + writer + reviewer + audience) + role-creation wizard UI (adapt existing briefingGenerator) |
| 13-20 | Document workspace: markdown editor + section schema + server-side persistence per session |
| 21-26 | Section-rotation assembly UI (port AssemblyControls from desktop) |
| 27-32 | User account session persistence + smoke tests |
| 33-36 | Clean-UI polish per ui-arch craft contract |
| 37-40 | Push to `feature/vaaklite-v1` + PR + auto-merge to main |

## Discipline locks for the 40h sprint

- Reuse JWT auth + agent runtime + briefingGenerator verbatim — NO rewrites
- Zustand state only (matches existing web-client convention)
- Pure CSS, no Tailwind (matches existing web-client + desktop conventions)
- Smoke + critical-path tests only; NOT 100% coverage chase
- Gate-review verbosity proportional per F-DC-KRL2 — small commits get 1-paragraph verdicts
- Zero human questions during sprint per human directive — autonomous mode authorized
- If hard blocker hits that genuinely requires human input, ONE structured `to:"human"` question lands in decision panel; otherwise silence + delivery

## Lane assignments

- **developer:1** — execute build chain. Hour-1 starts after msg 5725 push completes.
- **tester:0** — expand the 7-item acceptance criteria with edge cases per msg 5733. Lock as gate-#1 contract per ship.
- **ui-architect:1** — write UI craft contract (typography, spacing system, hierarchy, color tokens) before Hour-13. "Super clean intuitive simple" is the explicit human craft framing.
- **evil-architect:0** — pre-load gate-#2 class-of-bug audit slate per upcoming ship.
- **dev-challenger:0** — original 6-flag review at msg 5731 resolved by this spec; concur or push back fast.
- **architect:0** — vision.md update after Vaaklite v1 ratifies; cross-session continuity.

## Adversarial pre-defenses (per dev-challenger msg 5731 add)

- Don't reinvent auth — copy JWT path verbatim
- Don't reinvent role briefing — adapt briefingGenerator.ts
- Don't reinvent discussion modes — Delphi/Continuous can be lightly tweaked
- Don't write tests for everything — minimum smoke + critical-path
- Pick one CSS framework (pure-CSS) and one state library (Zustand) upfront — don't switch mid-build
- Burn-rate: lock fast, lock right; pre-coding ambiguity = mid-build churn

## Cross-references

- Existing web-service at `web-service/` per `MEMORY.md` §Web Service (Built Feb 24, 2026)
- Existing web-client at `web-client/` per `MEMORY.md` §Web Client SPA (Built Feb 24, 2026)
- Path B `persistedState.ts` SHA `2fe16e8` — pattern for any web-client localStorage usage
- Decision-panel-v1.2 SHA `1e2f0be` — pattern for human-prompt surfaces if needed mid-build
- Keepalive series — pattern for liveness if real-time presence needed
- briefingGenerator.ts — single source of truth for role briefings; adapt for discussion roles

## Spec status

LOCKED 2026-05-19. Team executes autonomously. Architect:0 standing by for arbitration if disputes arise mid-chain; otherwise silent until ratification.
