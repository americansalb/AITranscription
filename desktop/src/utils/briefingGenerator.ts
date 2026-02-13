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
// Peer relationship generator
// ---------------------------------------------------------------------------

export interface PeerRole {
  slug: string;
  title: string;
  description: string;
  tags: string[];
  permissions: string[];
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
}

export function generateBriefing(input: BriefingInput): string {
  const { title, description, tags, permissions, peers, maxInstances } = input;

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

  // Section 4: Peer relationships
  const peerRelationships = generatePeerRelationships(tags, peers);

  // Section 5: Multi-instance coordination (only for roles with max_instances > 1)
  const multiInstanceSection = (maxInstances ?? 1) > 1
    ? `\n## 5. Multi-Instance Coordination

When multiple instances of this role are active:
1. ALWAYS check \`project_claims\` before starting ANY file work
2. If another instance already claimed the files you need, pick a different task or coordinate via \`project_send\`
3. When the manager assigns a task to your role generically (not your specific instance), the FIRST instance to claim the files owns the task — other instances must wait for a different assignment
4. NEVER work on the same file as another instance of your role
5. If you see a directive addressed to your role without an instance number, check if another instance already started it before beginning
`
    : "";

  // Section 6 (or 5 if no multi-instance): Action boundary (from permissions)
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

  const sectionNum = (maxInstances ?? 1) > 1 ? 6 : 5;

  return `# ${title}

## 1. Identity
${identity}

## 2. Primary Function
${primaryFunction}

## 3. Anti-patterns
${antiPatterns}

## 4. Peer Relationships
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
