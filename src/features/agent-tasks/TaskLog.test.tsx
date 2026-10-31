// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { ToastHost, ToastProvider } from "../../ui";
import { TaskRepairDialog } from "./TaskLog";
import type { AgentTask } from "./taskStore";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn(), reportFrontendError: vi.fn() }));

const task = { id: "task-7", surfaceLabel: "Qoder 桌面端", action: "update", state: "failed" } as AgentTask;
let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const repairers = (list: unknown[]) => vi.mocked(invoke).mockImplementation(async (command) => command === "agent_repairers" ? list : undefined);
const render = (onClose = vi.fn()) => act(async () => root.render(<ToastProvider><TaskRepairDialog task={task} onClose={onClose} /><ToastHost /></ToastProvider>));
const button = (label: string) => [...host.querySelectorAll("button")].find((item) => item.textContent?.includes(label));

describe("TaskRepairDialog", () => {
  it("opens the agent for this task, says what comes next, and closes", async () => {
    repairers([{ id: "codex", name: "Codex CLI" }]);
    const onClose = vi.fn();
    await render(onClose);
    expect(host.textContent).toContain("问你确认");
    await act(async () => button("打开 Codex CLI")!.click());
    expect(invoke).toHaveBeenCalledWith("agent_repair_start", { id: "task-7", agent: "codex" });
    expect(host.textContent).toContain("状态刷新");
    expect(onClose).toHaveBeenCalled();
  });

  it("says what to do when no agent is available, with nothing to start", async () => {
    repairers([]);
    await render();
    expect(host.textContent).toContain("没有找到可用的智能体");
    expect(button("打开")).toBeUndefined();
  });

  it("stays open and says why when the terminal cannot be opened", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "agent_repairers") return [{ id: "claude", name: "Claude Code" }];
      throw new Error("no terminal");
    });
    const onClose = vi.fn();
    await render(onClose);
    await act(async () => button("打开 Claude Code")!.click());
    expect(onClose).not.toHaveBeenCalled();
    expect(host.querySelector(".toast.err")?.textContent).toContain("no terminal");
  });
});
