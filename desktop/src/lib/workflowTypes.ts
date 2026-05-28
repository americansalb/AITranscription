/**
 * Workflow type display map — full / quick / bugfix workflows shown on the
 * Workflow Type picker + project header chip. Extracted from CollabTab.tsx
 * so the display mapping has a single source of truth + can be unit-tested.
 */

export interface WorkflowTypeDef {
  label: string;
  color: string;
  desc: string;
}

export const WORKFLOW_TYPES: Record<string, WorkflowTypeDef> = {
  full: { label: "Full Review", color: "#9b59b6", desc: "Complete onboarding + planning + full review pipeline" },
  quick: { label: "Quick Feature", color: "#17bf63", desc: "Skip onboarding, abbreviated review cycle" },
  bugfix: { label: "Bug Fix", color: "#f5a623", desc: "Focused diagnosis and fix, minimal review" },
};

const DEFAULT_LABEL = "No Workflow";
const DEFAULT_COLOR = "#657786";

export function getWorkflowDisplay(
  type?: string,
  customColors?: Record<string, string>,
): { label: string; color: string } {
  if (type && WORKFLOW_TYPES[type]) {
    const color = customColors?.[type] || WORKFLOW_TYPES[type].color;
    return { label: WORKFLOW_TYPES[type].label, color };
  }
  return { label: DEFAULT_LABEL, color: DEFAULT_COLOR };
}
