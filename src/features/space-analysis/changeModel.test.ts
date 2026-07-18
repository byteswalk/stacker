import { describe, expect, it } from "vitest";
import { latestComparablePair, signedBytes } from "./changeModel";
import type { SnapshotMetadata } from "./types";

const item = (id: string, targetFingerprint: string, createdAt: string): SnapshotMetadata => ({ id, targetFingerprint, createdAt, targets: [], allocatedBytes: 0, directoryCount: 0 });

describe("snapshot change model", () => {
  it("selects the latest two compatible snapshots", () => {
    const pair = latestComparablePair([item("old", "a", "2026-01-01"), item("other", "b", "2026-03-01"), item("new", "a", "2026-02-01")], "a");
    expect(pair?.current.id).toBe("new"); expect(pair?.base.id).toBe("old");
  });
  it("formats signed deltas", () => {
    expect(signedBytes(12, String)).toBe("+12"); expect(signedBytes(-4, String)).toBe("-4");
  });
});
