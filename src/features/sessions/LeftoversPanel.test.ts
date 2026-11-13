import { describe, expect, it } from "vitest";
import { covered, defaultLeftovers, parentOf, sizeOf } from "./LeftoversPanel";
import type { LeftoverItem, LeftoverReport } from "./types";

const item = (id: string, bytes: number, recommended: boolean, blocked: string | null = null): LeftoverItem =>
  ({ id, kind: "build", label: id, explain: "", path: id, bytes, files: 1, recommended, blocked });

describe("leftovers selection", () => {
  const report = {
    projects: [
      { items: [item("a", 100, true), item("b", 50, false), item("c", 70, true, "有程序正在运行：app.exe")] },
      { items: [item("d", 30, true)] },
    ],
  } as unknown as LeftoverReport;

  it("ticks what is recommended and not blocked", () => {
    expect(defaultLeftovers(report)).toEqual(["a", "d"]);
  });

  it("adds up only what is ticked", () => {
    const all = report.projects.flatMap((p) => p.items);
    expect(sizeOf(all, new Set(["a", "b"]))).toBe(150);
    expect(sizeOf(all, new Set())).toBe(0);
  });
});

describe("parts of a build folder", () => {
  it("count once when the whole folder is ticked", () => {
    expect(parentOf("p|Incremental|target:incremental")).toBe("p|Build|target");
    expect(parentOf("p|Stale|target:stale")).toBe("p|Build|target");
    expect(parentOf("p|Build|target")).toBeNull();
    const items = [item("p|Build|target", 100, false), item("p|Incremental|target:incremental", 40, false)];
    expect(sizeOf(items, new Set(["p|Build|target", "p|Incremental|target:incremental"]))).toBe(100);
    expect(sizeOf(items, new Set(["p|Incremental|target:incremental"]))).toBe(40);
    expect(covered("p|Incremental|target:incremental", new Set(["p|Build|target"]))).toBe(true);
  });
});
