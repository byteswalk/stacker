import "fake-indexeddb/auto";
import { describe, expect, it } from "vitest";
import type { SiteApi } from "./siteClient";
import { listConversations, openDb } from "./db";
import { createPacer } from "./pacer";
import { refreshIndex } from "./refresh";
import { SiteError } from "../shared/types";

const conv = (id: string) => ({ id, title: id, createdAt: 1, updatedAt: 2, archived: false });

describe("refreshIndex", () => {
  it("walks every page, then marks the rest of that account removed", async () => {
    const db = await openDb("refresh-1");
    const pages: Record<string, { items: ReturnType<typeof conv>[]; next: string | null }> = {
      first: { items: [conv("a"), conv("b")], next: "2" },
      "2": { items: [conv("c")], next: null },
    };
    const api = {
      account: async () => ({ remoteId: "u", label: "ChatGPT" }),
      list: async (_site: string, cursor: string | null) => pages[cursor ?? "first"],
    } as unknown as SiteApi;
    const totals: number[] = [];
    const pacer = createPacer({ gapMs: 0, sleep: async () => {} });
    const result = await refreshIndex(api, db, "chatgpt", pacer, (n) => totals.push(n), () => 5);
    expect(result).toMatchObject({ added: 3, removed: 0, total: 3 });
    expect(totals).toEqual([2, 3]);
    pages.first = { items: [conv("a")], next: null };
    const again = await refreshIndex(api, db, "chatgpt", pacer, () => {}, () => 6);
    expect(again).toMatchObject({ removed: 2 });
    expect((await listConversations(db)).filter((c) => c.removedAt === 6).map((c) => c.id).sort()).toEqual(["b", "c"]);
  });

  it("retries on E_RATE and completes the walk", async () => {
    const db = await openDb("refresh-2");
    let listAttempts = 0;
    const pages: Record<string, { items: ReturnType<typeof conv>[]; next: string | null }> = {
      first: { items: [conv("a"), conv("b")], next: "2" },
      "2": { items: [conv("c")], next: null },
    };
    const api = {
      account: async () => ({ remoteId: "u", label: "ChatGPT" }),
      list: async (_site: string, cursor: string | null) => {
        listAttempts++;
        if (listAttempts === 2) {
          throw new SiteError("E_RATE", "", 5);
        }
        return pages[cursor ?? "first"];
      },
    } as unknown as SiteApi;
    const pacer = createPacer({ gapMs: 0, sleep: async () => {} });
    const result = await refreshIndex(api, db, "chatgpt", pacer, () => {}, () => 5);
    expect(result).toMatchObject({ added: 3, total: 3 });
    expect(listAttempts).toBe(3); // first page (fail), first page (retry), second page
  });
});
