import { describe, expect, it } from "vitest";
import { agentTally, diskTone, tightestVolume } from "./WorkstationGroups";

const GB = 1024 ** 3;

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
      { root: "C:\\", totalBytes: 500 * GB, freeBytes: 20 * GB, fixed: true },
      // 90% used, yet 400 GB left: nothing to worry about.
      { root: "K:\\", totalBytes: 4000 * GB, freeBytes: 400 * GB, fixed: true },
      // Removable drives are somebody else's problem.
      { root: "E:\\", totalBytes: 100 * GB, freeBytes: 1 * GB, fixed: false },
    ]);
    expect(tight?.root).toBe("C:");
    expect(tight?.tone).toBe("warn");
    expect(tightestVolume([])).toBeNull();
  });

  it("judges a disk by the room it has left", () => {
    expect(diskTone(400 * GB, 0.9)).toBe("ok");
    expect(diskTone(50 * GB, 0.96)).toBe("warn");
    expect(diskTone(20 * GB, 0.5)).toBe("warn");
    expect(diskTone(5 * GB, 0.5)).toBe("bad");
  });
});
