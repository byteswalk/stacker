// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { MigrationDialog } from "./MigrationDialog";
import type { LocationStatus } from "./types";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const status: LocationStatus = {
  agent: "codex", source: "C:/Users/u/.codex", actual: "C:/Users/u/.codex", kind: "normal", step: null,
  target: "", backup: "", backupExists: false, suggestedTarget: "D:/AgentData/codex",
  drives: [{ root: "D:/", fileSystem: "NTFS", free: 200e9, fixed: true }],
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const startButton = () => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("开始迁移")) as HTMLButtonElement;

describe("migration dialog", () => {
  it("blocks while the agent runs and starts once checks pass", async () => {
    let problems = ["E_APP_RUNNING"];
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "migration_check") return { problems, bytes: 13e9, files: 13927, free: 200e9, target: "D:/AgentData/codex" };
      if (command === "migration_start") return { agent: "codex", action: "migrate", state: "running", copied: 0, total: 13e9, error: "" };
      return null;
    });
    await act(async () => { root.render(<MigrationDialog status={status} mode="migrate" onClose={() => {}} />); });
    expect(host.textContent).toContain("请先完全退出 Codex");
    expect(startButton().disabled).toBe(true);

    problems = [];
    const recheck = [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("重新检查"));
    await act(async () => { recheck!.click(); });
    expect(startButton().disabled).toBe(false);
    await act(async () => { startButton().click(); });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("migration_start", { agent: "codex", target: "D:/AgentData/codex" });
  });
});
