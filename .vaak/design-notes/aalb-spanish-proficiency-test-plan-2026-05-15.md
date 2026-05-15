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

<!-- delegation: owner=developer:1 section=III.B deadline=execution-phase deps=III.A -->

**III.B — Secure browser / lockdown environment (PHASED).** Phase 1: Tauri-native (kiosk-mode, no copy/paste, no other windows, no screen recording) for sponsored AALB admin. Phase 2: lockdown-browser (Chrome enterprise policies + tab-isolation) for self-assessment. Phase 3: hybrid (native shell loading sandboxed web content) when mobile in-scope. Honest threat-model acknowledgments per dev-challenger msg 2110 #4: gaze tracking and keyboard cadence have high FP rates; live human proctor is the primary control for accreditation-grade testing; VM-detection (CPUID hypervisor bit, IOMMU signatures, clock-jitter) is Phase 1 limitation explicitly documented; off-screen-accomplice via secondary device is uncatchable without live proctor.

<!-- delegation: owner=platform-engineer:0 section=III.C deadline=execution-phase deps=III.A,VII.A -->

**III.C — Identity verification.** Photo ID OCR + face-match. Live selfie at session start matched against pre-registered photo OR government ID. Random re-verification mid-test if proctor flags suspicion. Fall back to manual review when automated confidence < 0.95. Cross-platform: WebRTC getUserMedia uniform on desktop; iOS Safari needs native shell. OS camera-permission flows asymmetric (macOS TCC prompt, Windows silent, Linux varies) → OS-conditional error copy.

<!-- delegation: owner=platform-engineer:0 section=III.C.1 deadline=execution-phase deps=III.C,VII -->

**III.C.1 — Self-hosted vs vendor trade-off + privacy-impact analysis (per dev-challenger msg 2110 #3 + platform-engineer msg 2115).** Self-hosted (face_recognition + dlib or InsightFace, ~$200-500/mo GPU) vs commercial vendor (Veriff/Persona/AWS Rekognition/Azure Face, ~$50/test, break-even ~10K tests/mo). Privacy: biometric data shipped to third-party conflicts with GDPR/state laws (CCPA, BIPA, etc.); explicit dep on VII Compliance for jurisdictional scope.

<!-- delegation: owner=developer:1 section=III.D deadline=execution-phase deps=III.A,III.C,IV.C -->

**III.D — Real-time proctoring pipeline.** WebRTC peer connection to proctor monitor (or SFU like mediasoup for multi-candidate batching). Behavioral signals: gaze tracking (face landmarks), audio voice continuity, keyboard cadence, screen-region focus. Honest threat-model labeling per dev-challenger msg 2110 #4: what's caught vs not. Post-hoc review compiles candidate session to mp4 + event timeline; proctor scrubs for anomalies. Critical cross-cut to IV.C: anti-cheat engine reads applied-accommodation profile and suppresses corresponding signals (screen-reader use ≠ off-screen-look; extended-time ≠ extended-idle = cheat).

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
