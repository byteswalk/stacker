// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { DistillPanel } from "./DistillPanel";
import type { DistillResult } from "./types";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const qa: DistillResult = {
  id: "qa-1", kind: "qa", title: "Where should we go?", body: "Kyoto in spring.",
  sources: [{ key: "web:chatgpt:a", kind: "web", title: "Trip plan", link: "" }],
  state: "draft", by: "claude / sonnet / low", folder: "", createdAt: 1, updatedAt: 2,
};
const skill: DistillResult = {
  ...qa, id: "skill-1", kind: "skill", title: "Plan a trip", folder: "Plan a trip",
  sources: [{ key: "session:codex:s1", kind: "session", title: "Trip code", link: "C:/x/s1.jsonl" }],
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers(); vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "distill_list") return { items: [qa, skill], total: 2, counts: { qa: 1, requirement: 0, prompt: 0, skill: 1, total: 2 } };
    if (command === "distill_state") return { ...qa, state: "adopted" };
    if (command === "distill_save") return { ...qa, title: "Edited" };
    if (command === "distill_export") return "C:/data/exports/distill/distill-1.md";
    return null;
  });
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

async function mount() {
  await act(async () => { root.render(<DistillPanel refresh={0} onNew={() => {}} />); });
  await act(async () => { await vi.advanceTimersByTimeAsync(350); });
}
async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => { (el as HTMLElement).click(); });
}
const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text));

describe("distill results library", () => {
  it("lists results with their type, state and sources", async () => {
    await mount();
    expect(host.textContent).toContain("Where should we go?");
    expect(host.textContent).toContain("经验问答");
    expect(host.textContent).toContain("Trip plan");
    expect(host.textContent).toContain("草稿");
  });

  it("filters by type", async () => {
    await mount();
    await click([...host.querySelectorAll(".distill-filters button")].find((b) => b.textContent?.includes("skill 草稿")));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    const last = vi.mocked(invoke).mock.calls.filter(([c]) => c === "distill_list").pop();
    expect(last?.[1]).toMatchObject({ query: { kind: "skill" } });
  });

  it("opens a result, edits it, adopts it and opens its skill folder", async () => {
    await mount();
    await click([...host.querySelectorAll(".distill-title")].pop());
    expect(host.querySelector(".modal")).toBeTruthy();
    const body = host.querySelector(".modal textarea") as HTMLTextAreaElement;
    expect(body.value).toContain("Kyoto in spring.");
    await click(button("打开 skill 草稿文件夹"));
    expect(invoke).toHaveBeenCalledWith("distill_open", { target: "skill", name: "Plan a trip" });
    await click(button("标为已采用"));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(invoke).toHaveBeenCalledWith("distill_state", { id: "skill-1", state: "adopted" });
  });

  it("asks before deleting and exports the current filter", async () => {
    await mount();
    await click([...host.querySelectorAll(".distill-title")][0]);
    await click(button("删除"));
    expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).not.toContain("distill_delete");
    await click([...host.querySelectorAll(".modal button")].filter((b) => b.textContent === "删除").pop());
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(invoke).toHaveBeenCalledWith("distill_delete", { id: "qa-1" });
  });
});
