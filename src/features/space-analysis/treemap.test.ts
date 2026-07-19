import { describe, expect, it } from "vitest";
import { intersectionArea, layoutTreemap } from "./treemap";

describe("layoutTreemap", () => {
  it("keeps every rectangle inside the container without overlap", () => {
    const rows = layoutTreemap([
      { id: "a", value: 60 },
      { id: "b", value: 40 },
      { id: "c", value: 25 },
      { id: "d", value: 10 },
    ], 800, 400);

    expect(rows).toHaveLength(4);
    expect(rows.every((row) => (
      row.x >= 0
      && row.y >= 0
      && row.x + row.width <= 800
      && row.y + row.height <= 400
    ))).toBe(true);
    for (let left = 0; left < rows.length; left += 1) {
      for (let right = left + 1; right < rows.length; right += 1) {
        expect(intersectionArea(rows[left], rows[right])).toBe(0);
      }
    }
  });

  it("ignores zero and invalid values while preserving stable descending order", () => {
    const rows = layoutTreemap([
      { id: "zero", value: 0 },
      { id: "second", value: 8 },
      { id: "first", value: 8 },
      { id: "largest", value: 12 },
      { id: "negative", value: -3 },
      { id: "nan", value: Number.NaN },
    ], 300, 200);

    expect(rows.map((row) => row.id)).toEqual(["largest", "second", "first"]);
  });

  it("can preserve input order for top-level scan roots", () => {
    const rows = layoutTreemap([
      { id: "C:\\", value: 20 },
      { id: "D:\\", value: 80 },
    ], 300, 200, "input");

    expect(rows.map((row) => row.id)).toEqual(["C:\\", "D:\\"]);
  });

  it("is deterministic and accounts for the full container area", () => {
    const input = Array.from({ length: 30 }, (_, index) => ({
      id: `node-${index}`,
      value: (index % 7) + 1,
    }));
    const first = layoutTreemap(input, 997, 431);
    const second = layoutTreemap(input, 997, 431);

    expect(second).toEqual(first);
    const area = first.reduce((sum, row) => sum + row.width * row.height, 0);
    expect(area).toBeCloseTo(997 * 431, 6);
  });

  it("returns no rectangles for an empty or unusable container", () => {
    expect(layoutTreemap([{ id: "a", value: 1 }], 0, 100)).toEqual([]);
    expect(layoutTreemap([{ id: "a", value: 0 }], 100, 100)).toEqual([]);
  });
});
