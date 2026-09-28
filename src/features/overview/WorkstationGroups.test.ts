import { describe, expect, it } from "vitest";
import { agentTally, tightestVolume } from "./WorkstationGroups";

describe("workstation summary", () => {
  it("counts an agent once however many surfaces it has, and updates one by one", () => {
    const tally = agentTally([
      { id: "claude", cli: { installed: true, updateAvailable: true }, desktop: { installed: true, updateAvailable: false } },
      { id: "codex", cli: { installed: true, updateAvailable: false }, desktop: null },
      // Installed nowhere: counted in the total, not in what is installed.
      { id: "pi", cli: { installed: false }, desktop: { installed: false } },
    ]);
    expect(tally).toEqual({ installed: 2, total: 3, updates: 1 });
  });

  it("names the disk with the least room left, not the biggest one", () => {
    const tight = tightestVolume([
      { root: "C:\\", totalBytes: 500, freeBytes: 50, fixed: true },
      { root: "D:\\", totalBytes: 4000, freeBytes: 2000, fixed: true },
      // Removable drives are somebody else's problem.
      { root: "E:\\", totalBytes: 100, freeBytes: 1, fixed: false },
    ]);
    expect(tight?.root).toBe("C:");
    expect(tight?.ratio).toBeCloseTo(0.9);
    expect(tightestVolume([])).toBeNull();
  });
});
