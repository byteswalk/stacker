import "fake-indexeddb/auto";
import { beforeEach, describe, expect, it } from "vitest";
import {
  accountDisplayName, addExcerpt, addTag, bodyIsFresh, createFolder, deleteFolder, getBody, getConversation,
  listAccounts, listConversations, listExcerpts, markRemoved, mergeListing, openDb, putBody, renameAccount,
  updateLocal, upsertAccount, type Account, type Db,
} from "./db";

let db: Db;
let n = 0;
beforeEach(async () => { db = await openDb(`test-${n++}`); });

const item = (id: string, updatedAt = 2) => ({ id, title: id.toUpperCase(), createdAt: 1, updatedAt, archived: false });

describe("db", () => {
  it("keeps local fields when a listing refreshes a conversation", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u1", label: "ChatGPT" }, 10);
    expect(account).toMatchObject({ key: "chatgpt:u1", name: "ChatGPT", alias: "" });
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
    expect(two.alias).toBe("");
    expect(accountDisplayName(two)).toBe("Claude");
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

  it("filters excerpts by site only", async () => {
    const chatgptAccount = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "ChatGPT" }, 1);
    const claudeAccount = await upsertAccount(db, "claude", { remoteId: "o", label: "Claude" }, 1);
    await mergeListing(db, chatgptAccount, [item("a")], true, 1);
    await mergeListing(db, claudeAccount, [item("b")], true, 1);
    await addExcerpt(db, { site: "chatgpt", conversationId: "a", url: "https://chatgpt.com/c/a", pageTitle: "A", text: "gpt text", note: "" }, 5);
    await addExcerpt(db, { site: "claude", conversationId: "b", url: "https://claude.ai/chat/b", pageTitle: "B", text: "claude text", note: "" }, 6);
    expect((await listExcerpts(db, "claude")).map((e) => e.text)).toEqual(["claude text"]);
  });

  it("stores the site's name on first sight and updates it on every later upsert", async () => {
    const first = await upsertAccount(db, "chatgpt", { remoteId: "u1", label: "Ada Lovelace" }, 1);
    expect(first.name).toBe("Ada Lovelace");
    expect(accountDisplayName(first)).toBe("Ada Lovelace");
    const second = await upsertAccount(db, "chatgpt", { remoteId: "u1", label: "Ada L." }, 2);
    expect(second.name).toBe("Ada L.");
    expect(accountDisplayName(second)).toBe("Ada L.");
  });

  it("lets a user alias override the site name, and a cleared alias falls back to the name", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u1", label: "Ada Lovelace" }, 1);
    await renameAccount(db, account.key, "Work account");
    const renamed = (await listAccounts(db))[0];
    expect(renamed.alias).toBe("Work account");
    expect(accountDisplayName(renamed)).toBe("Work account");
    // The site keeps reporting its own name even while an alias is set.
    await upsertAccount(db, "chatgpt", { remoteId: "u1", label: "Ada L." }, 2);
    expect(accountDisplayName((await listAccounts(db))[0])).toBe("Work account");
    await renameAccount(db, account.key, "");
    const cleared = (await listAccounts(db))[0];
    expect(cleared.alias).toBe("");
    expect(accountDisplayName(cleared)).toBe("Ada L.");
  });

  it("migrates an old auto-generated alias so the real name shows through, but keeps a user-chosen alias", async () => {
    const auto: Account = { key: "chatgpt:u1", site: "chatgpt", remoteId: "u1", alias: "ChatGPT 2", lastSeen: 1 } as unknown as Account;
    const custom: Account = { key: "claude:o1", site: "claude", remoteId: "o1", alias: "My Work Claude", lastSeen: 1 } as unknown as Account;
    const db2 = await openDb(`test-migrate-${n++}`);
    await db2.put("accounts", auto);
    await db2.put("accounts", custom);
    const [migratedAuto, migratedCustom] = await listAccounts(db2);
    expect(migratedAuto).toMatchObject({ alias: "", name: "ChatGPT" });
    expect(migratedCustom).toMatchObject({ alias: "My Work Claude", name: "Claude" });
    expect(accountDisplayName(migratedAuto)).toBe("ChatGPT");
    expect(accountDisplayName(migratedCustom)).toBe("My Work Claude");
  });
});
