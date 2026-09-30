// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Conversation } from "../../lib/db";
import { ConversationList } from "./ConversationList";

// Every root a test mounts is unmounted inside act afterwards, so nothing React has queued runs
// after the test environment is gone.
const mounted: Root[] = [];
const track = (root: Root) => { mounted.push(root); return root; };
afterEach(() => { while (mounted.length) { const root = mounted.pop()!; act(() => root.unmount()); } });

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const conv = (id: string): Conversation => ({
  key: `claude:${id}`, site: "claude", account: "claude:o", id, title: `T ${id}`, createdAt: 0, updatedAt: 0, archived: false,
  folderId: null, tags: ["x"], favorite: false, note: "", bodyFetchedAt: 1, bodyUpdatedAt: 0, removedAt: null,
  listedAt: 0, localUpdatedAt: 0,
});

describe("ConversationList", () => {
  it("pages long result sets and selects a whole page at once", () => {
    const host = document.createElement("div");
    document.body.append(host);
    const onSelect = vi.fn();
    const items = Array.from({ length: 120 }, (_, i) => conv(String(i)));
    act(() => track(createRoot(host)).render(<ConversationList items={items} aliasOf={() => "Personal"} selected={[]} onSelect={onSelect} onOpen={() => {}} active={null} />));
    expect(host.querySelectorAll("tbody tr.ant-table-row")).toHaveLength(50);
    expect(host.textContent).toContain("共 120 条");
    expect(host.textContent).toContain("Personal");

    // The header checkbox takes the current page only; the link above the table takes every result.
    act(() => (host.querySelector("thead input[type=checkbox]") as HTMLInputElement).click());
    expect(onSelect).toHaveBeenCalledWith(items.slice(0, 50).map((c) => c.key));

    act(() => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("选中全部结果"))!.click());
    expect(onSelect).toHaveBeenLastCalledWith(items.map((c) => c.key));
    host.remove();
  });
});
