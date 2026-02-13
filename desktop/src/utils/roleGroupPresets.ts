import type { RoleGroup } from "../lib/collabTypes";

/** Pre-built role groups that ship with the app. These are read-only. */
export const BUILTIN_ROLE_GROUPS: RoleGroup[] = [
  {
    slug: "coding-team",
    name: "Coding Team",
    icon: "\uD83D\uDCBB",
    description: "Full development pipeline with review cycle",
    builtin: true,
    roles: [
      { slug: "manager", instances: 1 },
      { slug: "architect", instances: 1 },
      { slug: "developer", instances: 2 },
      { slug: "tester", instances: 1 },
      { slug: "ux-engineer", instances: 1 },
    ],
  },
  {
    slug: "research-team",
    name: "Research Team",
    icon: "\uD83D\uDD2C",
    description: "Deep-dive research with multi-angle analysis",
    builtin: true,
    roles: [
      { slug: "manager", instances: 1 },
      { slug: "researcher", instances: 3 },
      { slug: "domain-expert", instances: 1 },
      { slug: "tech-writer", instances: 1 },
    ],
  },
  {
    slug: "security-audit",
    name: "Security Audit",
    icon: "\uD83D\uDD12",
    description: "Thorough security review with multiple auditors",
    builtin: true,
    roles: [
      { slug: "manager", instances: 1 },
      { slug: "security-auditor", instances: 2 },
      { slug: "developer", instances: 1 },
      { slug: "qa-lead", instances: 1 },
    ],
  },
  {
    slug: "sprint-team",
    name: "Sprint Team",
    icon: "\uD83C\uDFC3",
    description: "Agile sprint execution with dedicated lead",
    builtin: true,
    roles: [
      { slug: "sprint-lead", instances: 1 },
      { slug: "developer", instances: 3 },
      { slug: "tester", instances: 1 },
      { slug: "ux-engineer", instances: 1 },
    ],
  },
  {
    slug: "legal-analysis",
    name: "Legal Analysis",
    icon: "\u2696\uFE0F",
    description: "Multi-perspective legal review with moderation",
    builtin: true,
    roles: [
      { slug: "manager", instances: 1 },
      { slug: "moderator", instances: 1 },
      { slug: "researcher", instances: 2 },
      { slug: "domain-expert", instances: 1 },
    ],
  },
];
