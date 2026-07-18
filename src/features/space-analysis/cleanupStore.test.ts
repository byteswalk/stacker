import { describe, expect, it } from "vitest";
import { canSelectSafety, defaultSelectedNodeIds } from "./cleanupStore";
import type { DirectoryNode } from "./types";

function node(nodeId: string, safety: string): DirectoryNode {
  return { nodeId, parentId: null, name: nodeId, path: nodeId, allocatedBytes: 1, logicalBytes: 1, childCount: 0, safety, projectId: null, impactKey: null, cleanupKind: null };
}

describe("cleanup selection", () => {
  it("preselects only safe candidates", () => {
    expect([...defaultSelectedNodeIds([
      node("safe", "safe"), node("build", "rebuildable"), node("confirm", "needsConfirmation"), node("view", "viewOnly"),
    ])]).toEqual(["safe"]);
  });

  it("keeps view-only rows disabled", () => {
    expect(canSelectSafety("viewOnly")).toBe(false);
    expect(canSelectSafety("safe")).toBe(true);
    expect(canSelectSafety("rebuildable")).toBe(true);
    expect(canSelectSafety("needsConfirmation")).toBe(true);
  });
});
