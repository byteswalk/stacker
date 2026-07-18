import { describe, expect, it } from "vitest";
import {
  LAST_DIRECTORIES_KEY,
  LAST_DRIVES_KEY,
  disableRememberScanTargets,
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

class ThrowingStorage extends MemoryStorage {
  private readonly method: "get" | "set" | "remove";

  constructor(method: "get" | "set" | "remove") {
    super();
    this.method = method;
  }

  override getItem(key: string) {
    if (this.method === "get") throw new Error("storage read blocked");
    return super.getItem(key);
  }

  override setItem(key: string, value: string) {
    if (this.method === "set") throw new Error("storage write blocked");
    super.setItem(key, value);
  }

  override removeItem(key: string) {
    if (this.method === "remove") throw new Error("storage removal blocked");
    super.removeItem(key);
  }
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

  it("clears directory and drive targets only after the backend save succeeds", async () => {
    const storage = new MemoryStorage();
    storage.setItem(LAST_DIRECTORIES_KEY, JSON.stringify(["C:\\work"]));
    storage.setItem(LAST_DRIVES_KEY, JSON.stringify(["D:\\"]));

    const result = await disableRememberScanTargets(async () => {
      expect(storage.getItem(LAST_DIRECTORIES_KEY)).not.toBeNull();
      expect(storage.getItem(LAST_DRIVES_KEY)).not.toBeNull();
    }, storage);

    expect(result).toEqual({ ok: true });
    expect(storage.getItem(LAST_DIRECTORIES_KEY)).toBeNull();
    expect(storage.getItem(LAST_DRIVES_KEY)).toBeNull();
  });

  it("preserves target data when disabling fails to save backend settings", async () => {
    const storage = new MemoryStorage();
    storage.setItem(LAST_DIRECTORIES_KEY, JSON.stringify(["C:\\work"]));
    storage.setItem(LAST_DRIVES_KEY, JSON.stringify(["D:\\"]));

    const result = await disableRememberScanTargets(async () => {
      throw new Error("backend unavailable");
    }, storage);

    expect(result).toMatchObject({ ok: false, stage: "settings" });
    expect(storage.getItem(LAST_DIRECTORIES_KEY)).toBe(JSON.stringify(["C:\\work"]));
    expect(storage.getItem(LAST_DRIVES_KEY)).toBe(JSON.stringify(["D:\\"]));
  });

  it("returns an empty list when storage reads throw", () => {
    expect(loadRememberedTargets("directories", new ThrowingStorage("get"))).toEqual([]);
  });

  it("returns a controlled failure when storage writes throw", () => {
    const result = rememberStartedScan(
      "directories",
      ["C:\\work"],
      true,
      new ThrowingStorage("set"),
    );

    expect(result).toMatchObject({ ok: false });
  });

  it("returns a controlled failure when storage removals throw", async () => {
    const result = await disableRememberScanTargets(
      async () => undefined,
      new ThrowingStorage("remove"),
    );

    expect(result).toMatchObject({ ok: false, stage: "storage" });
  });
});
