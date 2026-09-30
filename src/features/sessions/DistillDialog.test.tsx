// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { DistillDialog } from "./DistillDialog";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

let host: HTMLDivElement;
let root: Root;
let jobState = "running";
let jobDropped = 0;
let jobError = "";
// 后端真的开始过一个任务了吗：没开始过时 `distill_job` 必须回 null，就像真实后端一样，
// 否则每个测试一挂载就会立刻「看见」一个正在跑的任务。
let jobStarted = false;
let previewSkipped = 0;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers(); vi.clearAllMocks();
  jobState = "running"; jobDropped = 0; jobError = ""; jobStarted = false; previewSkipped = 0;
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "distill_preview") return { items: [{ title: "Trip plan", chars: 30 }], totalChars: 30, runner: { backend: "claude", model: "sonnet", effort: null }, skipped: previewSkipped };
    if (command === "distill_candidates") return [{ kind: "excerpt", key: "e1", title: "Budget", subtitle: "Book early", available: true }];
    if (command === "distill_start") { jobStarted = true; return { id: "distill-1", state: "running", stage: "distilling", done: 0, total: 2, saved: 0, folders: [], dropped: 0, error: "", by: "claude / sonnet / low" }; }
    if (command === "distill_job") {
      if (!jobStarted) return null;
      return {
        id: "distill-1",
        state: jobState,
        stage: jobState === "running" ? "distilling" : "saving",
        done: jobState === "running" ? 1 : 2,
        total: 2,
        saved: jobState === "running" ? 0 : 3,
        folders: jobState === "running" ? [] : ["Plan a trip"],
        dropped: jobState === "running" ? 0 : jobDropped,
        error: jobError,
        by: "claude / sonnet / low",
      };
    }
    return null;
  });
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

async function mount() {
  await act(async () => { root.render(<DistillDialog initial={[{ kind: "web", key: "chatgpt:a" }]} onClose={() => {}} />); });
  await act(async () => { await vi.advanceTimersByTimeAsync(10); });
}
async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => { (el as HTMLElement).click(); });
}
const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text));

describe("distill dialog", () => {
  it("asks before sending anything to a model, naming the runner and the size", async () => {
    await mount();
    expect(host.textContent).toContain("Trip plan");
    await click(button("开始提炼"));
    expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).not.toContain("distill_start");
    // The dialog names the AI source from Preferences; it no longer offers its own.
    expect(host.textContent).toContain("本机 claude / sonnet");
    expect(host.textContent).toContain("30");
  });

  it("sends only the chosen output types and shows progress, then the skill folder", async () => {
    await mount();
    await click([...host.querySelectorAll(".distill-kinds label")].map((l) => l.querySelector("input"))[2]);
    await click(button("开始提炼"));
    const confirm = [...host.querySelectorAll(".modal button")].filter((b) => b.textContent === "开始提炼").pop();
    await click(confirm);
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(invoke).toHaveBeenCalledWith("distill_start", expect.objectContaining({
      sources: [{ kind: "web", key: "chatgpt:a" }],
      kinds: ["qa", "requirement", "prompt"],
    }));
    expect(host.textContent).toContain("正在提炼");
    jobState = "completed";
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(host.textContent).toContain("已完成");
    expect(host.textContent).toContain("3");
    await click(button("打开 skill 草稿文件夹"));
    expect(invoke).toHaveBeenCalledWith("distill_open", { target: "skills", name: "" });
  });

  it("can add another source from the picker", async () => {
    await mount();
    await click(button("添加来源"));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    await click([...host.querySelectorAll(".distill-candidate")].pop());
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    const last = vi.mocked(invoke).mock.calls.filter(([c]) => c === "distill_preview").pop();
    expect(last?.[1]).toMatchObject({ sources: [{ kind: "web", key: "chatgpt:a" }, { kind: "excerpt", key: "e1" }] });
  });

  it("shows how many sources could not be read, in the preview", async () => {
    previewSkipped = 2;
    await mount();
    expect(host.textContent).toContain("2");
    expect(host.textContent).toContain("个来源无法读取，已跳过");
  });

  it("reports items dropped over the cap once the job is done, but not while still running", async () => {
    await mount();
    await click(button("开始提炼"));
    await click([...host.querySelectorAll(".modal button")].filter((b) => b.textContent === "开始提炼").pop());
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    // 还在跑的时候，不该出现「另有 N 条未保存」这种要等跑完才有意义的行。
    expect(host.textContent).not.toContain("条未保存");
    jobDropped = 5;
    jobState = "completed";
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(host.textContent).toContain("另有");
    expect(host.textContent).toContain("条未保存（超出上限）");
  });

  it("does not show a red alert for a user-requested cancel, even though the backend records an error code for it", async () => {
    await mount();
    await click(button("开始提炼"));
    await click([...host.querySelectorAll(".modal button")].filter((b) => b.textContent === "开始提炼").pop());
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    // 取消也会让后端记一个 error 码（例如 E_CANCELLED），但状态是 cancelled，不是 failed：
    // 这不是一次失败，不该弹红色错误提示。
    jobState = "cancelled";
    jobError = "E_CANCELLED";
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(host.textContent).toContain("已取消");
    expect(host.querySelector("[role=alert]")).toBeNull();
  });

  it("shows the red alert for an actual failure", async () => {
    await mount();
    await click(button("开始提炼"));
    await click([...host.querySelectorAll(".modal button")].filter((b) => b.textContent === "开始提炼").pop());
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    jobState = "failed";
    jobError = "E_RUNNER_FAILED";
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(host.querySelector("[role=alert]")).not.toBeNull();
  });

  it("seeds itself from an already-running job on mount, instead of failing later with E_DISTILL_BUSY", async () => {
    // 模拟对话框上次开着的时候启动了一个任务、被关掉了，但任务还在后端跑着。
    jobStarted = true;
    jobState = "running";
    await mount();
    expect(host.textContent).toContain("正在提炼");
    // 材料/产出类型这些挑选来源的界面不应该出现：直接进了「任务进行中」的画面。
    expect(host.querySelector(".distill-kinds")).toBeNull();
  });

  it("clears a stale preview on a failed refresh so the confirm dialog cannot quote old numbers", async () => {
    await mount();
    expect(host.textContent).toContain("Trip plan");
    // 挂载时的第一次预览成功；加一个来源触发的这次刷新失败。
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "distill_candidates") return [{ kind: "excerpt", key: "e1", title: "Budget", subtitle: "Book early", available: true }];
      if (command === "distill_preview") throw new Error("E_NO_BODY");
      return null;
    });
    await click(button("添加来源"));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    await click([...host.querySelectorAll(".distill-candidate")].pop());
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(host.textContent).not.toContain("Trip plan");
    expect(button("开始提炼")?.hasAttribute("disabled")).toBe(true);
  });
});
