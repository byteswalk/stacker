import { describe, expect, it } from "vitest";
import {
  LAST_DIRECTORIES_KEY,
  LAST_DRIVES_KEY,
  applyRememberScanTargetsPreference,
  loadRememberedTargets,
  markTargetAvailability,
  rememberStartedScan,
} from "./targetStore";

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();
  get length() { return this.values.size; }
  clear() { this.values.clear(); }
  getItem(key: string) { return this.values.get(key) ?? null; }
  key(index: number) { return [...this.values.keys()][index] ?? null; }
  removeItem(key: string) { this.values.delete(key); }
  setItem(key: string, value: string) { this.values.set(key, String(value)); }
}

describe("remembered space-analysis targets", () => {
  it("treats invalid JSON as an empty target list", () => {
    const storage = new MemoryStorage();
    storage.setItem(LAST_DIRECTORIES_KEY, "not-json");

    expect(loadRememberedTargets("directories", storage)).toEqual([]);
  });

  it("retains unavailable targets and marks them invalid", () => {
    const remembered = ["C:\\", "Z:\\"];

    expect(markTargetAvailability(remembered, ["C:\\"])).toEqual([
      { target: "C:\\", valid: true },
      { target: "Z:\\", valid: false },
    ]);
  });

  it("does not mutate storage when a selector loads remembered targets", () => {
    const storage = new MemoryStorage();
    storage.setItem(LAST_DRIVES_KEY, JSON.stringify(["D:\\"]));
    const before = storage.getItem(LAST_DRIVES_KEY);

    expect(loadRememberedTargets("drives", storage)).toEqual(["D:\\"]);
    expect(storage.getItem(LAST_DRIVES_KEY)).toBe(before);
  });

  it("persists targets only through the scan-start action", () => {
    const storage = new MemoryStorage();

    loadRememberedTargets("directories", storage);
    expect(storage.getItem(LAST_DIRECTORIES_KEY)).toBeNull();

    rememberStartedScan("directories", ["C:\\work", "C:\\work", "D:\\src"], true, storage);
    expect(JSON.parse(storage.getItem(LAST_DIRECTORIES_KEY) ?? "[]")).toEqual([
      "C:\\work",
      "D:\\src",
    ]);
  });

  it("clears directory and drive targets immediately when remembering is disabled", () => {
    const storage = new MemoryStorage();
    storage.setItem(LAST_DIRECTORIES_KEY, JSON.stringify(["C:\\work"]));
    storage.setItem(LAST_DRIVES_KEY, JSON.stringify(["D:\\"]));

    applyRememberScanTargetsPreference(false, storage);

    expect(storage.getItem(LAST_DIRECTORIES_KEY)).toBeNull();
    expect(storage.getItem(LAST_DRIVES_KEY)).toBeNull();
  });
});
