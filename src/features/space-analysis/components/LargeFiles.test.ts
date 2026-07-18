import { describe, expect, it } from "vitest";
import type { LargeFileRow } from "../types";
import { mergeLargeFilePage, sameLargeFileRequest } from "./LargeFiles";

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

describe("sameLargeFileRequest", () => {
  const current = { taskId: "scan-1", thresholdBytes: 1024, generation: 4 };

  it("rejects stale results when the task, threshold, or generation changes", () => {
    expect(sameLargeFileRequest(current, current)).toBe(true);
    expect(sameLargeFileRequest(current, { ...current, taskId: "scan-2" })).toBe(false);
    expect(sameLargeFileRequest(current, { ...current, thresholdBytes: 2048 })).toBe(false);
    expect(sameLargeFileRequest(current, { ...current, generation: 3 })).toBe(false);
  });
});
