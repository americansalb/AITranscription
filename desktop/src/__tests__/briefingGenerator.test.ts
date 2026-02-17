/**
 * Tests for briefingGenerator — role briefing template generation.
 *
 * Covers:
 *   - CANONICAL_TAGS: all 12 tags present with required fields
 *   - ROLE_TEMPLATES: all 8 templates with valid structure
 *   - generateBriefing: section structure, tag-based functions, anti-patterns,
 *     peer relationships, multi-instance coordination, action boundaries
 *   - Edge cases: empty tags, empty peers, unknown tags
 */
import { describe, it, expect } from "vitest";
import {
  CANONICAL_TAGS,
  ROLE_TEMPLATES,
  generateBriefing,
  type BriefingInput,
  type PeerRole,
  type RoleTemplate,
} from "../utils/briefingGenerator";


// =============================================================================
// CANONICAL_TAGS
// =============================================================================

describe("CANONICAL_TAGS", () => {
  it("has exactly 12 tags", () => {
    expect(CANONICAL_TAGS.length).toBe(12);
  });

  it("each tag has id, label, and description", () => {
    for (const tag of CANONICAL_TAGS) {
      expect(tag.id).toBeTruthy();
      expect(tag.label).toBeTruthy();
      expect(tag.description).toBeTruthy();
    }
  });

  it("all expected tag IDs are present", () => {
    const ids = CANONICAL_TAGS.map(t => t.id);
    const expected = [
      "implementation", "code-review", "testing", "architecture",
      "moderation", "security", "compliance", "analysis",
      "coordination", "red-team", "documentation", "debugging",
    ];
    for (const id of expected) {
      expect(ids).toContain(id);
    }
  });

  it("has no duplicate IDs", () => {
    const ids = CANONICAL_TAGS.map(t => t.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});


// =============================================================================
// ROLE_TEMPLATES
// =============================================================================

describe("ROLE_TEMPLATES", () => {
  it("has exactly 8 templates", () => {
    expect(ROLE_TEMPLATES.length).toBe(8);
  });

  it("each template has required fields", () => {
    for (const template of ROLE_TEMPLATES) {
      expect(template.id).toBeTruthy();
      expect(template.title).toBeTruthy();
      expect(template.description).toBeTruthy();
      expect(Array.isArray(template.tags)).toBe(true);
      expect(template.tags.length).toBeGreaterThan(0);
      expect(Array.isArray(template.permissions)).toBe(true);
      expect(template.permissions.length).toBeGreaterThan(0);
      expect(template.maxInstances).toBeGreaterThanOrEqual(1);
    }
  });

  it("all template tags reference valid canonical tag IDs", () => {
    const validIds = CANONICAL_TAGS.map(t => t.id);
    for (const template of ROLE_TEMPLATES) {
      for (const tag of template.tags) {
        expect(validIds).toContain(tag);
      }
    }
  });

  it("has no duplicate template IDs", () => {
    const ids = ROLE_TEMPLATES.map(t => t.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("expected template IDs are present", () => {
    const ids = ROLE_TEMPLATES.map(t => t.id);
    const expected = [
      "researcher", "security-auditor", "devops-engineer",
      "technical-writer", "domain-expert", "qa-lead",
      "pair-programmer", "sprint-lead",
    ];
    for (const id of expected) {
      expect(ids).toContain(id);
    }
  });
});


// =============================================================================
// generateBriefing — Basic structure
// =============================================================================

describe("generateBriefing structure", () => {
  const minimalInput: BriefingInput = {
    title: "Test Role",
    description: "A test role for unit testing.",
    tags: ["testing"],
    permissions: ["status"],
    peers: [],
  };

  it("returns a markdown string with the role title as heading", () => {
    const result = generateBriefing(minimalInput);
    expect(result.startsWith("# Test Role")).toBe(true);
  });

  it("contains all 6 required sections for single-instance role", () => {
    const result = generateBriefing(minimalInput);
    expect(result).toContain("## 1. Identity");
    expect(result).toContain("## 2. Primary Function");
    expect(result).toContain("## 3. Anti-patterns");
    expect(result).toContain("## 4. Peer Relationships");
    expect(result).toContain("## 5. Action Boundary");
    expect(result).toContain("## 6. Onboarding");
  });

  it("includes identity text with title and description", () => {
    const result = generateBriefing(minimalInput);
    expect(result).toContain("You are the Test Role. A test role for unit testing.");
  });

  it("includes onboarding steps", () => {
    const result = generateBriefing(minimalInput);
    expect(result).toContain("Read the codebase files relevant to your function");
    expect(result).toContain("Check the message board for context");
    expect(result).toContain("Claim files before editing");
    expect(result).toContain("Report completion via");
  });
});


// =============================================================================
// generateBriefing — Tag-based functions
// =============================================================================

describe("generateBriefing primary functions", () => {
  it("includes function description for testing tag", () => {
    const result = generateBriefing({
      title: "Tester",
      description: "Tests things.",
      tags: ["testing"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("validate implementations through testing");
  });

  it("includes function description for implementation tag", () => {
    const result = generateBriefing({
      title: "Dev",
      description: "Develops.",
      tags: ["implementation"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("write and modify code");
  });

  it("includes multiple function descriptions for multiple tags", () => {
    const result = generateBriefing({
      title: "Multi",
      description: "Multi role.",
      tags: ["implementation", "code-review", "testing"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("write and modify code");
    expect(result).toContain("review code for quality");
    expect(result).toContain("validate implementations");
  });

  it("provides fallback when no valid tags", () => {
    const result = generateBriefing({
      title: "Unknown",
      description: "No tags.",
      tags: ["nonexistent-tag"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("responsibilities are defined by the team lead");
  });

  it("provides fallback for empty tags array", () => {
    const result = generateBriefing({
      title: "Empty",
      description: "No tags.",
      tags: [],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("responsibilities are defined by the team lead");
  });
});


// =============================================================================
// generateBriefing — Anti-patterns
// =============================================================================

describe("generateBriefing anti-patterns", () => {
  it("always includes universal anti-patterns", () => {
    const result = generateBriefing({
      title: "Any",
      description: "Any role.",
      tags: ["analysis"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("NEVER send acknowledgment-only messages");
    expect(result).toContain("NEVER relay the human's words");
  });

  it("includes moderation-specific anti-patterns for moderation tag", () => {
    const result = generateBriefing({
      title: "Moderator",
      description: "Moderates.",
      tags: ["moderation"],
      permissions: ["moderation"],
      peers: [],
    });
    expect(result).toContain("NEVER express personal opinions on debate topics");
    expect(result).toContain("NEVER weight one side's arguments over another");
  });

  it("includes implementation-specific anti-patterns", () => {
    const result = generateBriefing({
      title: "Dev",
      description: "Develops.",
      tags: ["implementation"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("NEVER modify code you haven't read first");
    expect(result).toContain("NEVER skip claiming files before editing");
  });

  it("includes testing-specific anti-patterns", () => {
    const result = generateBriefing({
      title: "Tester",
      description: "Tests.",
      tags: ["testing"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("NEVER mark tests as passing without actually running them");
  });

  it("includes code-review anti-pattern", () => {
    const result = generateBriefing({
      title: "Reviewer",
      description: "Reviews.",
      tags: ["code-review"],
      permissions: ["review"],
      peers: [],
    });
    expect(result).toContain("NEVER approve work without reading the actual code changes");
  });

  it("includes security anti-pattern for security tag", () => {
    const result = generateBriefing({
      title: "Security",
      description: "Secures.",
      tags: ["security"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("NEVER disclose discovered vulnerabilities publicly");
  });

  it("includes no-broadcast anti-pattern when broadcast not in permissions", () => {
    const result = generateBriefing({
      title: "Limited",
      description: "Limited.",
      tags: ["analysis"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("NEVER attempt to broadcast messages");
  });

  it("omits no-broadcast anti-pattern when broadcast IS in permissions", () => {
    const result = generateBriefing({
      title: "Broadcaster",
      description: "Can broadcast.",
      tags: ["analysis"],
      permissions: ["status", "broadcast"],
      peers: [],
    });
    expect(result).not.toContain("NEVER attempt to broadcast messages");
  });
});


// =============================================================================
// generateBriefing — Peer relationships
// =============================================================================

describe("generateBriefing peer relationships", () => {
  const peer1: PeerRole = {
    slug: "developer",
    title: "Developer",
    description: "Writes code",
    tags: ["implementation", "debugging"],
    permissions: ["status", "handoff"],
  };

  const peer2: PeerRole = {
    slug: "manager",
    title: "Project Manager",
    description: "Coordinates work",
    tags: ["coordination"],
    permissions: ["assign_tasks", "review", "broadcast"],
  };

  it("lists peers with their descriptions", () => {
    const result = generateBriefing({
      title: "Tester",
      description: "Tests.",
      tags: ["testing"],
      permissions: ["status"],
      peers: [peer1, peer2],
    });
    expect(result).toContain("**developer**: Developer — Writes code");
    expect(result).toContain("**manager**: Project Manager — Coordinates work");
  });

  it("shows shared tag focus when tags overlap", () => {
    const result = generateBriefing({
      title: "Debugger",
      description: "Debugs.",
      tags: ["debugging", "implementation"],
      permissions: ["status"],
      peers: [peer1],
    });
    // peer1 has implementation and debugging — both overlap
    expect(result).toContain("Shared focus:");
  });

  it("shows assign_tasks hint for peers with that permission", () => {
    const result = generateBriefing({
      title: "Dev",
      description: "Develops.",
      tags: ["implementation"],
      permissions: ["status"],
      peers: [peer2],
    });
    expect(result).toContain("Can assign you work.");
  });

  it("shows review hint for peers with that permission", () => {
    const result = generateBriefing({
      title: "Dev",
      description: "Develops.",
      tags: ["implementation"],
      permissions: ["status"],
      peers: [peer2],
    });
    expect(result).toContain("Can review your output.");
  });

  it("handles no peers", () => {
    const result = generateBriefing({
      title: "Solo",
      description: "Alone.",
      tags: ["analysis"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).toContain("No other roles are currently defined");
  });
});


// =============================================================================
// generateBriefing — Multi-instance coordination
// =============================================================================

describe("generateBriefing multi-instance", () => {
  it("omits multi-instance section when maxInstances is 1", () => {
    const result = generateBriefing({
      title: "Solo",
      description: "Single instance.",
      tags: ["analysis"],
      permissions: ["status"],
      peers: [],
      maxInstances: 1,
    });
    expect(result).not.toContain("Multi-Instance Coordination");
  });

  it("omits multi-instance section when maxInstances is undefined", () => {
    const result = generateBriefing({
      title: "Solo",
      description: "No instances specified.",
      tags: ["analysis"],
      permissions: ["status"],
      peers: [],
    });
    expect(result).not.toContain("Multi-Instance Coordination");
  });

  it("includes multi-instance section when maxInstances > 1", () => {
    const result = generateBriefing({
      title: "Dev",
      description: "Multi dev.",
      tags: ["implementation"],
      permissions: ["status"],
      peers: [],
      maxInstances: 3,
    });
    expect(result).toContain("## 5. Multi-Instance Coordination");
    expect(result).toContain("ALWAYS check `project_claims`");
    expect(result).toContain("NEVER work on the same file as another instance");
  });

  it("shifts section numbers when multi-instance is included", () => {
    const result = generateBriefing({
      title: "Dev",
      description: "Multi dev.",
      tags: ["implementation"],
      permissions: ["status"],
      peers: [],
      maxInstances: 2,
    });
    // With multi-instance: sections are 1-7 instead of 1-6
    expect(result).toContain("## 6. Action Boundary");
    expect(result).toContain("## 7. Onboarding");
  });
});


// =============================================================================
// generateBriefing — Action boundary
// =============================================================================

describe("generateBriefing action boundary", () => {
  it("maps permissions to action descriptions", () => {
    const result = generateBriefing({
      title: "Manager",
      description: "Manages.",
      tags: ["coordination"],
      permissions: ["broadcast", "assign_tasks", "review"],
      peers: [],
    });
    expect(result).toContain("send messages to all team members simultaneously");
    expect(result).toContain("assign tasks to other team members");
    expect(result).toContain("review and approve/reject others' work");
  });

  it("provides fallback for empty permissions", () => {
    const result = generateBriefing({
      title: "Minimal",
      description: "Minimal role.",
      tags: ["analysis"],
      permissions: [],
      peers: [],
    });
    expect(result).toContain("send directed messages to specific roles");
  });
});


// =============================================================================
// generateBriefing — Integration: full template round-trip
// =============================================================================

describe("generateBriefing template round-trip", () => {
  it("generates valid briefing for every ROLE_TEMPLATE", () => {
    for (const template of ROLE_TEMPLATES) {
      const input: BriefingInput = {
        title: template.title,
        description: template.description,
        tags: template.tags,
        permissions: template.permissions,
        peers: [],
        maxInstances: template.maxInstances,
      };
      const result = generateBriefing(input);

      // Every template should produce a valid briefing with all sections
      expect(result).toContain(`# ${template.title}`);
      expect(result).toContain("## 1. Identity");
      expect(result).toContain("## 2. Primary Function");
      expect(result).toContain("## 3. Anti-patterns");
      expect(result).toContain("## 4. Peer Relationships");
      expect(result.length).toBeGreaterThan(200); // Meaningful content
    }
  });
});
