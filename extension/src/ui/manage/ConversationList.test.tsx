// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import type { Conversation } from "../../lib/db";
import { ConversationList } from "./ConversationList";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const conv = (id: string): Conversation => ({
  key: `claude:${id}`, site: "claude", account: "claude:o", id, title: `T ${id}`, createdAt: 0, updatedAt: 0, archived: false,
  folderId: null, tags: ["x"], favorite: false, note: "", bodyFetchedAt: 1, bodyUpdatedAt: 0, removedAt: null,
});

describe("ConversationList", () => {
  it("selects the page and every result", () => {
    const host = document.createElement("div");
    const onSelect = vi.fn();
    const items = Array.from({ length: 120 }, (_, i) => conv(String(i)));
    act(() => createRoot(host).render(<ConversationList items={items} aliasOf={() => "Personal"} selected={new Set()} onSelect={onSelect} onOpen={() => {}} active={null} />));
    expect(host.querySelectorAll("li")).toHaveLength(50);
    act(() => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("选中全部结果"))!.click());
    expect(onSelect).toHaveBeenCalledWith(items.map((c) => c.key), true);
    expect(host.textContent).toContain("Personal");
  });
});
