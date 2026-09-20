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

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers(); vi.clearAllMocks();
  jobState = "running";
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "summary_settings") return { runner: "same", codexModel: "", codexEffort: "low", claudeModel: "sonnet", claudeEffort: "low" };
    if (command === "runner_options") return [{ agent: "claude", installed: true, models: [{ id: "sonnet", label: "sonnet", efforts: ["low"], defaultEffort: "low" }], efforts: ["low"] }];
    if (command === "distill_preview") return { items: [{ title: "Trip plan", chars: 30 }], totalChars: 30, runner: { agent: "claude", model: "sonnet", effort: "low" } };
    if (command === "distill_candidates") return [{ kind: "excerpt", key: "e1", title: "Budget", subtitle: "Book early", available: true }];
    if (command === "distill_start") return { id: "distill-1", state: "running", stage: "distilling", done: 0, total: 2, saved: 0, folders: [], error: "", by: "claude / sonnet / low" };
    if (command === "distill_job") return { id: "distill-1", state: jobState, stage: jobState === "running" ? "distilling" : "saving", done: jobState === "running" ? 1 : 2, total: 2, saved: jobState === "running" ? 0 : 3, folders: jobState === "running" ? [] : ["Plan a trip"], error: "", by: "claude / sonnet / low" };
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
    expect(host.textContent).toContain("Claude · sonnet · low");
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
});
