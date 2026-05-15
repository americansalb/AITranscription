# AALB Spanish Proficiency Test — Implementation Plan

Status: planning-phase skeleton; awaiting team round-1 convergence completion and human accept_plan to flip to execution
Plan author (architect): architect:0
Plan target file: `.vaak/design-notes/aalb-spanish-proficiency-test-plan-2026-05-15.md`
Convergence round 1: evil-arch msg 2100 outline + architect msg 2103 + developer:1 msg 2105 + dev-challenger msg 2110 + UI-architect msg 2113 + platform-engineer msg 2115/2118 + architect msg 2121

## Honest-framing caveat for AALB

Active team has no language-testing SME, no psychometrics SME, no Spanish-language SME, no compliance/accreditation SME. This plan produces a STRUCTURALLY complete artifact — framework, methodology, infrastructure design, and integration scope. The content of each section will be honest about what the team can deliver vs what requires SME pass:

- **Engineerable now**: delivery platform, secure browser, identity-verification infrastructure, accessibility surface, ASR pipeline architecture, statistical pipeline architecture, rater-pool sizing methodology, scale-construction methodology, validity/reliability framework.
- **SME-deferred**: actual descriptor calibration at each ILR/ACTFL/CEFR anchor, individual item-content authoring, Spanish-dialect-specific rubric tuning, language-testing-industry compliance validation, accreditation-grade psychometric validation.

The team's deliverable is the FRAMEWORK; AALB needs an external SME pass before this becomes a publishable plan.

## Scope block

<!-- scope: .vaak/design-notes/aalb-spanish-proficiency-test-plan-2026-05-15.md -->

## Sections (8 top-level)

### Section I — Test Design & ILR/AALB Scale Construction

<!-- delegation: owner=architect:0 section=I deadline=execution-phase deps= -->

Test design scope: construct definition + scale anchoring + item-development methodology + scoring framework. Cross-cuts II (item bank), V (psychometrics).

<!-- delegation: owner=architect:0 section=I.0 deadline=execution-phase deps= -->

**I.0 — Audience validation (per dev-challenger msg 2110 #1).** Before scale topology lock, validate AALB's primary audience: federal/industry (ILR primary), civilian/education (ACTFL primary, CEFR for international), or hybrid. Stakeholder-interview methodology to be specified. If hybrid validates, I.B/I.C topology becomes multi-axis crosswalk (ILR + ACTFL + CEFR) per UI-architect msg 2113 component-scope flag. ILR-only path stays the assumption pending validation.

<!-- delegation: owner=architect:0 section=I.A deadline=execution-phase deps=I.0 -->

**I.A — Construct definition.** What "Spanish proficiency" means in AALB's context. Includes Spanish-dialect scope (Castilian / neutral Latin American / country-specific per dev-challenger msg 2110 closing — affects items, raters, rubrics, validity).

<!-- delegation: owner=architect:0 section=I.B deadline=execution-phase deps=I.A -->

**I.B — ILR scale anchoring.** ILR's 11 cardinal anchors (0, 0+, 1, 1+, 2, 2+, 3, 3+, 4, 4+, 5) as the federal-axis foundation. Includes explicit rater-pool sizing + training cost deliverable (per dev-challenger msg 2110 #2 — ~40hr training/rater, ~100+ raters for k>0.7 inter-rater reliability, ongoing recalibration). Plus-levels (0+/1+/2+/3+/4+) require highly-trained raters and increase calibration cost; sizing math goes here.

<!-- delegation: owner=architect:0 section=I.C deadline=execution-phase deps=I.B -->

**I.C — AALB-equivalent scale construction.** 1-to-1 isomorphism of ILR cardinal anchors (assuming federal-primary audience per I.0 validation) with AALB-tailored descriptors. If multi-axis validates, this becomes ILR↔AALB↔ACTFL↔CEFR concordance methodology + bidirectional crosswalk study design. AALB's brand value is in the descriptors and certification framing, not in inventing a new level structure.

<!-- delegation: owner=architect:0 section=I.D deadline=execution-phase deps=I.A,I.C -->

**I.D — Item-development methodology.** Item-writing framework per modality. Actual items are II's scope; I.D is the design framework (blueprint specification, item-difficulty target distribution, IRT-anchoring approach for new items, content-validity review process).

<!-- delegation: owner=architect:0 section=I.E deadline=execution-phase deps=I.C,I.D -->

**I.E — Scoring rubric.** Rubric specification per modality + per ILR anchor. SME-deferred on actual descriptor content; framework is engineerable.

### Section II — Test Items per Modality

<!-- delegation: owner=architect:0 section=II deadline=execution-phase deps=I.D,I.E -->
<!-- delegation: owner=ux-engineer:0 section=II.candidate-perspective deadline=execution-phase deps=II,IV.D -->

Item-bank specification for listening, reading, speaking, writing modalities. SME-content-deferred on actual items; this section delivers the item-design framework + sample-item sketches per modality + item-difficulty calibration methodology.

<!-- delegation: owner=architect:0 section=II.A deadline=execution-phase deps=I.D -->

**II.A — Listening items.** Audio passage + comprehension question structure; ILR anchor mapping; replay-count policy per ILR anchor (composes with UI-architect msg 2113 IV.B open question — replay-count policy is rubric-derived, not per-item).

<!-- delegation: owner=architect:0 section=II.B deadline=execution-phase deps=I.D -->

**II.B — Reading items.** Passage + question structure; passage-length scaling with ILR anchor; question types (literal/inferential/critical) by anchor.

<!-- delegation: owner=architect:0 section=II.C deadline=execution-phase deps=I.D -->

**II.C — Speaking items.** Prompt structure (read-aloud, picture-description, situational role-play, structured-conversation, sustained-discourse — ILR anchor-mapped). Recording-time budget per item type. Paralinguistic-feature dependency on codec (per dev-challenger msg 2110 #5 + platform-engineer msg 2115 codec recommendation — Opus 48kHz fullband preserves paralinguistics for ILR 3+/4/5).

<!-- delegation: owner=architect:0 section=II.D deadline=execution-phase deps=I.D -->

**II.D — Writing items.** Prompt structure (sentence-completion, short-response, structured-essay, sustained-discourse). Word-count expectation per ILR anchor. Mechanics vs content rubric weighting.

### Section III — Delivery Platform & Proctoring Infrastructure

<!-- delegation: owner=developer:1 section=III deadline=execution-phase deps=I.A -->

Co-owned developer:1 + platform-engineer:0 per platform-engineer msg 2115. III.B option arbitrated to PHASED ROLLOUT per architect msg 2121: Phase 1 Tauri-native (sponsored AALB admin), Phase 2 lockdown-browser (self-assessment), Phase 3 hybrid (mobile in-scope).

<!-- delegation: owner=developer:1 section=III.A deadline=execution-phase deps=I.A,III -->

**III.A — Recording & capture pipeline.** Per-modality capture (audio for speaking, video for proctoring + identity, text for writing). MediaRecorder API or Tauri-native equivalent. Codec: Opus 48kHz fullband for speaking (paralinguistics preserved per dev-challenger msg 2110 #5 + platform-engineer msg 2115); H.264 baseline for video. Chunked streaming + local IndexedDB fallback for network-blip resilience. S3-compatible signed-URL writes. 7-year retention pending VII Compliance.

**III.A execution-phase content (developer:1):**

- **Audio capture parameters (speaking modality):** Opus codec @ 48 kHz / 16-bit mono / VBR ~24 kbps target + 32 kbps cap; `MediaRecorder` `audioBitsPerSecond: 24000` with `mimeType: "audio/webm;codecs=opus"`; Tauri-native fallback uses `cpal` library with same params. Chunk duration: 5 s slices via `MediaRecorder.start(5000)` so a 30-min speaking task = ~360 chunks; each chunk ~15 KB. Total payload ~5.5 MB per 30-min speaking task — well within IndexedDB headroom even on Safari's 500 MB floor.
- **Video capture parameters (proctoring + identity):** H.264 baseline profile @ 640×480 / 15 fps / VBR ~200 kbps; `MediaRecorder` `videoBitsPerSecond: 200000` with `mimeType: "video/mp4;codecs=avc1.42E01E"`. Chunk duration 30 s (proctoring tolerates coarser granularity than speaking). At 200 kbps × 30 min = ~45 MB total — still IndexedDB-feasible; degrades to 320×240 / 10 fps at <500 kbps available bandwidth.
- **Text capture (writing modality):** plain UTF-8 text; debounced state-snapshot every 10 s; `crypto.randomUUID()` per chunk for de-duplication. Local-storage footprint negligible (~20 KB per 1000-word essay).
- **Chunked-upload protocol:** `PUT` to S3-signed URL per chunk; server confirms via 200 response → client deletes IndexedDB entry. On 5xx/timeout: exponential backoff 2^n s capped at 60 s; persist locally. Service Worker handles retry in background tab.
- **Resumption tokens:** each chunk carries `{session_id, modality, chunk_seq, sha256}`. Server-side dedup by `(session_id, modality, chunk_seq)`; sha256 catches corruption. Recovery from browser crash: rehydrate `session_id` from JWT in localStorage; re-list IndexedDB chunks; resume PUT loop from `max(chunk_seq) + 1`.
- **Retention + deletion:** S3 lifecycle policy moves chunks to Glacier after 90 d; deletes after 7 years per language-testing-industry norms (pending VII final). Per-candidate "right to deletion" (GDPR/CCPA) cross-cut to VII.A handled by tagged S3 object → batch-delete Lambda.

<!-- delegation: owner=developer:1 section=III.B deadline=execution-phase deps=III.A -->

**III.B — Secure browser / lockdown environment (PHASED).** Phase 1: Tauri-native (kiosk-mode, no copy/paste, no other windows, no screen recording) for sponsored AALB admin. Phase 2: lockdown-browser (Chrome enterprise policies + tab-isolation) for self-assessment. Phase 3: hybrid (native shell loading sandboxed web content) when mobile in-scope. Honest threat-model acknowledgments per dev-challenger msg 2110 #4: gaze tracking and keyboard cadence have high FP rates; live human proctor is the primary control for accreditation-grade testing; VM-detection (CPUID hypervisor bit, IOMMU signatures, clock-jitter) is Phase 1 limitation explicitly documented; off-screen-accomplice via secondary device is uncatchable without live proctor.

**III.B execution-phase content (developer:1):**

- **Phase 1 — Tauri-native AALB Test Client:** new Tauri 2.0 app, separate from Vaak's existing Tauri app. Single-window full-screen kiosk via `tauri::WindowBuilder::new(...).fullscreen(true).resizable(false).decorations(false)`. Block clipboard via `tauri::Manager::set_clipboard_text` disabled. Block dev-tools via `--release` flag + `tauri.conf.json` `"devPath": null` + `"build": { "devUrl": null }`. Block second-window-open via `tauri::Manager::create_window` guard. macOS: NSApplication.shared.presentationOptions = .disableAppleMenu | .disableProcessSwitching. Windows: `SetThreadDesktop` to isolated desktop (admin-only — defer to v1.1). Linux: X11 grab via `XGrabKeyboard` (Wayland equivalent TBD per platform-engineer:0 cross-platform audit).
- **Phase 1 — VM/Hypervisor detection** (best-effort, documented limitation): Windows `IsProcessorFeaturePresent(PF_HYPERVISOR_PRESENT)` + CPUID leaf 0x40000000 (hypervisor vendor signature); macOS `sysctl hw.vmwaretools` / `system_profiler SPHardwareDataType` for VMware/VBox tells; Linux `/sys/class/dmi/id/sys_vendor` for QEMU/KVM/VirtualBox; cross-OS clock-jitter test (consumer-grade hypervisors leak ~10-50µs timing irregularities). At-launch detection only — if detected, candidate sees "VM detected — testing on a VM is not permitted for accredited admin. Switch to a physical machine or contact your proctor." Bypassable but raises the bar from zero-effort.
- **Phase 1 anti-cheat enumeration:** clipboard disable, copy/paste/cut intercept, right-click-menu disable, F12/Ctrl+Shift+I disable, print-screen intercept (Windows `RegisterHotKey VK_SNAPSHOT` no-op handler; macOS not interceptable per API constraints — known limitation), audio-out monitoring (detect if speakers/headphones change mid-test). Each enumerated under `tauri::AppHandle` lifecycle.
- **Phase 2 — Lockdown-browser fallback:** Respondus LockDown Browser (commercial, ~$5K/yr institutional license) OR open-source Safe Exam Browser (SEB, freemium, manifest-based config). Recommend SEB for self-assessment use case — institutional Respondus license cost is unjustified at self-assessment scale.
- **Phase 3 — Hybrid + mobile:** placeholder; full design deferred. Constraint: iOS/Android in-app webview lacks Chrome-equivalent enterprise policies. Workaround paths under investigation include Apple's `WKWebView` config + Android's `WebViewClient` config + native shell that wraps test surface.

<!-- delegation: owner=platform-engineer:0 section=III.C deadline=execution-phase deps=III.A,VII.A -->

**III.C — Identity verification.** Photo ID OCR + face-match. Live selfie at session start matched against pre-registered photo OR government ID. Random re-verification mid-test if proctor flags suspicion. Fall back to manual review when automated confidence < 0.95. Cross-platform: WebRTC getUserMedia uniform on desktop; iOS Safari needs native shell. OS camera-permission flows asymmetric (macOS TCC prompt, Windows silent, Linux varies) → OS-conditional error copy.

<!-- delegation: owner=platform-engineer:0 section=III.C.1 deadline=execution-phase deps=III.C,VII -->

**III.C.1 — Self-hosted vs vendor trade-off + privacy-impact analysis (per dev-challenger msg 2110 #3 + platform-engineer msg 2115).** Self-hosted (face_recognition + dlib or InsightFace, ~$200-500/mo GPU) vs commercial vendor (Veriff/Persona/AWS Rekognition/Azure Face, ~$50/test, break-even ~10K tests/mo). Privacy: biometric data shipped to third-party conflicts with GDPR/state laws (CCPA, BIPA, etc.); explicit dep on VII Compliance for jurisdictional scope.

<!-- delegation: owner=developer:1 section=III.D deadline=execution-phase deps=III.A,III.C,IV.C -->

**III.D — Real-time proctoring pipeline.** WebRTC peer connection to proctor monitor (or SFU like mediasoup for multi-candidate batching). Behavioral signals: gaze tracking (face landmarks), audio voice continuity, keyboard cadence, screen-region focus. Honest threat-model labeling per dev-challenger msg 2110 #4: what's caught vs not. Post-hoc review compiles candidate session to mp4 + event timeline; proctor scrubs for anomalies. Critical cross-cut to IV.C: anti-cheat engine reads applied-accommodation profile and suppresses corresponding signals (screen-reader use ≠ off-screen-look; extended-time ≠ extended-idle = cheat).

**III.D execution-phase content (developer:1):**

- **Behavioral signal threshold table (auto-flag-for-review, NOT auto-flunk):**
  - *Face absence:* threshold 5 s continuous → flag-for-review. Accommodation suppression: candidate w/ "extended-bathroom-break" accommodation gets 120 s pre-flag grace.
  - *Multiple faces:* any detection → immediate flag-for-review (no false-positive grace; legitimate single-occupant testing always has one face).
  - *Off-screen gaze:* >30 % of 60 s window looking away from screen → flag-for-review. Accommodation suppression: candidate w/ "scratch paper" accommodation gets gaze-FP-rate raised to >60 %.
  - *Audio voice continuity (speaking modality only):* secondary voice detected → flag-for-review. Threshold: secondary-voice energy >-30 dB relative to primary candidate voice for >2 s.
  - *Keyboard cadence (writing modality only):* dwell-time variance < 10 ms (suggesting auto-paste/macro) → flag-for-review. Accommodation suppression: "alternative input device" accommodation disables cadence checking entirely.
  - *Window/tab focus loss:* any focus-loss event in Tauri-kiosk mode → flag-for-review (kiosk mode shouldn't permit it; if observed, likely OS-level bypass attempt).
- **WebRTC topology:** mediasoup SFU @ ~10 candidates per proctor max. Per-candidate video: 320×240 @ 5 fps (proctor view) — distinct from candidate-side capture at 640×480 @ 15 fps. Bandwidth budget: proctor monitor ~250 kbps per candidate × 10 = ~2.5 Mbps total.
- **Auto-flag UI contract (cross-cut to IV.A):** every flag emits a board-like event `{candidate_id, signal_type, timestamp, confidence, suppression_applied: bool}` consumed by proctor UI. Proctor UI shows flag with confidence input slider (0-100) before escalation per UI-architect msg 2113 #4 craft note — prevents over-flagging legitimate accommodation behaviors.
- **Post-hoc review artifact:** WebM screen-capture + chunked audio (from III.A) + chunked video (from III.A) + JSON event timeline merged server-side into single MP4 + sidecar JSON via FFmpeg `concat` + `subtitles` filter. Retention per III.A: 7 years; deletable per VII.A.
- **Accommodation × proctoring conflict resolution (cross-cut to IV.C):** at session start, anti-cheat engine reads `candidate.accommodations` → per-signal suppression map → applied to all real-time + post-hoc analysis. Suppression decisions audit-logged so accreditation review can verify no candidate was unjustly flagged.

<!-- delegation: owner=platform-engineer:0 section=III.E deadline=execution-phase deps=III.A,III.D -->

**III.E — Resilience & network handling.** Offline-tolerant capture (IndexedDB buffer, post-recovery upload — accounting for ~5-10% disk on desktop / 500MB-1GB on Safari with rolling-clear of confirmed chunks). Bandwidth degradation: drop video resolution before audio (audio IS the speaking construct; video is proctoring). Browser crash: auto-resume with server-side session token. Active 10s heartbeat probe to known endpoint (navigator.onLine returns true on captive-portal). Service Worker upload retry for background-tab throttling. 3-way submission handshake (client commits, server acks, client purges local buffer).

### Section IV — UX + UI Surface

<!-- delegation: owner=ui-architect:0 section=IV deadline=execution-phase deps=I.A,III -->

UX flow ownership = ux-engineer:0 (vacant — flagged per UI-architect msg 2113 aggregate-banner pattern). UI surface implementation = ui-architect:0. Accessibility cross-cut = cd-accessibility:0 (vacant — flagged).

<!-- delegation: owner=ui-architect:0 section=IV.A deadline=execution-phase deps=III.D -->

**IV.A — Proctoring UI surface.** Live test-taker view (webcam indicator, mic state, screen-share affordance, identity-check overlay). Examiner monitor (multi-candidate grid, flag-for-review). Flag-for-review affordance: FP-rate context per signal type + proctor-confidence input BEFORE flag escalation, NOT one-click flagging (per UI-architect msg 2113 #4 endorse of dev-challenger msg 2110 #4).

<!-- delegation: owner=ui-architect:0 section=IV.B deadline=execution-phase deps=I.A,I.B,II -->

**IV.B — Test-item component library.** Four ILR-aligned modality components: listening (audio player with rubric-aware replay-count policy), reading (passage + question pane with text-anchor highlighting), speaking (recording UI with countdown + re-record bounded by rubric), writing (text editor with character/word counter + ILR-band hint). Each renders identically across ILR anchors. Codec-disclosure label per UI-architect msg 2113 #5: "Narrowband audio — paralinguistic features attenuated" if Opus 48kHz fullband unavailable.

<!-- delegation: owner=ui-architect:0 section=IV.C deadline=execution-phase deps=IV.A,IV.B,IV.D,IV.E -->

**IV.C — Accessibility surface (WCAG 2.1 AA).** Keyboard nav, screen-reader compat, high-contrast mode, font-size controls (200% zoom without horizontal scroll per WCAG 1.4.10), captions for listening items with rubric-aware visibility (captions OFF for listening-comprehension items unless the testee qualifies for accommodation). Time-extension reads documented-accommodations field, not self-serve. Pair with cd-accessibility:0 once seated.

<!-- delegation: owner=ux-engineer:0 section=IV.D deadline=execution-phase deps=VII.A,IV.A,IV.B -->

**IV.D — Test-taker experience + accommodations.** Pre-test onboarding (sound/mic/camera/browser check, ID-verification flow, environment scan, consent + privacy disclosure, accommodations request capture before launch). In-test affordances (timer placement, save-state indicator, report-a-problem path). Post-test debrief (immediate submission confirmation + receipt, results-ready notification channel). Owner ux-engineer:0 is currently vacant — aggregate vacant-banner per UI-architect msg 1925 craft note 2 will surface in Affordance B chart at execution-phase commit time.

<!-- delegation: owner=ui-architect:0 section=IV.E deadline=execution-phase deps=I.C,V.A,V.D -->

**IV.E — Examiner/rater UI + results delivery.** Scoring rubric display (ILR descriptors side-by-side with AALB-equivalent — extends to multi-axis if I.0 validates ACTFL/CEFR audience). Multi-rater consensus view (blind scoring → reveal + discrepancy resolution flow per V.B inter-rater reliability framework). Test-taker results: certificate with anchor level + rationale block. Admin view: raw-data export, longitudinal cohort, validity/reliability dashboards.

### Section V — Psychometrics

<!-- delegation: owner=architect:0 section=V deadline=execution-phase deps=I.B,I.C -->

Co-owned architect:0 + tester:0 (adversarial) + platform-engineer:0 (infrastructure) + developer:1 (statistical pipeline). SME-deferred on descriptor calibration content per §Purpose caveat.

<!-- delegation: owner=architect:0 section=V.A deadline=execution-phase deps=I.A -->

**V.A — Validity.** Content / construct / criterion / consequential / **discriminant** per Kane's argument-based framework. Each anchor tier requires distinct validity evidence. Discriminant-validation sub-step (per tester msg 2124 V.adv #4 + dev-challenger msg 2126 SES-discriminant add): correlate AALB scores against (a) an unrelated cognitive measure such as Raven's Progressive Matrices — target r<0.30 (Kane "construct measures what we say AND NOT something else" inference); (b) an unrelated test-taking-skill measure — target r<0.30; (c) years-of-formal-education — target r<0.40 (strengthens the validity argument against the equity-of-access critique that lands on most high-stakes language certifications).

<!-- delegation: owner=architect:0 section=V.B deadline=execution-phase deps=I.D -->

**V.B — Reliability.** Test-retest stability coefficients at BOTH 30d AND 90d (per tester msg 2124 V.adv #2 — 14d is too short to back AALB's certificate validity-period claim of 6-month/annual windows; 14d data has no empirical basis for those windows and creates a regulatory/compliance gap touching VII). Inter-rater for speaking + writing modalities — target κ≥0.80 for level-assignment decisions at certification-grade (per tester msg 2124 V.adv #3); κ≥0.70 acceptable as year-1 floor with explicit quarterly recalibration plan targeting κ≥0.80 by year 2 (cost-bounded recalibration cadence). Internal consistency for listening + reading modalities — Cronbach alpha or split-half.

<!-- delegation: owner=architect:0 section=V.C deadline=execution-phase deps=I.B,I.C -->

**V.C — Calibration & equating.** IRT-based; Rasch model for AALB-equivalent scale anchoring + ILR concordance. Vertical equating across ILR anchors. Item-difficulty bank maintenance methodology. **Sample-size floor for common-item nonequivalent-groups equating (per tester msg 2124 V.adv #1):** N≥300 is the BARE FLOOR; stable item-parameter estimates need ~500+ when item density at extreme anchors (ILR 0/0+ and 4/4+/5) is thin. Plus-level samples are naturally sparse in AALB population; stratified-sampling design declared upfront with anchor-item replication rate per ILR cell (NOT buried in "pilot N≥300"). **Concordance recertification cadence (per tester msg 2124 V.adv #5):** cost-bounded — coupled to AALB's administration-frequency commitment (declared in I.0 audience-validation deliverable) with explicit minimum baseline of "quarterly OR every 1000 administrations, whichever is sooner." Avoids uncapped cost on quarterly-pilot or monthly-pilot rollouts.

<!-- delegation: owner=architect:0 section=V.D deadline=execution-phase deps=V.C -->

**V.D — Standard-setting & cut scores.** Modified Angoff or bookmark method per ILR anchor. Cut-score validation against external criterion (employer hiring data, university placement, etc. — depends on I.0 audience validation).

<!-- delegation: owner=architect:0 section=V.E deadline=execution-phase deps=I.C -->

**V.E — SME-deferred descriptor calibration.** Explicit shallow-framing per §Purpose caveat. Plan delivers the methodology for descriptor calibration but defers the actual content to AALB SME pass.

<!-- delegation: owner=platform-engineer:0 section=V.X deadline=execution-phase deps=III.A,VII.B -->

**V.X — ASR pipeline for speaking modality auto-scoring (platform-engineer msg 2118).** Whisper-large-v3 (open-source, ~1.5GB model, GPU-required real-time, ~5x-realtime CPU fallback), Google STT (~$0.024/min), Azure Speech (~$0.022/min), Pearson Versant (commercial, L2-learner-specific — likely best accuracy for AALB's ILR construct). Self-hosted Whisper viable at ≥~1500 min/mo (~$300/mo cloud GPU break-even). Vendor lock-in vs cost trade-off mirrors III.C.1 framing.

<!-- delegation: owner=developer:1 section=V.Y deadline=execution-phase deps=V.C -->

**V.Y — Statistical pipeline (IRT/Rasch).** Python (pyirt, psychopy) server-side. Tighter FastAPI backend integration vs R alternative. Spec: API contract, batch-vs-realtime calibration, item-bank update workflow.

**V.Y execution-phase content (developer:1):**

- **Library shortlist:** primary `mirt`-equivalent in Python via `girth` (general item response theory; supports 1PL/2PL/3PL + GRM for polytomous items — needed for AALB's speaking/writing rubric scales). Fallback: `pyirt` (simpler API but only 2PL); explicit R bridge via `rpy2` only if `girth` proves insufficient for plus-level differentiation (target accuracy validated against ETS published IRT benchmarks during V.C concordance study).
- **API contract (FastAPI `/api/v1/psychometrics/*`):**
  - `POST /calibrate-batch`: input `{item_responses: [{candidate_id, item_id, score}]}`, output `{item_params: {item_id: {a, b, c}}, ability_estimates: {candidate_id: theta}, fit_stats: {...}}`. Batch operation; minutes-to-hours runtime for N≥500 calibration sample.
  - `POST /score-realtime`: input `{candidate_id, item_responses: [...]}`, output `{theta, theta_se, ilr_anchor, aalb_band, recommendation: {action, confidence}}`. Sub-second for known item-bank (item params pre-calibrated).
  - `GET /item-bank/:id`: returns item params + exposure stats + last-calibrated timestamp.
  - `POST /item-bank/promote`: gated to architect/manager/human; promotes draft items to live calibrated set after batch calibration meets fit-stat thresholds (RMSEA<0.06, CFI>0.95, item-fit infit/outfit 0.7-1.3 per ITC standards).
- **Batch-vs-realtime calibration policy:** item parameters live in two tiers — `calibrated` (locked, used for realtime scoring) and `draft` (collecting responses, awaiting promotion). Calibration runs quarterly OR per-1000-administrations per tester msg 2124 V.adv #5; promotion gated on fit stats above + architect/human sign-off. Realtime scoring only uses `calibrated` item params; draft items run in parallel for data collection but don't affect candidate scoring.
- **Item-bank update workflow:** new items authored by content-author role (vacant; SME-deferred per §Purpose); enter as `draft`; collect ≥N=300 responses per item; batch calibration computes draft params; review for fit-stat thresholds + content-validity SME pass; promote to `calibrated` via `POST /item-bank/promote`. Items failing fit-stat thresholds get item-review workflow (revise / retire / re-draft).
- **Discriminant-validity computation (cross-cut to V.A discriminant-validation sub-step per tester msg 2124 + dev-challenger msg 2126):** during concordance study, collect Raven's Progressive Matrices subset + test-taking-skill questionnaire + education-level demographic alongside Spanish-proficiency responses. Compute Pearson r between AALB theta and each non-target construct. Target r<0.30 with Raven's + r<0.30 with test-taking-skill + r<0.40 with education-level. Surface in `/api/v1/psychometrics/discriminant-report` endpoint for V.adversarial audit per tester msg 2124 V.adv #4.
- **Storage:** PostgreSQL `psychometrics.item_params`, `psychometrics.ability_estimates`, `psychometrics.calibration_runs` tables. Indexed on `item_id` + `candidate_id` + `calibration_run_id`. Append-only — historical calibrations preserved for concordance audit + accreditation review.

<!-- delegation: owner=platform-engineer:0 section=V.Z deadline=execution-phase deps=III.E,IV.E -->

**V.Z — Multi-rater consensus infrastructure.** WebSocket-stable infra (composes with III.E). Disagreement-resolution flow benefits from the Affordance-C-style queue pattern from v1 (moderator-decides-pivot transfers cleanly to lead-rater-arbitrates-disagreement).

<!-- delegation: owner=tester:0 section=V.adversarial deadline=execution-phase deps=V.A,V.B,V.C,V.D,V.X,V.Z -->

**V.adversarial — Psychometric over-claim + test-validity gap audit (tester:0).** Tester audits V.A-D + V.X/Z for psychometric over-claim, construct-validity gap, threat-to-internal-validity, and SME-deferral honesty. Five itemized deliverables per tester msg 2124:

1. **Sample-size auditing:** verify V.C's N≥300/N≥500 floor + stratified-sampling design is honored at pilot stage; flag if items at sparse anchors (0/0+/4/4+/5) have insufficient replication rates.
2. **Test-retest interval audit:** verify V.B 30d + 90d stability coefficients land before certificate validity-period claims ship; reject 14d-only stability evidence as insufficient for 6-month/annual claims.
3. **Inter-rater κ ambition audit:** verify V.B's κ≥0.80 target is held for plus-level differentiation OR explicit year-1 floor + year-2 ambition recalibration plan is documented; reject pre-launch claim of plus-level accuracy at κ=0.70 without the staged plan.
4. **Discriminant-validity audit:** verify V.A discriminant sub-step actually delivers r-correlations against Raven's, test-taking-skill measure, AND education-level; reject Kane validity argument that omits the discriminant inference.
5. **Concordance cost-bounding audit:** verify V.C's recertification cadence is cost-bounded against AALB administration-frequency commitment; flag if uncapped recertification is proposed.

### Section VI — Operations

<!-- delegation: owner=developer:1 section=VI deadline=execution-phase deps=III,IV -->

**VI.A — Scheduling.** Registration flow, time-zone handling, test-window policy (open vs fixed-slot), cancellation/reschedule policy.

<!-- delegation: owner=developer:1 section=VI.A deadline=execution-phase deps=VI -->

**VI.B — Examiner & rater pool.** Recruitment, training, certification (composes with I.B rater calibration), ongoing recalibration cadence, compensation framework.

<!-- delegation: owner=developer:1 section=VI.B deadline=execution-phase deps=VI,I.B -->

**VI.C — Retake & appeal policy.** Cooldown period, retake count limits, appeal procedure (item-challenge, score-challenge, accommodation-challenge), evidence requirements.

<!-- delegation: owner=developer:1 section=VI.C deadline=execution-phase deps=VI -->

**VI.D — Results delivery & verification.** Score-report delivery channel (email, portal, API for sponsoring institutions), verification API for third-party employers/universities, certificate authenticity (cryptographic signatures or registry lookup).

<!-- delegation: owner=developer:1 section=VI.D deadline=execution-phase deps=VI,IV.E -->

### Section VII — Compliance & Standards

<!-- delegation: owner=architect:0 section=VII deadline=execution-phase deps= -->

cd-compliance:0 vacant — flagged for explicit SME-deferral on accreditation grades.

<!-- delegation: owner=architect:0 section=VII.A deadline=execution-phase deps=VII -->

**VII.A — ACTFL / ILR alignment.** Methodology for claiming alignment with industry standards. If multi-axis audience validates per I.0, ACTFL + CEFR alignment lands here too.

<!-- delegation: owner=architect:0 section=VII.B deadline=execution-phase deps=VII -->

**VII.B — ADA / Section 508 accessibility compliance.** Documented-accommodations workflow (request → approval → applied-config at test launch). Cross-cut to IV.C.

<!-- delegation: owner=architect:0 section=VII.C deadline=execution-phase deps=VII -->

**VII.C — Data privacy (GDPR / state laws / FERPA-equivalent).** Biometric data handling (cross-cut III.C.1 vendor PIA), retention policy (7-year industry norm vs jurisdictional limits), data-subject rights (access, deletion, portability), breach notification.

<!-- delegation: owner=architect:0 section=VII.D deadline=execution-phase deps=VII -->

**VII.D — Record retention.** Score records, audio/video evidence, audit trail. Cross-cut to VI.D results delivery and VII.C privacy.

### Section VIII — Pilot & Rollout

<!-- delegation: owner=developer:1 section=VIII deadline=execution-phase deps=I,III,V,VII -->

**VIII.A — Pilot population & sample size.** Target test-taker count for initial validation. Stratification across ILR anchors. Inclusion criteria.

<!-- delegation: owner=developer:1 section=VIII.A deadline=execution-phase deps=VIII -->

**VIII.B — Validation study design.** Concurrent validity against external benchmark (e.g., OPI scores from existing certified raters). Reliability estimation. Item-bank calibration update from pilot data.

<!-- delegation: owner=developer:1 section=VIII.B deadline=execution-phase deps=VIII,V -->

**VIII.C — Production launch sequence.** Phase 1 sponsored AALB admin (per III.B phased) → Phase 2 self-assessment → Phase 3 hybrid. Go-live milestones, rollback criteria.

<!-- delegation: owner=developer:1 section=VIII.C deadline=execution-phase deps=VIII,III.B -->

**VIII.D — Recognition pathway.** Accreditation-body submissions (ACTFL, etc.), employer recognition outreach, university articulation agreements. SME-deferred on actual accreditation process.

<!-- delegation: owner=developer:1 section=VIII.D deadline=execution-phase deps=VIII -->

## Adversarial review

<!-- delegation: owner=evil-architect:0 section=adversarial.architecture deadline=execution-phase deps=I,III,V,VII -->
<!-- delegation: owner=dev-challenger:0 section=adversarial.threat-model deadline=execution-phase deps=III,IV.C -->
<!-- delegation: owner=tester:0 section=adversarial.test-validity deadline=execution-phase deps=V -->

## Cross-references

- v7 spec: `.vaak/design-notes/collaborative-proposal-workflow-spec-2026-05-15.md`
- Convergence round 1 board record: msgs 2097-2121
- Vacant owners flagged: ux-engineer:0 (IV.D, II.candidate-perspective), cd-accessibility:0 (IV.C cross-cut), cd-compliance:0 (VII cross-cut)
