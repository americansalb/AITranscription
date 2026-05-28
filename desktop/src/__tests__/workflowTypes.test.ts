/**
 * Tests for workflowTypes — WORKFLOW_TYPES map + getWorkflowDisplay.
 */
import { describe, it, expect } from "vitest";

import { WORKFLOW_TYPES, getWorkflowDisplay } from "../lib/workflowTypes";

describe("WORKFLOW_TYPES map", () => {
  it("contains full / quick / bugfix entries with label + color + desc", () => {
    expect(WORKFLOW_TYPES.full).toEqual({
      label: "Full Review",
      color: "#9b59b6",
      desc: "Complete onboarding + planning + full review pipeline",
    });
    expect(WORKFLOW_TYPES.quick).toEqual({
      label: "Quick Feature",
      color: "#17bf63",
      desc: "Skip onboarding, abbreviated review cycle",
    });
    expect(WORKFLOW_TYPES.bugfix).toEqual({
      label: "Bug Fix",
      color: "#f5a623",
      desc: "Focused diagnosis and fix, minimal review",
    });
  });
});

describe("getWorkflowDisplay", () => {
  it("returns the canonical mapping when type is in WORKFLOW_TYPES", () => {
    expect(getWorkflowDisplay("full")).toEqual({ label: "Full Review", color: "#9b59b6" });
    expect(getWorkflowDisplay("quick")).toEqual({ label: "Quick Feature", color: "#17bf63" });
    expect(getWorkflowDisplay("bugfix")).toEqual({ label: "Bug Fix", color: "#f5a623" });
  });

  it("overrides color when customColors provides one for the matched type", () => {
    expect(getWorkflowDisplay("full", { full: "#000000" })).toEqual({
      label: "Full Review",
      color: "#000000",
    });
  });

  it("ignores customColors that don't match the resolved type", () => {
    expect(getWorkflowDisplay("full", { quick: "#000000" })).toEqual({
      label: "Full Review",
      color: "#9b59b6",
    });
  });

  it("returns the 'No Workflow' fallback when type is undefined", () => {
    expect(getWorkflowDisplay()).toEqual({ label: "No Workflow", color: "#657786" });
  });

  it("returns the 'No Workflow' fallback when type is unknown", () => {
    expect(getWorkflowDisplay("custom_workflow")).toEqual({ label: "No Workflow", color: "#657786" });
  });

  it("returns the 'No Workflow' fallback for empty-string type", () => {
    expect(getWorkflowDisplay("")).toEqual({ label: "No Workflow", color: "#657786" });
  });

  it("customColors only affects RESOLVED types, not the fallback path", () => {
    expect(getWorkflowDisplay("custom_workflow", { custom_workflow: "#abcdef" })).toEqual({
      label: "No Workflow",
      color: "#657786",
    });
  });
});
