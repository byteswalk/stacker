// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Conversation } from "../../lib/db";
import type { ItemResult } from "../../lib/deleteJob";
import type { SiteId } from "../../shared/types";
import { DeleteDialog } from "./DeleteDialog";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const conv = (id: string, account = "chatgpt:u", site: SiteId = "chatgpt"): Conversation => ({
  key: `${site}:${id}`, site, account, id, title: id, createdAt: 0, updatedAt: 0, archived: false,
  folderId: null, tags: [], favorite: false, note: "", bodyFetchedAt: null, bodyUpdatedAt: null, removedAt: null,
  listedAt: 0, localUpdatedAt: 0,
});
let host: HTMLDivElement;
let root: Root | null = null;
// Ant Design renders the dialog in a portal on document.body, not inside the mount point.
const ui = () => document.body;
// Unmount inside act, so no render React has queued is left to run after the test
// environment is torn down — that is what made this file fail at random under load.
afterEach(() => {
  if (root) act(() => root!.unmount());
  root = null;
  host?.remove();
  document.body.querySelectorAll(".ant-modal-root").forEach((n) => n.remove());
});

const ALIAS: Record<string, string> = { "chatgpt:u": "工作号", "chatgpt:other": "私人号", "claude:o": "Claude 号" };
function mount(onRun = vi.fn(async () => []), items = [conv("a"), conv("b", "chatgpt:other"), conv("c")], siteErrors: Partial<Record<SiteId, string>> = {}) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root!.render(<DeleteDialog items={items} currentAccounts={new Set(["chatgpt:u"])} aliasOf={(k) => ALIAS[k] ?? k}
    siteErrors={siteErrors} onRun={onRun} onClose={() => {}} />));
  return onRun;
}
// Ant Design widens a two-character Chinese label ("中止" -> "中 止"), so compare without spaces.
const flat = (text: string | null) => (text ?? "").replace(/\s/g, "");
const button = (text: string) => [...ui().querySelectorAll("button")].find((b) => flat(b.textContent).includes(flat(text)))!;

describe("DeleteDialog", () => {
  it("defaults to slim export and warns about other accounts", () => {
    mount();
    expect((ui().querySelector("input[value=slim]") as HTMLInputElement).checked).toBe(true);
    expect(ui().textContent).toContain("1 条不属于当前登录的账号");
  });
  it("counts items per site and account alias", () => {
    mount();
    const lines = [...ui().querySelectorAll("ul.groups li")].map((li) => li.textContent);
    expect(lines).toEqual(["ChatGPT · 工作号：2 条", "ChatGPT · 私人号：1 条 — 不是当前登录的账号，会被跳过"]);
    expect(ui().textContent).toContain("将删除 2 条对话");
  });
  it("shows a failed account check for a site instead of calling it another account, and leaves those items out", async () => {
    const items = [conv("a"), conv("k", "claude:o", "claude")];
    const onRun = mount(vi.fn(async () => []), items, { claude: "请先在浏览器中打开并登录该网站" });
    const lines = [...ui().querySelectorAll("ul.groups li")].map((li) => li.textContent);
    expect(lines).toEqual(["ChatGPT · 工作号：1 条", "Claude · Claude 号：1 条 — 请先在浏览器中打开并登录该网站"]);
    expect(ui().textContent).not.toContain("不属于当前登录的账号");
    await act(async () => button("删除 1 条").click());
    expect(onRun).toHaveBeenCalledWith([items[0]], "slim", expect.any(AbortSignal), expect.any(Function));
  });
  it("cannot start when no item can be deleted", () => {
    mount(vi.fn(async () => []), [conv("a")], { chatgpt: "接口已变化，请先刷新该站点" });
    expect((button("删除 0 条") as HTMLButtonElement).disabled).toBe(true);
  });
  it("asks a second time before deleting without a copy", async () => {
    const onRun = mount();
    act(() => (ui().querySelector("input[value=direct]") as HTMLInputElement).click());
    await act(async () => button("删除 2 条").click());
    expect(onRun).not.toHaveBeenCalled();
    expect(ui().textContent).toContain("不会留下任何副本");
    await act(async () => button("确认直接删除").click());
    expect(onRun).toHaveBeenCalledWith(expect.any(Array), "direct", expect.any(AbortSignal), expect.any(Function));
  });
  it("shows progress immediately, disables 完成, and blocks a second run", async () => {
    let resolveRun: (r: ItemResult[]) => void = () => {};
    const onRun = mount(vi.fn(() => new Promise<ItemResult[]>((resolve) => { resolveRun = resolve; })));
    const trigger = button("删除 2 条");
    act(() => { trigger.click(); trigger.click(); });
    expect(onRun).toHaveBeenCalledTimes(1);
    expect(ui().textContent).toContain("正在删除");
    expect(button("中止")).toBeTruthy();
    expect((button("完成") as HTMLButtonElement).disabled).toBe(true);
    const leaving = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(leaving);
    expect(leaving.defaultPrevented).toBe(true);
    await act(async () => { resolveRun([]); });
    const after = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(after);
    expect(after.defaultPrevented).toBe(false);
  });
  it("leaves out an unverified site's items and says why", async () => {
    const reason = "未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除";
    const items = [conv("a"), conv("g", "grok:default", "grok")];
    const onRun = mount(vi.fn(async () => []), items, { grok: reason });
    const lines = [...ui().querySelectorAll("ul.groups li")].map((li) => li.textContent);
    expect(lines).toEqual(["ChatGPT · 工作号：1 条", `Grok · grok:default：1 条 — ${reason}`]);
    expect(ui().querySelector("ul.groups li:last-child span")?.className).toContain("ant-typography-danger");
    await act(async () => button("删除 1 条").click());
    expect(onRun).toHaveBeenCalledWith([items[0]], "slim", expect.any(AbortSignal), expect.any(Function));
  });
});
