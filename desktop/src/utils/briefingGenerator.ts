/**
 * Briefing template generator for custom roles.
 *
 * Generates structured markdown briefings from role metadata,
 * tags, permissions, and peer role relationships.
 */

// ---------------------------------------------------------------------------
// Canonical tags — the known tag vocabulary with descriptions
// ---------------------------------------------------------------------------

export interface CanonicalTag {
  id: string;
  label: string;
  description: string;
}

export const CANONICAL_TAGS: CanonicalTag[] = [
  { id: "implementation", label: "Implementation", description: "Writes and modifies code" },
  { id: "code-review", label: "Code Review", description: "Reviews code quality and correctness" },
  { id: "testing", label: "Testing", description: "Validates implementations, writes tests" },
  { id: "architecture", label: "Architecture", description: "Designs system structure and patterns" },
  { id: "moderation", label: "Moderation", description: "Runs structured discussions and debates" },
  { id: "security", label: "Security", description: "Security analysis and auditing" },
  { id: "compliance", label: "Compliance", description: "Regulatory and policy compliance" },
  { id: "analysis", label: "Analysis", description: "Research, investigation, and analysis" },
  { id: "coordination", label: "Coordination", description: "Task management and team coordination" },
  { id: "red-team", label: "Red Team", description: "Adversarial testing and attack simulation" },
  { id: "documentation", label: "Documentation", description: "Writes docs, specs, and technical writing" },
  { id: "debugging", label: "Debugging", description: "Diagnoses and resolves bugs" },
];

// Lookup map for quick access
const TAG_MAP: Record<string, CanonicalTag> = {};
for (const tag of CANONICAL_TAGS) {
  TAG_MAP[tag.id] = tag;
}

// ---------------------------------------------------------------------------
// Permission descriptions
// ---------------------------------------------------------------------------

const PERMISSION_DESCRIPTIONS: Record<string, string> = {
  broadcast: "You can send messages to all team members simultaneously.",
  review: "You can review and approve/reject others' work.",
  assign_tasks: "You can assign tasks to other team members.",
  status: "You can post status updates about your work.",
  question: "You can ask questions to other team members.",
  handoff: "You can hand off completed work to other roles.",
  moderation: "You can moderate structured discussions and debates.",
};

// ---------------------------------------------------------------------------
// Tag-based function descriptions
// ---------------------------------------------------------------------------

const TAG_FUNCTIONS: Record<string, string> = {
  implementation: "You write and modify code to implement features and fix bugs.",
  "code-review": "You review code for quality, correctness, and adherence to project patterns.",
  testing: "You validate implementations through testing and report defects.",
  architecture: "You design system architecture and ensure technical consistency across the codebase.",
  moderation: "You facilitate structured discussions, enforce rules, and maintain neutrality.",
  security: "You analyze code and systems for security vulnerabilities and recommend mitigations.",
  compliance: "You ensure implementations meet regulatory requirements and organizational policies.",
  analysis: "You research topics, gather information, and produce analytical reports.",
  coordination: "You coordinate work across team members, manage priorities, and track progress.",
  "red-team": "You perform adversarial testing to find weaknesses in designs and implementations.",
  documentation: "You write documentation, specifications, and technical guides.",
  debugging: "You diagnose root causes of bugs and develop targeted fixes.",
};

// ---------------------------------------------------------------------------
// Anti-pattern templates based on role type
// ---------------------------------------------------------------------------

function generateAntiPatterns(tags: string[], permissions: string[]): string {
  const patterns: string[] = [];

  // Universal anti-patterns
  patterns.push("- NEVER send acknowledgment-only messages (\"Got it\", \"Will do\")");
  patterns.push("- NEVER fill your turn with performance content when you have nothing substantive — pass instead (see Turn Discipline)");
  patterns.push("- NEVER relay the human's words back to them");

  if (tags.includes("moderation")) {
    patterns.push("- NEVER express personal opinions on debate topics");
    patterns.push("- NEVER weight one side's arguments over another");
  }

  if (tags.includes("implementation") || tags.includes("debugging")) {
    patterns.push("- NEVER modify code you haven't read first");
    patterns.push("- NEVER skip claiming files before editing");
  }

  if (tags.includes("code-review")) {
    patterns.push("- NEVER approve work without reading the actual code changes");
  }

  if (tags.includes("testing")) {
    patterns.push("- NEVER mark tests as passing without actually running them");
  }

  if (tags.includes("security") || tags.includes("red-team")) {
    patterns.push("- NEVER disclose discovered vulnerabilities publicly before they are fixed");
  }

  if (!permissions.includes("broadcast")) {
    patterns.push("- NEVER attempt to broadcast messages — use directed messages to specific roles");
  }

  return patterns.join("\n");
}

// ---------------------------------------------------------------------------
// Turn discipline generator (Assembly Line: passing-as-default culture)
// ---------------------------------------------------------------------------

function generateTurnDiscipline(tags: string[]): string {
  const isAdversarial = tags.some((t) => t === "red-team" || t === "security");

  const actBullets: string[] = [];
  const passBullets: string[] = [
    "- Nothing has changed direction or advanced the work since the previous speaker",
    "- The previous speaker covered your lens completely",
    "- You would otherwise send \"agree\" / \"endorsing in full\" / acknowledgment-only content",
  ];

  if (tags.includes("implementation") || tags.includes("debugging")) {
    actBullets.push("- A code assignment, bug report, or implementation question lands");
    actBullets.push("- You have a status update from work in progress");
  }
  if (tags.includes("code-review")) {
    actBullets.push("- New code commits land that haven't been reviewed");
    actBullets.push("- A peer requests review of a specific artifact");
  }
  if (tags.includes("testing")) {
    actBullets.push("- A testable artifact (commit, build, spec) lands");
    actBullets.push("- A test failure or regression needs to be reported");
  }
  if (tags.includes("architecture")) {
    actBullets.push("- A design decision needs arbitration or a spec needs drafting");
    actBullets.push("- A pattern violation requires correction");
  }
  if (tags.includes("red-team") || tags.includes("security")) {
    actBullets.push("- A new contract, spec, or commit needs adversarial review");
    actBullets.push("- A finding has been missed, downgraded, or theatrically fixed");
  }
  if (tags.includes("moderation")) {
    actBullets.push("- A structured discussion (Delphi, Oxford, Red Team) needs to be run");
    actBullets.push("- A debate rule has been violated");
  }
  if (tags.includes("coordination")) {
    actBullets.push("- An assignment needs to be routed or priorities arbitrated");
  }
  if (tags.includes("analysis")) {
    actBullets.push("- A research or investigation question is open");
  }
  if (tags.includes("documentation")) {
    actBullets.push("- A doc gap, spec request, or update lands");
  }
  if (tags.includes("compliance")) {
    actBullets.push("- A regulatory checkpoint is reached or a compliance risk is identified");
  }
  if (actBullets.length === 0) {
    actBullets.push("- A directive in your scope lands");
  }
  actBullets.push("- You have substantive content that changes direction or advances the work");

  const adversarialNote = isAdversarial
    ? "\n\n**Adversarial-lens note:** Your pass threshold is LOWER than non-adversarial roles. When a new spec, contract, or commit lands, you should act unless you have verified nothing was missed. Silence from your lens after a contract change is itself a finding."
    : "";

  return `When the mic lands on you, you have one decision: act or pass. Passing is the default. A round where most agents pass and a couple do real work is a SUCCESSFUL round, not a failed one.

**Act when:**
${actBullets.join("\n")}

**Pass when:**
${passBullets.join("\n")}

**How to pass:** send one short message stating what the previous speaker did and that you have nothing to add. Example: \`project_send(to="all", type="status", subject="passing", body="Read msg N from <speaker>. No add from <my lens>.")\`. Then the mic rotates. Do NOT fill your turn with performance content; "endorsing in full" without substantive add is a pass — say so directly.${adversarialNote}`;
}

// ---------------------------------------------------------------------------
// Peer relationship generator
// ---------------------------------------------------------------------------

export interface PeerRole {
  slug: string;
  title: string;
  description: string;
  tags: string[];
  permissions: string[];
  /** Character/stats per human msg 3254 + spec. Optional — legacy peers
      without stats omit; specialist-lookup falls back to generic role names. */
  stats?: RoleStats;
}

/**
 * Character/stats system per human msg 3254 + spec at
 * .vaak/design-notes/character-stats-system-2026-05-16.md.
 * Each axis 1-10. Mirrors RoleStats in collab.rs.
 */
export interface RoleStats {
  td: number; // Technical Depth
  ar: number; // Adversarial Rigor
  cp: number; // Communication Precision
  do: number; // Domain Ownership
  pd: number; // Process Discipline
  ja: number; // Judgment Under Ambiguity
}

const STAT_DIMENSIONS: Array<{ key: keyof RoleStats; label: string }> = [
  { key: "td", label: "Technical Depth" },
  { key: "ar", label: "Adversarial Rigor" },
  { key: "cp", label: "Communication Precision" },
  { key: "do", label: "Domain Ownership" },
  { key: "pd", label: "Process Discipline" },
  { key: "ja", label: "Judgment Under Ambiguity" },
];

/**
 * Generate the cognitive-budget framing block per spec §5.
 *
 * CRITICAL per spec §5 + evil-arch msg 3263 §4: NO LITERAL NUMBERS in
 * output text. Stats are read as DATA; output is FRAMING TEXT only.
 * Prevents recursive "per my AR=10..." citation pattern.
 *
 * CRITICAL per spec §3 corollary + dev-challenger msg 3266 §5: low stats
 * bias attention budget but do NOT exempt the agent from verification
 * responsibilities. Multi-verifier coverage is load-bearing.
 */
function generateStatFraming(
  roleTitle: string,
  stats: RoleStats,
  peers: PeerRole[],
): string {
  // For each stat with stat ≤ 6, find the peer with the highest matching
  // stat to name as deferral target. Tie-break alphabetical for stability.
  const findSpecialist = (dim: keyof RoleStats): string | null => {
    let best: PeerRole | null = null;
    for (const peer of peers) {
      if (!peer.stats) continue;
      if (best == null || peer.stats[dim] > best.stats![dim] ||
          (peer.stats[dim] === best.stats![dim] && peer.slug < best.slug)) {
        best = peer;
      }
    }
    return best ? best.title : null;
  };

  const lines: string[] = [];
  for (const { key, label } of STAT_DIMENSIONS) {
    const v = stats[key];
    if (v >= 9) {
      lines.push(`- You're the team's strongest voice on ${label}. Lead here.`);
    } else if (v >= 7) {
      lines.push(`- Strong on ${label}. Engage when needed.`);
    } else if (v >= 5) {
      const specialist = findSpecialist(key);
      const target = specialist ? specialist : "the team's specialist";
      lines.push(
        `- ${label} isn't your primary focus. When complex ${label} decisions arise, flag them for ${target}. Your cognitive budget is better spent on your strongest dimensions.`,
      );
    } else {
      const specialist = findSpecialist(key);
      const target = specialist ? specialist : "the team's specialist";
      lines.push(`- ${label} is explicitly outside your scope. Always defer to ${target}.`);
    }
  }

  return `## 0. Your Cognitive Budget

You're playing the role of ${roleTitle}. You have a limited cognitive budget; spend it where you're the team's strongest voice. Below: what to lead on, what to flag for specialists.

${lines.join("\n")}

**Verification responsibility preserved:** your stat profile biases your cognitive budget toward your 9s and 10s, but does NOT exempt you from verification responsibilities. If a peer specialist's output crosses your read path, you still independently verify what crosses your lane — multi-verifier coverage is a safety net, not redundant overhead.
`;
}

function generatePeerRelationships(
  myTags: string[],
  peers: PeerRole[]
): string {
  if (peers.length === 0) return "No other roles are currently defined.";

  const lines: string[] = [];
  for (const peer of peers) {
    const sharedTags = myTags.filter((t) => peer.tags.includes(t));
    let relationship: string;

    if (sharedTags.length > 0) {
      const overlap = sharedTags.map((t) => TAG_MAP[t]?.label || t).join(", ");
      relationship = `${peer.title} — ${peer.description}. Shared focus: ${overlap}.`;
    } else {
      relationship = `${peer.title} — ${peer.description}.`;
    }

    // Add interaction hints based on permissions
    if (peer.permissions.includes("assign_tasks")) {
      relationship += " Can assign you work.";
    }
    if (peer.permissions.includes("review")) {
      relationship += " Can review your output.";
    }

    lines.push(`- **${peer.slug}**: ${relationship}`);
  }

  return lines.join("\n");
}

// ---------------------------------------------------------------------------
// Main briefing generator
// ---------------------------------------------------------------------------

export interface BriefingInput {
  title: string;
  description: string;
  tags: string[];
  permissions: string[];
  peers: PeerRole[];
  maxInstances?: number;
  /** Character/stats per human msg 3254. Optional — when absent the
      cognitive-budget framing section is omitted (legacy roles). */
  stats?: RoleStats;
}

export function generateBriefing(input: BriefingInput): string {
  const { title, description, tags, permissions, peers, maxInstances, stats } = input;

  // Section 0: Cognitive-budget framing (per human msg 3254 character/stats
  // system). Renders before Identity when stats present. Empty string for
  // legacy roles without stats — keeps Phase 1 backward-compatible.
  const cognitiveBudget = stats ? generateStatFraming(title, stats, peers) : "";

  // Section 1: Identity
  const identity = `You are the ${title}. ${description}`;

  // Section 2: Primary function (from tags)
  const functions = tags
    .map((t) => TAG_FUNCTIONS[t])
    .filter(Boolean);
  const primaryFunction =
    functions.length > 0
      ? functions.join("\n")
      : "Your responsibilities are defined by the team lead.";

  // Section 3: Anti-patterns
  const antiPatterns = generateAntiPatterns(tags, permissions);

  // Section 4: Turn discipline (Assembly Line: passing-as-default)
  const turnDiscipline = generateTurnDiscipline(tags);

  // Section 5: Peer relationships
  const peerRelationships = generatePeerRelationships(tags, peers);

  // Section 6: Multi-instance coordination (only for roles with max_instances > 1)
  const multiInstanceSection = (maxInstances ?? 1) > 1
    ? `\n## 6. Multi-Instance Coordination

When multiple instances of this role are active:
1. ALWAYS check \`project_claims\` before starting ANY file work
2. If another instance already claimed the files you need, pick a different task or coordinate via \`project_send\`
3. When the manager assigns a task to your role generically (not your specific instance), the FIRST instance to claim the files owns the task — other instances must wait for a different assignment
4. NEVER work on the same file as another instance of your role
5. If you see a directive addressed to your role without an instance number, check if another instance already started it before beginning
`
    : "";

  // Action boundary (from permissions)
  const actionLines = permissions
    .map((p) => PERMISSION_DESCRIPTIONS[p])
    .filter(Boolean)
    .map((d) => `- ${d}`);
  const actionBoundary =
    actionLines.length > 0
      ? actionLines.join("\n")
      : "- You can send directed messages to specific roles.";

  // Onboarding section
  const onboarding = [
    "1. Read the codebase files relevant to your function",
    "2. Check the message board for context on current work",
    "3. Claim files before editing using `project_claim`",
    "4. Report completion via `project_send` when done",
  ].join("\n");

  const sectionNum = (maxInstances ?? 1) > 1 ? 7 : 6;

  return `# ${title}
${cognitiveBudget}
## 1. Identity
${identity}

## 2. Primary Function
${primaryFunction}

## 3. Anti-patterns
${antiPatterns}

## 4. Turn Discipline (Assembly Line)
${turnDiscipline}

## 5. Peer Relationships
${peerRelationships}
${multiInstanceSection}
## ${sectionNum}. Action Boundary
${actionBoundary}

## ${sectionNum + 1}. Onboarding
${onboarding}
`;
}

// ---------------------------------------------------------------------------
// Role template presets
// ---------------------------------------------------------------------------

export interface RoleTemplate {
  id: string;
  title: string;
  description: string;
  tags: string[];
  permissions: string[];
  maxInstances: number;
}

export const ROLE_TEMPLATES: RoleTemplate[] = [
  {
    id: "researcher",
    title: "Researcher",
    description: "Gathers information, investigates topics, and produces analytical reports",
    tags: ["analysis", "documentation"],
    permissions: ["status", "question"],
    maxInstances: 1,
  },
  {
    id: "security-auditor",
    title: "Security Auditor",
    description: "Analyzes code and systems for security vulnerabilities, performs threat modeling",
    tags: ["security", "red-team", "code-review"],
    permissions: ["status", "review"],
    maxInstances: 1,
  },
  {
    id: "devops-engineer",
    title: "DevOps Engineer",
    description: "Manages infrastructure, deployment pipelines, and operational tooling",
    tags: ["implementation", "testing"],
    permissions: ["status", "handoff"],
    maxInstances: 1,
  },
  {
    id: "technical-writer",
    title: "Technical Writer",
    description: "Writes documentation, API specs, user guides, and technical references",
    tags: ["documentation"],
    permissions: ["status"],
    maxInstances: 1,
  },
  {
    id: "domain-expert",
    title: "Domain Expert",
    description: "Provides specialized knowledge and analysis for a specific domain",
    tags: ["analysis"],
    permissions: ["status", "question"],
    maxInstances: 1,
  },
  {
    id: "qa-lead",
    title: "QA Lead",
    description: "Plans test strategies, coordinates quality assurance, and reviews test coverage",
    tags: ["testing", "code-review", "coordination"],
    permissions: ["status", "review", "assign_tasks"],
    maxInstances: 1,
  },
  {
    id: "pair-programmer",
    title: "Pair Programmer",
    description: "Works interactively alongside another role, writing and reviewing code together",
    tags: ["implementation", "code-review", "debugging"],
    permissions: ["status", "handoff", "question"],
    maxInstances: 1,
  },
  {
    id: "sprint-lead",
    title: "Sprint Lead",
    description: "Tech lead who coordinates work, reviews code, and ensures quality across the sprint",
    tags: ["coordination", "code-review", "testing"],
    permissions: ["assign_tasks", "review", "status", "broadcast"],
    maxInstances: 1,
  },
];
