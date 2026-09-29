import { describe, expect, it } from "vitest";
import { layout } from "./SpaceMap";
import type { DirectoryNode } from "../types";

const node = (name: string, bytes: number): DirectoryNode => ({
  nodeId: name, parentId: null, name, path: `C:\\${name}`,
  allocatedBytes: bytes, logicalBytes: bytes, childCount: 0,
  safety: "keep", projectId: null, impactKey: null, cleanupKind: null,
});

describe("space map layout", () => {
  it("gives each folder the share of the canvas its size deserves", () => {
    const tiles = layout([node("big", 600), node("small", 400)], 100, 100);
    const area = (name: string) => {
      const tile = tiles.find((t) => t.node.name === name)!;
      return tile.width * tile.height;
    };
    expect(area("big")).toBeCloseTo(6000, 0);
    expect(area("small")).toBeCloseTo(4000, 0);
    // Every tile stays inside the canvas.
    for (const tile of tiles) {
      expect(tile.x).toBeGreaterThanOrEqual(0);
      expect(tile.y).toBeGreaterThanOrEqual(0);
      expect(tile.x + tile.width).toBeLessThanOrEqual(100.01);
      expect(tile.y + tile.height).toBeLessThanOrEqual(100.01);
    }
  });

  it("keeps tiles closer to square than a plain row would", () => {
    const sizes = [50, 30, 12, 5, 3];
    const tiles = layout(sizes.map((size, i) => node(`n${i}`, size)), 600, 400);
    const ratios = tiles.map((tile) => Math.max(tile.width / tile.height, tile.height / tile.width));
    // A single row of five tiles across 600×400 would be 5:1 or worse.
    expect(Math.max(...ratios)).toBeLessThan(5);
  });

  it("has nothing to draw for empty folders or an empty canvas", () => {
    expect(layout([node("empty", 0)], 100, 100)).toEqual([]);
    expect(layout([node("a", 10)], 0, 100)).toEqual([]);
    expect(layout([], 100, 100)).toEqual([]);
  });
});
