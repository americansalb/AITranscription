# Build Plan — Make role differentiation real (unique context, not illusion)

**Date:** 2026-06-08
**Author:** architect:0 (design-of-record consolidation)
**Source:** Delphi discussion 10 FINAL Decision Record (moderator msgs 162 + 168 + 171) + human rulings
(msg 158) + human directive "implement it all" with guardrail (msg 170).
**Status:** PROVISIONAL — see §6. Build order is authoritative; the core bet is not yet blind-validated.

---

## 1. Core (unanimous) + hard anti-pattern

Differentiation must live in each agent's **INFORMATION/INPUTS** (knowledge, state, evidence), NOT in
instructions layered over identical inputs. **ANTI-PATTERN (reject on sight):** shipping longer/sharper
role briefings. "Edit the .md files" = illusion v2. If a proposed change is more persona prose, reject.

## 2. The resolved context architecture — THREE layers (human ruling #1)

My original two-layer split (shared/private) was sharpened by the human into three, because "shared
ground-truth" conflated two things with opposite handling:

- **SHARED FACTS** — code, diff, spec-of-record, raw data, test outputs, state/currency ledgers, and
  any source used for cross-verification. **NEVER fork.** Privatizing these collapses verification (two
  seats reviewing different private copies of the same diff cannot adjudicate findings).
- **GATED CONCLUSIONS** — findings/verdicts/recommendations, **withheld until each agent independently
  commits, then revealed.** The "board is a homogenizer" problem is conclusion-LEAKAGE, not
  fact-sharing. The blind phase is the prototype of conclusion-gating.
- **ROLE-PRIVATE LENS** — threat model, domain/pedagogy corpus, incident log, role-scoped retrieval
  index, accumulating per-seat memory. **Differentiation lives ONLY here.**

## 3. Levers — match to the FAILURE, not just cost (human ruling #2)

- **Information asymmetry** (private context/memory) → fixes "agent doesn't KNOW X." Cheap, bounded, real.
- **Action-boundary** (already shipped) → fixes "builder won't attack own work." Proven on the board,
  same model.
- **Cross-model** (deferred, directive 213) → the ONLY fix for "all agents confidently wrong the SAME
  way." A same-model skeptic shares the blind spot. Un-defer the moment that failure appears.
  "Cheapest-real-first" is fine for ROI, wrong as a reflex.

## 4. Build order (human ruling #3: harness-first; + moderator sequencing catch msg 168/164)

0. **FIX blind-commit enforcement FIRST** (prerequisite). The split discussion.json/protocol state that
   broke discussion 10's blind round must be fixed, else the harness (which measures at commit = a blind
   round) inherits the same leak. See [[project_lock_unify_deferred]].
1. **Ablation harness — 4-ARM design** (dev-challenger msg 173, concretizing evil-arch 166/171). Built
   BEFORE any lever; its job is NOT to bless lever 1 but to answer "can same-model + context EVER clear
   the bar, or is cross-model required NOW?" **No number = no merge.**
   - **Measure AT COMMIT**, before the board homogenizes (post-board every mechanism scores ~0). The
     blind-Delphi submission path is the commit-point capture substrate.
   - Metric = first-order SUBSTANTIVE divergence = 1 − Jaccard(LLM-extracted/normalized finding-sets);
     ignore surface text. One task/diff, all arms run as **developer:0 vs developer:1** (identical
     stance, asymmetric info → any divergence is PURELY information, never persona).
   - **THE 4 ARMS:**
     - **A — OFF:** same model, identical context = the FLOOR (≈0; this session's evil-arch-vs-dc gate
       review already IS this baseline).
     - **B — ON-modest:** same model + a realistic lever-1 `context.md`. Does a normal doc move the needle?
     - **C — ON-MAX:** same model + MAXIMUM plausible asymmetry (full threat model + incident corpus +
       private retrieval). The same-model CEILING.
     - **D — CROSS-MODEL:** different weights, same task, no special context. The decorrelation REFERENCE.
   - **DECISION RULE — TWO SEPARATE QUESTIONS (evil-arch msg 178; corrects dc 173's conflation):**
     - **Q1 "does lever 1 ship?" = the ABSOLUTE two-number gate ONLY** (divergence UP **and** decision-
       quality vs ground truth maintained — see ship gate below). Lever 1 ships iff **C (or B) clears the
       absolute bar.** This is PRIMARY and standalone. Do NOT gate shipping on "C approaches D."
     - **Q2 "is cross-model worth pursuing NOW?" = the C-vs-D DIAGNOSTIC.** Pursue cross-model iff **D
       clears the absolute bar AND exceeds C by a margin.** If **C ≈ A** (max context barely beats the
       floor) AND C fails the absolute bar → ceiling is real, levers 1/2 are deck chairs, **UN-DEFER
       cross-model** (lever 1's highest-value death).
     - WHY separate: "C approaches D" as a ship rule misfires two ways — (i) if D itself is LOW (a
       one-right-answer task), "C approaches D" greenlights TRIVIAL divergence = shipping illusion
       because the reference was low; (ii) if C clears the absolute bar without nearing a higher D,
       lever 1 STILL ships (it cleared the bar) and cross-model would just add MORE, separately.
     - **B** gives the ROI slope (is a cheap doc most of the way to C, or only the expensive corpus?).
   - **ARM D VALIDITY GUARD (architect msg 175 — or the rule misfires):** "C approaches D" only means
     something if D's divergence is DECORRELATION, not DEGRADATION (a weaker 2nd model disagrees because
     it's worse, not because its blind spots differ). So (1) Arm D MUST use a **peer-capability** model;
     (2) measure decision-quality-vs-ground-truth PER ARM (gate-b below) and count D as a valid ceiling
     reference ONLY if D's quality ≥ A's. Compare divergence **at equal-or-better quality**, else you
     compare signal (C) to noise (D).
   - **ARM C VALIDITY GUARD (dc 173):** C's reading is only as good as the "max" context — a weak max
     under-estimates the ceiling and wrongly greenlights cross-model. Arm C must use genuinely strong,
     curated context (loops to the §5 content-curation 80%).
   - **Ship gate (two numbers, tunable):** (a) swap changes ≥30% of a seat's first-order findings
     (overlap < 0.7) AND mean pairwise first-order Jaccard across seats < 0.5; AND (b) decision quality
     vs ground truth does NOT drop. (a) alone is gameable by noise context; (b) kills differentiation-theater.
   - The ceiling verdict must come from an adversarial seat, not the (contaminated) converged consensus.
2. **Lever 1 — per-role context injection.** A THIRD injector in the vaak-mcp.rs briefing-assembly chain
   (~L10263-10282), mirroring `inject_stat_framing`/`inject_judgment_pack`:
   `inject_role_private_context(project_dir, role, briefing)` reads `.vaak/roles/<role>/context/*.md`,
   appends under "## Your Private Context". Additive, **fail-open** (no dir → today's behavior), no schema
   change, ~30 LOC, one rebuild. Validated against the harness.
   - **Pilot:** a LENS-vs-builder pair on the SAME diff (security/platform seat with a private
     threat-model/OS-compat doc vs a builder with none) — deliberately NOT builder-vs-adversary (converges).
   - **Control that isolates INFO from STANCE:** two instances of ONE role (developer:0 vs developer:1),
     private doc given to only one. Same stance, different info → any divergence is PURELY the context.
   - **TOKEN-BUDGET CAP (dc msg 180-2 — gates lever 1, acute at harness Arm C):** the injector appends
     "## Your Private Context" AFTER stat-framing/judgment-pack. A modest doc is fine, but Arm C's
     max-asymmetry corpus (full threat model + incident log) can push the actual TASK out of the window
     or spike cost. REQUIRE a size cap whose fail-safe truncates the CONTEXT, never the briefing/task —
     else the harness's own max-arm bricks the agents it measures.
3. **Lever 2 — per-role accumulating memory** (`.vaak/roles/<role>/memory/`), validated against the harness.
4. **Cross-model** — only on a seat whose ablation shows prompt+context differentiation is insufficient
   (matched to the "confidently-wrong-the-same-way" failure, not cost).
   - **STRUCT ROUND-TRIP GUARD (dc msg 180-3 — gates lever 4):** the per-role `model` field in
     project.json MUST be declared in the Rust `RoleConfig` struct or serde STRIPS it on the read
     round-trip and the wizard silently drops it — this repo's documented struct-strips-undeclared-fields
     bug ([[project_tauri_rust_struct_strips_undeclared_fields]], c9d4825, ~1hr on the currency toggle).
     **UPDATE (developer msg 216 + evil-arch 218 — two halves):** `RoleConfig` ALREADY declares
     `model_provider` + `model_id` (collab.rs:2268-2269). So dc's #3 splits:
     - HALF 1 (Rust read round-trip / serde-strip) — **HANDLED**: the struct declaration IS the
       c9d4825-class fix; the fields won't strip on the project.json read.
     - HALF 2 (UI can SET them) — **STILL OPEN for lever-3**: the wizard FORM must expose
       `model_provider`/`model_id`, else the fields exist but nothing can populate them → per-role model
       is unsettable from the UI. Plus the launcher must actually honor the field at spawn.
     So "won't silently strip" ✓; "wizard can set + launcher honors per-role model" = remaining lever-3 wiring.

## 5. Guardrails

- **Acceptance bar (everything):** shared facts, gated conclusions, ablation-passing, lever matched to
  observed failure. If it's longer .md files, reject.
- **DO NOT BREAK THE ROLE-CREATION WIZARD/PROCESS (human msg 170 — hard guardrail).** Concrete protections:
  - Lever 1 is ADDITIVE and reads a SEPARATE path (`.vaak/roles/<role>/context/*.md`). The wizard writes
    the role briefing to `.vaak/roles/<role>.md` and is generated by `briefingGenerator.ts`. **Leave
    briefingGenerator.ts and the 7-step wizard untouched** (moderator 168 confirms).
  - **THE INVARIANT — per-slug state is ROLE-CRUD-OWNED (architect, generalizing evil-arch 178 + dc 180):**
    the moment lever 1/2 add `.vaak/roles/<slug>/context/` and `/memory/`, those dirs become part of the
    role's IDENTITY, not free-floating files. Role CRUD (collab.rs create/update/delete_role) must manage
    them across the FULL lifecycle — audit EVERY CRUD path for per-slug state, don't just patch the
    enumerated cases:
    - **DELETE** → rm `context/` + `memory/` (else slug-reuse inherits a dead role's private threat-model
      = cross-role data bleed; the precise "fuck up the role process" failure msg 170 warned about).
    - **RENAME / slug-change** → N/A in this codebase (VERIFIED 2026-06-08, developer msg 200 +
      architect): `update_role` (collab.rs L2320) takes no new-slug param and there is no `rename_role`
      — slug is IMMUTABLE. So there is no rename path to strand context/memory; the DELETE-guard is the
      COMPLETE CRUD lifecycle fix. (If a rename op is ever added, this branch reactivates: MOVE the dirs.)
    - **DUPLICATE / EXPORT / IMPORT** (if the wizard exposes them) → carry-or-deliberately-omit these
      dirs, decided per op, never silently.
    - **ACCEPTANCE TEST (lever-1 checklist AND wizard CRUD):** create role → add context+memory → delete
      → recreate SAME slug → assert ZERO stale private state leaks in. Add a rename variant.
  - **CREATE-TIME coexistence (the safe half, still required):** `.vaak/roles/<role>.md` (file,
    wizard-owned) and `.vaak/roles/<role>/context/` (dir, lever-owned) coexist without collision for
    every slug incl. multi-word; missing `context/` dir = fail-open identical-to-today behavior.
  - Step 0 (blind-enforcement fix) touches discussion/protocol state, not the role system — but confirm
    no shared lock path with role CRUD before landing.
- **Content pipeline is the real 80% (moderator 168 cost flag):** the injection plumbing is the easy 20%.
  Curating each `context.md` (who authors/maintains the threat model, incident log, domain corpus) is the
  ongoing cost. Budget the content pipeline or it's illusion with extra steps.

## 5b. Open UX item (NOT data-integrity — routed, unowned)

`delete_role_group` (collab.rs:3019-3043) removes ONLY the group record from `config.role_groups`; it
does NOT delete member roles or touch their `.vaak/roles/<member>/` dirs. **Data-integrity: clean** (no
bleed — verified evil-arch 210 + dc 212; create-side-clear subsumes it). **But the UX is unowned:**
deleting a group silently leaves member roles standalone. Is silent-orphan intended, or should it
orphan-WARN / offer cascade? Touches the human's "don't break the role process" guardrail (msg 170) in a
UX sense only. Owner: role-wizard surface (ux-engineer / ui-architect — both vacant) or human to confirm.
Not urgent, not a bug.

## 6. Provisional status

The R1 4/4 convergence is **contaminated** — blind enforcement didn't engage (2 of 4 positions leaked
pre-submission), so the core bet "same-model + unique context = real differentiation" went effectively
unchallenged. **Re-validate via a genuinely-blind round once step 0 lands** before treating this plan as
confirmed consensus. Until then: build order is authoritative, the conclusion is not.
