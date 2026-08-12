import { describe, expect, it } from "vitest";
import {
  cleanupTargetsForTrackingRoots,
  normalizeWorkSessionProfile,
  type WorkSessionTrackingRoot,
} from "./managedWorkSessionStore";

const root = (partial: Partial<WorkSessionTrackingRoot>): WorkSessionTrackingRoot => ({
  id: "root",
  label: "Root",
  path: "C:\\root",
  category: "cache",
  safety: "rebuildable",
  defaultEnabled: true,
  reason: "test",
  ...partial,
});

describe("managed work session cleanup handoff", () => {
  it("excludes project and protected agent data", () => {
    expect(cleanupTargetsForTrackingRoots([
      root({ id: "project", category: "project", safety: "protected", path: "D:\\work" }),
      root({ id: "agent", category: "agent-data", safety: "protected", path: "C:\\agent" }),
      root({ id: "npm", path: "C:\\npm-cache" }),
      root({ id: "maven", safety: "review", path: "C:\\m2" }),
    ])).toEqual(["C:\\npm-cache", "C:\\m2"]);
  });

  it("deduplicates cache paths before opening cleanup review", () => {
    expect(cleanupTargetsForTrackingRoots([
      root({ id: "one", path: "C:\\cache" }),
      root({ id: "two", path: "c:/CACHE" }),
    ])).toEqual(["C:\\cache"]);
  });
});

describe("managed work session profile migration", () => {
  it("preserves desktop session settings from the current schema", () => {
    expect(normalizeWorkSessionProfile({
      workspace: "D:\\work",
      agentId: "codex",
      mode: "desktop",
      desktopAction: "attach",
      desktopPid: 42,
      shell: "cmd",
      enabledItems: ["git", "node"],
      trackingRootIds: ["project"],
      trackingRootsConfigured: true,
      contractFingerprint: "fingerprint",
    })).toMatchObject({
      workspace: "D:\\work",
      agentId: "codex",
      mode: "desktop",
      desktopAction: "attach",
      desktopPid: 42,
      shell: "cmd",
    });
  });

  it("migrates incomplete legacy profiles to safe defaults", () => {
    expect(normalizeWorkSessionProfile({ workspace: "D:\\legacy", shell: "invalid" as never })).toEqual({
      workspace: "D:\\legacy",
      agentId: "",
      mode: "cli",
      shell: "powershell",
      desktopAction: "launch",
      desktopPid: null,
      enabledItems: [],
      trackingRootIds: [],
      trackingRootsConfigured: false,
      contractFingerprint: "",
    });
  });
});
