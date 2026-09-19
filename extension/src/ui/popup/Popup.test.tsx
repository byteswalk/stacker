// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import "fake-indexeddb/auto";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { getConversation, mergeListing, openDb, updateLocal, upsertAccount } from "../../lib/db";
import { LAST_TAB_KEY, type SiteTab } from "../../lib/lastTab";
import { Popup } from "./Popup";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const tabOf = (id: string): SiteTab => ({ tabId: 1, site: "chatgpt", url: `https://chatgpt.com/c/${id}`, title: id });

async function until(check: () => boolean) {
  for (let i = 0; i < 50 && !check(); i++) await act(async () => { await new Promise((r) => setTimeout(r, 10)); });
  expect(check()).toBe(true);
}

describe("Popup note", () => {
  it("shows each conversation's own note and saves on blur only when it changed", async () => {
    const db = await openDb();
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "ChatGPT" }, 1);
    await mergeListing(db, account, ["a", "b"].map((id) => ({ id, title: `T ${id}`, createdAt: 1, updatedAt: 2, archived: false })), true, 1);
    await updateLocal(db, ["chatgpt:a"], { note: "note A" });
    await updateLocal(db, ["chatgpt:b"], { note: "note B" });

    let onChanged: (changes: Record<string, chrome.storage.StorageChange>) => void = () => {};
    Object.assign(globalThis, {
      chrome: {
        storage: { session: {
          get: vi.fn(async () => ({ [LAST_TAB_KEY]: tabOf("a") })),
          onChanged: { addListener: (l: typeof onChanged) => { onChanged = l; }, removeListener: () => {} },
        } },
        runtime: { getURL: (p: string) => p },
        tabs: { create: vi.fn() },
      },
    });
    const host = document.createElement("div");
    document.body.append(host);
    act(() => createRoot(host).render(<Popup />));
    const area = () => host.querySelector("textarea");
    await until(() => area()?.value === "note A");

    act(() => onChanged({ [LAST_TAB_KEY]: { newValue: tabOf("b") } }));
    await until(() => area()?.value === "note B");

    // Someone else changes the note meanwhile; an unchanged blur must not overwrite it.
    await updateLocal(db, ["chatgpt:b"], { note: "edited elsewhere" });
    act(() => { area()!.focus(); area()!.blur(); });
    await act(async () => { await new Promise((r) => setTimeout(r, 20)); });
    expect((await getConversation(db, "chatgpt:b"))?.note).toBe("edited elsewhere");
    host.remove();
  });
});
