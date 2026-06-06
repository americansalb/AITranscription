# BACKLOG — "Options-or-blocked" human-message gate

**Status:** SHIPPED 2026-06-06 (developer, on feature/strict-turn-discipline). Un-parked by manager directive msg 969 after human made it a hard rule (msg 966). Gate live in `handle_project_send` (vaak-mcp.rs): agent->human sends require `metadata.choices` (2-4) AND `metadata.allow_other==true`, else `[OptionsRequired]`. Human caller exempt; only fires when target is human; `project_buzz` intentionally NOT gated (nudge, not a decision — resolves the open question below). Sender-side gate → activates after sidecar rebuild + each CC window close+reopen.

_Prior status (history):_ PARKED — Manager ruling msg 962 — don't build a sender-side gate off a card pick mid-session (rebuild+restart detour). Behavioral rule (manager msg 944) delivered the intent; this gate is the *enforcement* version.

**Origin:** Human noticed (msg 941) that "decisions" surfaced to them were long prose they merely acknowledged, not actual pickable decisions. Posed as a decision card (ui-architect msg 949); human picked **Option A — hard rule: options-or-blocked** (msg 952), and added (msg 953) that **"other" must always be an option**.

## Spec (if/when built)

- Any message with `to == "human"` MUST carry `metadata.choices` with **2–4 pickable options** AND `metadata.allow_other == true` (mandatory free-text escape — the human must never be trapped by the agent's pre-selected options). Otherwise the send is **blocked** with `[OptionsRequired]`.
- **Pure result/status reports** satisfy the gate by ending in a "what next" options block (e.g. proceed / adjust / stop) — so reporting a result still works; it just closes with the next-step pick.
- **Human seats are exempt** (sovereign). Gate applies to **agent→human** sends only.

## Implementation notes

- Same pattern + same file as the vacant-seat send gate (`f174cfc`, `handle_project_send` in `desktop/src-tauri/src/bin/vaak-mcp.rs`). Small, well-trodden.
- **Activation:** sidecar rebuild (`npm run build-sidecar`) + each Claude Code window close+reopen — sender-side gate; stale sidecars won't enforce (the standing rule for all sender-side gates in this repo).
- Owner split: ui-architect = spec/design + UX gate-review; developer = Rust impl.

## Open design question (resolve before build)

- Should the gate also cover `project_buzz` to the human (the "second door" precedent from the vacant-seat work, msgs 224/2d9a231)? Likely yes for consistency, but buzz is a nudge not a decision — decide at build time.
