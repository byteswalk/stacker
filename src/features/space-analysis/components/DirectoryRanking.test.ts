import { describe, expect, it } from "vitest";
import type { DirectoryNode } from "../types";
import { mergeDirectoryPage } from "./DirectoryRanking";

function node(nodeId: string): DirectoryNode {
  return {
    nodeId,
    parentId: "root",
    name: nodeId,
    path: `C:\\${nodeId}`,
    allocatedBytes: 10,
    logicalBytes: 10,
    childCount: 0,
    safety: "view_only",
  };
}

describe("mergeDirectoryPage", () => {
  it("appends lazy pages without duplicating opaque node ids", () => {
    const first = mergeDirectoryPage(undefined, {
      items: [node("a"), node("b")],
      offset: 0,
      limit: 100,
      total: 3,
    });
    const second = mergeDirectoryPage(first, {
      items: [node("b"), node("c")],
      offset: 2,
      limit: 100,
      total: 3,
    });

    expect(second.items.map((item) => item.nodeId)).toEqual(["a", "b", "c"]);
    expect(second.nextOffset).toBe(4);
    expect(second.total).toBe(3);
  });
});
