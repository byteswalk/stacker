import { describe, expect, it } from "vitest";
import type { LargeFileRow } from "../types";
import { mergeLargeFilePage } from "./LargeFiles";

function file(nodeId: string): LargeFileRow {
  return {
    nodeId,
    name: `${nodeId}.bin`,
    path: `C:\\${nodeId}.bin`,
    allocatedBytes: 1024,
    logicalBytes: 2048,
    modifiedAt: null,
  };
}

describe("mergeLargeFilePage", () => {
  it("keeps backend page order and removes duplicate rows", () => {
    const result = mergeLargeFilePage(
      { items: [file("a")], total: 3, nextOffset: 1 },
      { items: [file("a"), file("b")], offset: 1, limit: 100, total: 3 },
    );

    expect(result.items.map((item) => item.nodeId)).toEqual(["a", "b"]);
    expect(result.nextOffset).toBe(3);
  });
});
