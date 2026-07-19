import { describe, expect, it } from "vitest";
import { canSelectSafety, compactCleanupSelection, defaultSelectedNodeIds, selectionWithNodes } from "./cleanupStore";
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

  it("selects or clears only selectable nodes in the requested category", () => {
    const category = [node("one", "safe"), node("two", "rebuildable"), node("view", "viewOnly")];
    const selected = selectionWithNodes(new Set(["outside"]), category, true);
    expect([...selected]).toEqual(["outside", "one", "two"]);
    expect([...selectionWithNodes(selected, category, false)]).toEqual(["outside"]);
  });
});

describe("cleanup preparation", () => {
  it("keeps only the parent when selected cleanup paths overlap", () => {
    const parent = { ...node("parent", "rebuildable"), path: String.raw`D:\project\target` };
    const child = { ...node("child", "rebuildable"), path: String.raw`D:\project\target\debug` };
    const sibling = { ...node("sibling", "safe"), path: String.raw`D:\project\node_modules` };
    expect(compactCleanupSelection([child, sibling, parent], new Set(["child", "sibling", "parent"])))
      .toEqual(["parent", "sibling"]);
  });
});
