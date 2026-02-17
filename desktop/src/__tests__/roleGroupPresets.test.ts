/**
 * Tests for roleGroupPresets — built-in team template data.
 *
 * Covers:
 *   - BUILTIN_ROLE_GROUPS: correct count, structure, required fields
 *   - Each preset: slug uniqueness, non-empty roles, valid instance counts
 *   - builtin flag on all presets
 *   - Specific preset verification (coding-team, research-team, etc.)
 */
import { describe, it, expect } from "vitest";
import { BUILTIN_ROLE_GROUPS } from "../utils/roleGroupPresets";


// =============================================================================
// ARRAY-LEVEL VALIDATION
// =============================================================================

describe("BUILTIN_ROLE_GROUPS — structure", () => {
  it("exports exactly 5 presets", () => {
    expect(BUILTIN_ROLE_GROUPS).toHaveLength(5);
  });

  it("all slugs are unique", () => {
    const slugs = BUILTIN_ROLE_GROUPS.map(g => g.slug);
    expect(new Set(slugs).size).toBe(slugs.length);
  });

  it("every preset has required fields", () => {
    for (const group of BUILTIN_ROLE_GROUPS) {
      expect(typeof group.slug).toBe("string");
      expect(group.slug.length).toBeGreaterThan(0);
      expect(typeof group.name).toBe("string");
      expect(group.name.length).toBeGreaterThan(0);
      expect(typeof group.icon).toBe("string");
      expect(group.icon.length).toBeGreaterThan(0);
      expect(typeof group.description).toBe("string");
      expect(group.description.length).toBeGreaterThan(0);
      expect(group.builtin).toBe(true);
      expect(Array.isArray(group.roles)).toBe(true);
      expect(group.roles.length).toBeGreaterThan(0);
    }
  });

  it("every role entry has slug and instances", () => {
    for (const group of BUILTIN_ROLE_GROUPS) {
      for (const role of group.roles) {
        expect(typeof role.slug).toBe("string");
        expect(role.slug.length).toBeGreaterThan(0);
        expect(typeof role.instances).toBe("number");
        expect(role.instances).toBeGreaterThanOrEqual(1);
      }
    }
  });

  it("all presets are marked builtin", () => {
    for (const group of BUILTIN_ROLE_GROUPS) {
      expect(group.builtin).toBe(true);
    }
  });
});


// =============================================================================
// INDIVIDUAL PRESET VERIFICATION
// =============================================================================

describe("coding-team preset", () => {
  const preset = BUILTIN_ROLE_GROUPS.find(g => g.slug === "coding-team");

  it("exists", () => {
    expect(preset).toBeDefined();
  });

  it("has correct name and icon", () => {
    expect(preset!.name).toBe("Coding Team");
  });

  it("includes manager, architect, developer, tester, ux-engineer", () => {
    const roleSlugs = preset!.roles.map(r => r.slug);
    expect(roleSlugs).toContain("manager");
    expect(roleSlugs).toContain("architect");
    expect(roleSlugs).toContain("developer");
    expect(roleSlugs).toContain("tester");
    expect(roleSlugs).toContain("ux-engineer");
  });

  it("has 2 developer instances", () => {
    const dev = preset!.roles.find(r => r.slug === "developer");
    expect(dev!.instances).toBe(2);
  });

  it("has 1 manager instance", () => {
    const mgr = preset!.roles.find(r => r.slug === "manager");
    expect(mgr!.instances).toBe(1);
  });
});


describe("research-team preset", () => {
  const preset = BUILTIN_ROLE_GROUPS.find(g => g.slug === "research-team");

  it("exists", () => {
    expect(preset).toBeDefined();
  });

  it("has 3 researcher instances", () => {
    const res = preset!.roles.find(r => r.slug === "researcher");
    expect(res!.instances).toBe(3);
  });

  it("includes domain-expert and tech-writer", () => {
    const roleSlugs = preset!.roles.map(r => r.slug);
    expect(roleSlugs).toContain("domain-expert");
    expect(roleSlugs).toContain("tech-writer");
  });
});


describe("security-audit preset", () => {
  const preset = BUILTIN_ROLE_GROUPS.find(g => g.slug === "security-audit");

  it("exists", () => {
    expect(preset).toBeDefined();
  });

  it("has 2 security-auditor instances", () => {
    const auditor = preset!.roles.find(r => r.slug === "security-auditor");
    expect(auditor!.instances).toBe(2);
  });

  it("includes qa-lead", () => {
    const roleSlugs = preset!.roles.map(r => r.slug);
    expect(roleSlugs).toContain("qa-lead");
  });
});


describe("sprint-team preset", () => {
  const preset = BUILTIN_ROLE_GROUPS.find(g => g.slug === "sprint-team");

  it("exists", () => {
    expect(preset).toBeDefined();
  });

  it("has sprint-lead instead of manager", () => {
    const roleSlugs = preset!.roles.map(r => r.slug);
    expect(roleSlugs).toContain("sprint-lead");
    expect(roleSlugs).not.toContain("manager");
  });

  it("has 3 developer instances", () => {
    const dev = preset!.roles.find(r => r.slug === "developer");
    expect(dev!.instances).toBe(3);
  });
});


describe("legal-analysis preset", () => {
  const preset = BUILTIN_ROLE_GROUPS.find(g => g.slug === "legal-analysis");

  it("exists", () => {
    expect(preset).toBeDefined();
  });

  it("includes moderator and domain-expert", () => {
    const roleSlugs = preset!.roles.map(r => r.slug);
    expect(roleSlugs).toContain("moderator");
    expect(roleSlugs).toContain("domain-expert");
  });

  it("has 2 researcher instances", () => {
    const res = preset!.roles.find(r => r.slug === "researcher");
    expect(res!.instances).toBe(2);
  });
});
