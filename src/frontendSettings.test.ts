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
  it("carries the environment's preferences and nothing that is this computer's own", () => {
    const source = new MemoryStorage();
    source.setItem("stacker.node.downloadSource", "official");
    source.setItem("stacker.python.install.onlyStable", "true");
    source.setItem("stacker.maven.customSettingsXml", "D:/m2/settings.xml");
    source.setItem("stacker.aiAnswers.v1", "{}");
    source.setItem("stacker.vault.columns.v1", "{}");
    source.setItem("unrelated", "ignored");

    const exported = collectFrontendSettings(source);
    expect(Object.keys(exported).sort()).toEqual(["stacker.maven.customSettingsXml", "stacker.node.downloadSource", "stacker.python.install.onlyStable"]);

    const target = new MemoryStorage();
    target.setItem("stacker.go.downloadSource", "goproxy");
    target.setItem("stacker.aiAnswers.v1", "kept");
    target.setItem("stacker.vault.columns.v1", "kept");
    restoreFrontendSettings({ ...exported, "stacker.vibe.status.v3": "from an old profile", unrelated: "ignored" }, target);
    expect(target.getItem("stacker.node.downloadSource")).toBe("official");
    expect(target.getItem("stacker.go.downloadSource")).toBeNull();
    expect(target.getItem("stacker.aiAnswers.v1")).toBe("kept");
    expect(target.getItem("stacker.vault.columns.v1")).toBe("kept");
    expect(target.getItem("stacker.vibe.status.v3")).toBeNull();
    expect(target.getItem("unrelated")).toBeNull();
  });
});
