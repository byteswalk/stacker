import { describe, expect, it } from "vitest";
import type { DirectoryNode } from "../types";
import { filterCleanupNodes } from "./DevelopmentArtifacts";

const nodes: DirectoryNode[] = [
  {
    nodeId: "node-modules",
    parentId: null,
    name: "node_modules",
    path: "D:\\Projects\\portal\\node_modules",
    allocatedBytes: 1024,
    logicalBytes: 1024,
    childCount: 0,
    safety: "rebuildable",
    projectId: "portal",
    impactKey: "spaceAnalysis.impact.nodeDependencies",
    cleanupKind: "nodeDependencies",
  },
  {
    nodeId: "cargo-target",
    parentId: null,
    name: "target",
    path: "C:\\Work\\desktop\\target",
    allocatedBytes: 2048,
    logicalBytes: 2048,
    childCount: 0,
    safety: "rebuildable",
    projectId: "desktop",
    impactKey: "spaceAnalysis.impact.rustBuildOutput",
    cleanupKind: "rustBuildOutput",
  },
];

describe("filterCleanupNodes", () => {
  it("matches directory names and full paths without case sensitivity", () => {
    expect(filterCleanupNodes(nodes, "NODE_MODULES")).toEqual([nodes[0]]);
    expect(filterCleanupNodes(nodes, "c:\\work")).toEqual([nodes[1]]);
  });

  it("returns every candidate for a blank query", () => {
    expect(filterCleanupNodes(nodes, "   ")).toBe(nodes);
  });
});
