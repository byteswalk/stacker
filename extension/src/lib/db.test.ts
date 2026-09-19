import "fake-indexeddb/auto";
import { beforeEach, describe, expect, it } from "vitest";
import {
  addExcerpt, addTag, bodyIsFresh, createFolder, deleteFolder, getBody, getConversation, listConversations,
  listExcerpts, markRemoved, mergeListing, openDb, putBody, updateLocal, upsertAccount, type Db,
} from "./db";

let db: Db;
let n = 0;
beforeEach(async () => { db = await openDb(`test-${n++}`); });

const item = (id: string, updatedAt = 2) => ({ id, title: id.toUpperCase(), createdAt: 1, updatedAt, archived: false });

describe("db", () => {
  it("keeps local fields when a listing refreshes a conversation", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u1", label: "ChatGPT" }, 10);
    expect(account).toMatchObject({ key: "chatgpt:u1", alias: "ChatGPT" });
    expect(await mergeListing(db, account, [item("a"), item("b")], true, 10)).toEqual({ added: 2, updated: 0, removed: 0 });
    await updateLocal(db, ["chatgpt:a"], { favorite: true, note: "keep" });
    await addTag(db, ["chatgpt:a", "chatgpt:b"], "work");
    expect(await mergeListing(db, account, [item("a", 5)], true, 20)).toEqual({ added: 0, updated: 1, removed: 1 });
    const a = await getConversation(db, "chatgpt:a");
    expect(a).toMatchObject({ updatedAt: 5, favorite: true, note: "keep", tags: ["work"], removedAt: null });
    expect((await getConversation(db, "chatgpt:b"))?.removedAt).toBe(20);
  });

  it("only marks missing ones removed after a complete listing of that account", async () => {
    const one = await upsertAccount(db, "claude", { remoteId: "o1", label: "Claude" }, 1);
    const two = await upsertAccount(db, "claude", { remoteId: "o2", label: "Claude" }, 1);
    expect(two.alias).toBe("Claude 2");
    await mergeListing(db, one, [item("x")], true, 1);
    await mergeListing(db, two, [item("y")], true, 1);
    await mergeListing(db, one, [], false, 2);
    expect((await getConversation(db, "claude:x"))?.removedAt).toBeNull();
    await mergeListing(db, one, [], true, 3);
    expect((await getConversation(db, "claude:x"))?.removedAt).toBe(3);
    expect((await getConversation(db, "claude:y"))?.removedAt).toBeNull();
  });

  it("stores bodies and tracks whether they are fresh", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "ChatGPT" }, 1);
    await mergeListing(db, account, [item("a", 5)], true, 1);
    await putBody(db, "chatgpt:a", { id: "a", title: "A", updatedAt: 5, messages: [] }, 9);
    const c = (await getConversation(db, "chatgpt:a"))!;
    expect([c.bodyFetchedAt, c.bodyUpdatedAt, bodyIsFresh(c)]).toEqual([9, 5, true]);
    expect((await getBody(db, "chatgpt:a"))?.title).toBe("A");
    await mergeListing(db, account, [item("a", 6)], true, 10);
    expect(bodyIsFresh((await getConversation(db, "chatgpt:a"))!)).toBe(false);
    await markRemoved(db, "chatgpt:a", 11);
    expect((await listConversations(db))[0].removedAt).toBe(11);
  });

  it("clears a deleted folder from its conversations and stores excerpts", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "ChatGPT" }, 1);
    await mergeListing(db, account, [item("a")], true, 1);
    const folder = await createFolder(db, "Trips", 1);
    await updateLocal(db, ["chatgpt:a"], { folderId: folder.id });
    await deleteFolder(db, folder.id);
    expect((await getConversation(db, "chatgpt:a"))?.folderId).toBeNull();
    await addExcerpt(db, { site: "chatgpt", conversationId: "a", url: "https://chatgpt.com/c/a", pageTitle: "A", text: "tip", note: "" }, 5);
    expect((await listExcerpts(db, "chatgpt", "a")).map((e) => e.text)).toEqual(["tip"]);
  });
});
