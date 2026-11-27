import { describe, expect, it } from "vitest";
import { clampThreshold, driveName, freePercent, itemLabel, lastRunOf, reasonLabel, stillLow, type LowSpaceRun } from "./lowSpace";

const run = (drive: string, finishedAt: number, freed: number) =>
  ({ drive, finishedAt, freed, at: finishedAt, total: 100, freeBefore: 1, freeAfter: 1, manual: false, steps: [], notes: [] }) as LowSpaceRun;

describe("how each low disk fared", () => {
  it("finds a disk's newest run whatever the root is written like", () => {
    const runs = [run("C:\\", 30, 0), run("I:\\", 20, 0), run("c:\\", 10, 5)];
    expect(lastRunOf(runs, "C:\\")?.finishedAt).toBe(30);
    expect(lastRunOf(runs, "i:")?.finishedAt).toBe(20);
    expect(lastRunOf(runs, "K:\\")).toBeUndefined();
  });

  it("shows a nearly full disk's share with one decimal", () => {
    expect(freePercent({ root: "C:\\", total: 500, free: 2.6 })).toBe("0.5");
    expect(freePercent({ root: "X:\\", total: 0, free: 0 })).toBe("0");
  });
});

describe("low-space cleanup on the page", () => {
  it("keeps the threshold a whole percent from 1 to 50", () => {
    expect(clampThreshold(5)).toBe(5);
    expect(clampThreshold(0)).toBe(1);
    expect(clampThreshold(80)).toBe(50);
    expect(clampThreshold(7.6)).toBe(8);
    expect(clampThreshold(Number("abc"))).toBe(5);
  });

  it("says whether a run left the disk low", () => {
    expect(stillLow({ freeAfter: 5, total: 100 }, 5)).toBe(true);
    expect(stillLow({ freeAfter: 6, total: 100 }, 5)).toBe(false);
    expect(stillLow({ freeAfter: 0, total: 0 }, 5)).toBe(false);
  });

  it("names disks, caches and reasons the way the page does", () => {
    expect(driveName("D:\\")).toBe("D:");
    expect(driveName("/")).toBe("/");
    expect(driveName("/Volumes/Data")).toBe("/Volumes/Data");
    expect(itemLabel("spaceAnalysis.known.npm")).toBe("npm 缓存");
    expect(itemLabel("构建产物 target")).toBe("构建产物 target");
    expect(reasonLabel("spaceAnalysis.cleanup.reason.accessDenied")).toBe("拒绝访问");
  });
});
