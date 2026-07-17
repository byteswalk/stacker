import { describe, expect, it } from "vitest";
import { collectFrontendSettings, restoreFrontendSettings } from "./frontendSettings";

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();
  get length() { return this.values.size; }
  clear() { this.values.clear(); }
  getItem(key: string) { return this.values.get(key) ?? null; }
  key(index: number) { return [...this.values.keys()][index] ?? null; }
  removeItem(key: string) { this.values.delete(key); }
  setItem(key: string, value: string) { this.values.set(key, String(value)); }
}

describe("frontend settings migration", () => {
  it("exports and restores only Stacker preferences", () => {
    const source = new MemoryStorage();
    source.setItem("stacker.node.downloadSource", "official");
    source.setItem("unrelated", "ignored");

    const exported = collectFrontendSettings(source);
    expect(exported).toEqual({ "stacker.node.downloadSource": "official" });

    const target = new MemoryStorage();
    target.setItem("stacker.legacy", "remove-me");
    restoreFrontendSettings({ ...exported, unrelated: "ignored" }, target);
    expect(target.getItem("stacker.node.downloadSource")).toBe("official");
    expect(target.getItem("stacker.legacy")).toBeNull();
    expect(target.getItem("unrelated")).toBeNull();
  });
});
