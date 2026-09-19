import "fake-indexeddb/auto";
import { openDB } from "idb";
import { beforeEach, describe, expect, it } from "vitest";
import {
  accountDisplayName, addExcerpt, addTag, applyBackup, bodyIsFresh, createFolder, deleteExcerpt, deleteFolder, dropOutbox,
  enqueueAll, getBody, getConversation, listAccounts, listConversations, listExcerpts, listFolders, markRemoved, mergeListing,
  onOutboxChange, openDb, outboxCount, putBody, readOutbox, renameAccount, renameFolder, updateLocal, upsertAccount,
  type Account, type Db,
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

  it("becomes fresh after putBody even when the listing's updatedAt outruns the body's own (Grok-shaped gap)", async () => {
    // Grok and Gemini derive the listing's updatedAt from a different field than the body's
    // latest-message time, so a live-fetched body can carry an updatedAt earlier than what the
    // listing already held for that conversation.
    const account = await upsertAccount(db, "grok", { remoteId: "u", label: "Grok" }, 1);
    await mergeListing(db, account, [item("a", 20)], true, 1);
    await putBody(db, "grok:a", { id: "a", title: "A", updatedAt: 5, messages: [] }, 9);
    const c = (await getConversation(db, "grok:a"))!;
    expect(c.bodyUpdatedAt).toBe(20);
    expect(bodyIsFresh(c)).toBe(true);
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

const clearOutbox = async (d: Db) => dropOutbox(d, (await readOutbox(d, 1000)).map((e) => e.seq!));

describe("outbox", () => {
  it("upgrades a version 1 database and queues everything for the first sync", async () => {
    const name = `v1-${n++}`;
    const old = await openDB(name, 1, {
      upgrade(d) {
        d.createObjectStore("accounts", { keyPath: "key" });
        d.createObjectStore("conversations", { keyPath: "key" }).createIndex("account", "account");
        d.createObjectStore("bodies", { keyPath: "key" });
        d.createObjectStore("folders", { keyPath: "id" });
        d.createObjectStore("excerpts", { keyPath: "id" }).createIndex("conversation", ["site", "conversationId"]);
      },
    });
    await old.put("conversations", {
      key: "chatgpt:a", site: "chatgpt", account: "chatgpt:u", id: "a", title: "A", createdAt: 1, updatedAt: 2,
      archived: false, folderId: null, tags: [], favorite: false, note: "kept", bodyFetchedAt: null, bodyUpdatedAt: null, removedAt: null,
    });
    old.close();
    const upgraded = await openDb(name);
    expect((await readOutbox(upgraded, 10)).map((e) => e.kind)).toEqual(["all"]);
    expect((await getConversation(upgraded, "chatgpt:a"))?.note).toBe("kept");
  });

  it("queues every local change, and only real changes", async () => {
    await clearOutbox(db);
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 2);
    await renameAccount(db, account.key, "Work", 3);
    await mergeListing(db, account, [item("a"), item("b")], true, 4);
    await mergeListing(db, account, [item("a"), item("b")], true, 5);
    await updateLocal(db, ["chatgpt:a"], { note: "x" }, 6);
    await addTag(db, ["chatgpt:a"], "t", 7);
    await addTag(db, ["chatgpt:a"], "t", 8);
    await putBody(db, "chatgpt:a", { id: "a", title: "A", updatedAt: 2, messages: [] }, 9);
    const folder = await createFolder(db, "F", 10);
    await renameFolder(db, folder.id, "G", 11);
    await deleteFolder(db, folder.id, 12);
    const excerpt = await addExcerpt(db, { site: "chatgpt", conversationId: "a", url: "u", pageTitle: "p", text: "t", note: "" }, 13);
    await deleteExcerpt(db, excerpt.id, 14);
    await markRemoved(db, "chatgpt:b", 15);
    expect((await readOutbox(db, 100)).map((e) => `${e.kind}:${e.key}`)).toEqual([
      "account:chatgpt:u", "account:chatgpt:u",
      "conversation:chatgpt:a", "conversation:chatgpt:b",
      "conversation:chatgpt:a", "conversation:chatgpt:a",
      "body:chatgpt:a",
      `folder:${folder.id}`, `folder:${folder.id}`, `removeFolder:${folder.id}`,
      `excerpt:${excerpt.id}`, `removeExcerpt:${excerpt.id}`,
      "conversation:chatgpt:b",
    ]);
    const a = (await getConversation(db, "chatgpt:a"))!;
    expect([a.listedAt, a.localUpdatedAt]).toEqual([5, 7]);
    expect((await getConversation(db, "chatgpt:b"))?.listedAt).toBe(15);
    expect((await listAccounts(db))[0].localUpdatedAt).toBe(3);
  });

  it("queues the conversations a deleted folder releases", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 1);
    const folder = await createFolder(db, "F", 2);
    await updateLocal(db, ["chatgpt:a"], { folderId: folder.id }, 3);
    await clearOutbox(db);
    await deleteFolder(db, folder.id, 4);
    expect((await readOutbox(db, 10)).map((e) => e.kind)).toEqual(["removeFolder", "conversation"]);
    expect((await getConversation(db, "chatgpt:a"))?.localUpdatedAt).toBe(4);
  });

  it("keeps at most one 'send everything' entry waiting", async () => {
    await enqueueAll(db, 1);
    await enqueueAll(db, 2);
    expect((await readOutbox(db, 10)).map((e) => e.kind)).toEqual(["all"]);
  });

  it("tells the registered listener after a queued change", async () => {
    let calls = 0;
    onOutboxChange(() => { calls++; });
    await createFolder(db, "F", 1);
    onOutboxChange(null);
    await createFolder(db, "G", 2);
    expect(calls).toBe(1);
  });
});

describe("applyBackup", () => {
  const wire = (id: string) => ({
    key: `chatgpt:${id}`, site: "chatgpt" as const, account: "chatgpt:u", id, title: id, createdAt: 1, updatedAt: 2,
    archived: false, removedAt: null, listedAt: 1, folderId: null, tags: [] as string[], favorite: false, note: "", localUpdatedAt: 0,
  });

  it("adds what is missing and takes only newer local fields, without queueing", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 1);
    await updateLocal(db, ["chatgpt:a"], { note: "local newer" }, 50);
    await clearOutbox(db);
    const counts = await applyBackup(db, {
      accounts: [{ key: "chatgpt:u", site: "chatgpt", remoteId: "u", name: "Ada", alias: "Work", lastSeen: 1, localUpdatedAt: 40 }],
      folders: [{ id: "f1", name: "Trips", createdAt: 1, localUpdatedAt: 1 }],
      conversations: [
        { ...wire("a"), note: "from stacker", localUpdatedAt: 10 },
        { ...wire("b"), folderId: "f1", tags: ["travel"], localUpdatedAt: 20 },
        { ...wire("x"), key: "elsewhere:x", site: "elsewhere" as never },
      ],
      excerpts: [{ id: "e1", site: "chatgpt", conversationId: "b", url: "u", pageTitle: "p", text: "tip", note: "", createdAt: 1, localUpdatedAt: 1 }],
    });
    expect(counts).toEqual({ accounts: 1, folders: 1, conversations: 1, excerpts: 1 });
    expect((await getConversation(db, "chatgpt:a"))?.note).toBe("local newer");
    expect(await getConversation(db, "chatgpt:b")).toMatchObject({ folderId: "f1", tags: ["travel"], bodyFetchedAt: null, bodyUpdatedAt: null });
    expect(await getConversation(db, "elsewhere:x")).toBeUndefined();
    expect((await listAccounts(db))[0].alias).toBe("Work");
    expect((await listFolders(db)).map((f) => f.name)).toEqual(["Trips"]);
    expect(await outboxCount(db)).toBe(0);
  });
});
