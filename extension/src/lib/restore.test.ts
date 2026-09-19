import "fake-indexeddb/auto";
import { describe, expect, it, vi } from "vitest";
import { getConversation, openDb, outboxCount, readOutbox, dropOutbox } from "./db";
import { restoreFromStacker } from "./restore";

describe("restoreFromStacker", () => {
  it("pages through every section and applies what Stacker has", async () => {
    const db = await openDb("restore-1");
    await dropOutbox(db, (await readOutbox(db, 100)).map((e) => e.seq!));
    const conversation = (id: string) => ({
      key: `chatgpt:${id}`, site: "chatgpt", account: "chatgpt:u", id, title: id, createdAt: 1, updatedAt: 2, archived: false,
      removedAt: null, listedAt: 1, folderId: null, tags: [], favorite: true, note: "", localUpdatedAt: 5,
    });
    const sections: Record<string, unknown[]> = {
      accounts: [{ key: "chatgpt:u", site: "chatgpt", remoteId: "u", name: "Ada", alias: "Work", lastSeen: 1, localUpdatedAt: 5 }],
      folders: [],
      conversations: [conversation("a"), conversation("b"), conversation("c")],
      excerpts: [],
    };
    // One item per page, so paging is exercised.
    const pull = vi.fn(async ({ section, offset }: { section: string; offset: number }) => ({
      items: sections[section].slice(offset, offset + 1),
      next: offset + 1 < sections[section].length ? offset + 1 : null,
    }));
    const counts = await restoreFromStacker(db, pull);
    expect(counts).toEqual({ accounts: 1, folders: 0, conversations: 3, excerpts: 0 });
    expect(pull).toHaveBeenCalledTimes(1 + 1 + 3 + 1);
    expect((await getConversation(db, "chatgpt:c"))?.favorite).toBe(true);
    expect(await outboxCount(db)).toBe(0);
  });

  it("stops when a page does not move forward", async () => {
    const db = await openDb("restore-2");
    const pull = vi.fn(async () => ({ items: [], next: 0 }));
    await expect(restoreFromStacker(db, pull)).resolves.toEqual({ accounts: 0, folders: 0, conversations: 0, excerpts: 0 });
    expect(pull).toHaveBeenCalledTimes(4);
  });
});
