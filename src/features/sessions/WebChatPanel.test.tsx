// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import type { WebChat } from "./types";
import { WebChatPanel } from "./WebChatPanel";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const chat: WebChat = {
  key: "chatgpt:c1", site: "chatgpt", account: "chatgpt:u1", accountName: "Work", id: "c1", title: "Trip plan",
  createdAt: 1, updatedAt: 1_700_000_000_000, archived: false, removedAt: null, folder: "Trips", tags: ["travel"],
  favorite: false, note: "", bodyFetchedAt: 5, bodyMessages: 1, bodyStale: false,
  summary: null, summaryBy: "", summaryAt: 0, summaryStale: false,
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers(); vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "webchat_list") return { items: [chat], total: 1, accounts: [{ key: "chatgpt:u1", site: "chatgpt", name: "Work" }] };
    if (command === "webchat_read") return { chat, messages: [{ role: "user", text: "Where should we go?", at: null, attachments: [] }], chars: 30, runner: { agent: "claude", model: "sonnet", effort: "low" } };
    if (command === "webchat_summarize") return { ...chat, summary: "Go to Kyoto", summaryBy: "claude / sonnet / low", summaryAt: 1_700_000_000_000 };
    return null;
  });
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

async function mount() {
  await act(async () => { root.render(<WebChatPanel refresh={0} />); });
  await act(async () => { await vi.advanceTimersByTimeAsync(350); });
}
async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => { (el as HTMLElement).click(); });
}
const lastQuery = () => (vi.mocked(invoke).mock.calls.filter(([c]) => c === "webchat_list").pop()?.[1] as { query: { search: string; fullText: boolean } }).query;

describe("web chats tab", () => {
  it("lists synced chats with their site, account and folder", async () => {
    await mount();
    expect(host.textContent).toContain("Trip plan");
    expect(host.textContent).toContain("ChatGPT · Work · Trips");
  });

  it("searches after a pause and can include bodies", async () => {
    await mount();
    const input = host.querySelector("input[aria-label='搜索网页对话']") as HTMLInputElement;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "kyoto");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click(host.querySelector(".session-check input"));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    expect(lastQuery()).toMatchObject({ search: "kyoto", fullText: true });
  });

  it("asks before sending a body to the runner, then shows the summary", async () => {
    await mount();
    await click(host.querySelector(".session-title"));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(host.textContent).toContain("Where should we go?");
    await click([...host.querySelectorAll("button")].find((b) => b.textContent?.includes("生成摘要")));
    expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).not.toContain("webchat_summarize");
    expect(host.textContent).toContain("Claude · sonnet · low");
    const confirm = [...host.querySelectorAll(".modal button")].filter((b) => b.textContent === "生成摘要").pop();
    await click(confirm);
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(invoke).toHaveBeenCalledWith("webchat_summarize", { key: "chatgpt:c1", settings: null, locale: expect.any(String) });
    expect(host.textContent).toContain("Go to Kyoto");
  });
});
