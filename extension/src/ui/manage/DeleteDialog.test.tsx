// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Conversation } from "../../lib/db";
import type { ItemResult } from "../../lib/deleteJob";
import { DeleteDialog } from "./DeleteDialog";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const conv = (id: string, account = "chatgpt:u"): Conversation => ({
  key: `chatgpt:${id}`, site: "chatgpt", account, id, title: id, createdAt: 0, updatedAt: 0, archived: false,
  folderId: null, tags: [], favorite: false, note: "", bodyFetchedAt: null, bodyUpdatedAt: null, removedAt: null,
});
let host: HTMLDivElement;
afterEach(() => host?.remove());

function mount(onRun = vi.fn(async () => [])) {
  host = document.createElement("div");
  document.body.append(host);
  act(() => createRoot(host).render(<DeleteDialog items={[conv("a"), conv("b", "chatgpt:other")]} currentAccounts={new Set(["chatgpt:u"])} onRun={onRun} onClose={() => {}} />));
  return onRun;
}
const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text))!;

describe("DeleteDialog", () => {
  it("defaults to slim export and warns about other accounts", () => {
    mount();
    expect((host.querySelector("input[value=slim]") as HTMLInputElement).checked).toBe(true);
    expect(host.textContent).toContain("1 条不属于当前登录的账号");
  });
  it("asks a second time before deleting without a copy", async () => {
    const onRun = mount();
    act(() => (host.querySelector("input[value=direct]") as HTMLInputElement).click());
    await act(async () => button("删除 2 条").click());
    expect(onRun).not.toHaveBeenCalled();
    expect(host.textContent).toContain("不会留下任何副本");
    await act(async () => button("确认直接删除").click());
    expect(onRun).toHaveBeenCalledWith("direct", expect.any(AbortSignal), expect.any(Function));
  });
  it("shows progress immediately, disables 完成, and blocks a second run", async () => {
    let resolveRun: (r: ItemResult[]) => void = () => {};
    const onRun = mount(vi.fn(() => new Promise<ItemResult[]>((resolve) => { resolveRun = resolve; })));
    const trigger = button("删除 2 条");
    act(() => { trigger.click(); trigger.click(); });
    expect(onRun).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("正在删除");
    expect(button("中止")).toBeTruthy();
    expect((button("完成") as HTMLButtonElement).disabled).toBe(true);
    await act(async () => { resolveRun([]); });
  });
});
