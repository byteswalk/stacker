// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import type { EntryView } from "./api";
import { columnsOf, EntryList } from "./EntryList";
import { MergeDialog } from "./MergeDialog";
import { groupEntries } from "./vaultView";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

function login(id: string, url: string, user: string, updatedAt: number, note = ""): EntryView {
  return {
    id, title: new URL(url).hostname, platform: new URL(url).hostname, kind: "other", expiresAt: null, tags: ["浏览器"], note,
    favorite: false, createdAt: 0, updatedAt, deletedAt: null, historyCount: 0, windows: false, ssh: null,
    fields: [{ name: "网址", secret: false, value: url, filled: true }, { name: "账号", secret: false, value: user, filled: true }, { name: "密码", secret: true, value: null, filled: true }],
  };
}

const entries = [
  login("a", "https://a.example.org/", "me", 5),
  login("b", "https://b.example.net/", "me", 4, "B 公司后台"),
  login("c", "https://c.example.io/", "me", 3),
  login("d", "https://d.example.dev/", "me", 2),
];

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  localStorage.clear();
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const render = async (list: EntryView[]) => {
  await act(async () => { root.render(<EntryList entries={list} today={new Date(2026, 9, 7)} onView={() => {}} onEdit={() => {}} onChanged={() => {}} />); });
};
const rows = () => [...host.querySelectorAll<HTMLElement>(".vault-row:not(.head)")];
const cells = () => rows().map((row) => row.querySelector<HTMLElement>(".vault-pick-cell")!);
const picked = () => rows().map((row) => row.classList.contains("picked"));
// React hears a row being entered from the pointer leaving the one before.
const fire = async (target: Element, type: string, init: MouseEventInit = {}) => {
  await act(async () => { target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, ...init })); });
};

describe("vault entry list", () => {
  it("picks every row the pointer is dragged across, and a range with shift", async () => {
    await render(entries);
    await fire(cells()[0], "pointerdown");
    await fire(rows()[0], "pointerout", { relatedTarget: rows()[1] });
    await fire(rows()[1], "pointerout", { relatedTarget: rows()[2] });
    await fire(window as unknown as Element, "pointerup");
    expect(picked()).toEqual([true, true, true, false]);
    // Pressing a picked box and dragging takes them back out.
    await fire(cells()[2], "pointerdown");
    await fire(rows()[2], "pointerout", { relatedTarget: rows()[1] });
    await fire(window as unknown as Element, "pointerup");
    expect(picked()).toEqual([true, false, false, false]);
    // Shift picks everything from the last box pressed (the third row) to this one.
    await fire(cells()[3], "pointerdown", { shiftKey: true });
    await fire(window as unknown as Element, "pointerup");
    expect(picked()).toEqual([true, false, true, true]);
    expect(host.textContent).toContain("已选 3 条");
  });

  it("shows a note in place of the site and says the full text on hover", async () => {
    await render(entries);
    const second = rows()[1].querySelectorAll<HTMLElement>(".mut")[0];
    expect(second.textContent).toBe("B 公司后台");
    expect(second.title).toContain("b.example.net");
  });

  it("keeps a widened column", () => {
    expect(columnsOf({})).toContain("minmax(0,1.6fr) minmax(0,1fr)");
    expect(columnsOf({ title: 320 })).toContain("320px minmax(0,1fr)");
  });

  it("lists what a merge would do before doing it", async () => {
    vi.mocked(invoke).mockImplementation((async (command: string, args?: { keep: string; others: string[] }) => {
      if (command === "vault_merge_preview") return args!.others.map((id) => id === "old");
      if (command === "vault_merge") return args!.others.length;
      return null;
    }) as typeof invoke);
    const group = groupEntries([
      login("new", "https://example.com/", "me", 9),
      login("old", "https://example.com", "me", 5),
      login("same", "https://EXAMPLE.com/#top", "me", 1),
    ])[0];
    const onDone = vi.fn();
    await act(async () => { root.render(<MergeDialog group={group} onClose={() => {}} onDone={onDone} />); });
    const text = () => document.body.textContent ?? "";
    expect(text()).toContain("https://example.com/");
    expect(text()).toContain("保留");
    expect(text()).toContain("密码不同，旧密码进历史");
    expect(text()).toContain("密码相同");
    // Leave one out, then merge only the other.
    const boxes = [...document.querySelectorAll<HTMLInputElement>(".vault-merge-row input[type=checkbox]")];
    await act(async () => { boxes[2].click(); });
    expect(text()).toContain("不动");
    const button = [...document.querySelectorAll<HTMLButtonElement>("button")].find((item) => item.textContent?.includes("合并 1 条"))!;
    await act(async () => { button.click(); });
    expect(invoke).toHaveBeenCalledWith("vault_merge", { keep: "new", others: ["old"] });
    expect(onDone).toHaveBeenCalled();
  });
});
