import "fake-indexeddb/auto";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BridgeError } from "./bridge";
import {
  addExcerpt, createFolder, deleteExcerpt, dropOutbox, enqueueAll, getBody, getConversation, mergeListing, openDb, outboxCount,
  putBody, readOutbox, updateLocal, upsertAccount, type Conversation, type Db,
} from "./db";
import { batches, bodyChunks, buildRequests, byteSize, flush, MAX_BYTES, MAX_MESSAGE_CHARS, MAX_NOTE_CHARS, type Call } from "./sync";

let db: Db;
let n = 0;
beforeEach(async () => {
  db = await openDb(`sync-${n++}`);
  await dropOutbox(db, (await readOutbox(db, 1000)).map((e) => e.seq!));
});
const item = (id: string) => ({ id, title: id, createdAt: 1, updatedAt: 2, archived: false });
const conv = { key: "chatgpt:a", site: "chatgpt", account: "chatgpt:u", id: "a" } as Conversation;

describe("batches", () => {
  it("splits by count and by size", () => {
    expect(batches([1, 2, 3, 4, 5], 2).map((b) => b.length)).toEqual([2, 2, 1]);
    const big = "x".repeat(400);
    expect(batches([big, big, big], 10, 1000).map((b) => b.length)).toEqual([2, 1]);
    expect(batches([])).toEqual([]);
  });
});

describe("bodyChunks", () => {
  it("splits a large body by message and keeps every chunk under the limit", () => {
    const messages = Array.from({ length: 12 }, (_, i) => ({ role: "user" as const, text: `${i}`.padEnd(200_000, "字"), at: null, attachments: [] }));
    const chunks = bodyChunks(conv, { key: "chatgpt:a", id: "a", title: "A", updatedAt: 2, messages }, 9);
    expect(chunks.length).toBeGreaterThan(1);
    chunks.forEach((c, i) => {
      expect([c.chunk, c.chunks, c.fetchedAt]).toEqual([i, chunks.length, 9]);
      expect(byteSize(c)).toBeLessThan(MAX_BYTES);
    });
    expect(chunks.flatMap((c) => c.messages)).toEqual(messages);
  });

  it("clips a message that could never fit and sends an empty body as one chunk", () => {
    const huge = { role: "assistant" as const, text: "y".repeat(MAX_MESSAGE_CHARS + 10), at: null, attachments: [] };
    const [only] = bodyChunks(conv, { key: "chatgpt:a", id: "a", title: "A", updatedAt: 2, messages: [huge] }, 1);
    expect(only.messages[0].text.endsWith("…[truncated]")).toBe(true);
    expect(bodyChunks(conv, { key: "chatgpt:a", id: "a", title: "A", updatedAt: 2, messages: [] }, 1))
      .toMatchObject([{ chunk: 0, chunks: 1, messages: [] }]);
  });

  it("sends the conversation's corrected bodyUpdatedAt, not the site's raw body time (Grok/Gemini can report 0)", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    // The listing says the conversation was updated at 100; Grok/Gemini's body read reports 0 for
    // its own latest-message time, a gap db.ts's putBody already closes on the stored conversation.
    await mergeListing(db, account, [item("a")].map((i) => ({ ...i, updatedAt: 100 })), true, 2);
    await putBody(db, "chatgpt:a", { id: "a", title: "A", updatedAt: 0, messages: [{ role: "user", text: "hi", at: null, attachments: [] }] }, 5);
    const c = (await getConversation(db, "chatgpt:a"))!;
    const body = (await getBody(db, "chatgpt:a"))!;
    const [chunk] = bodyChunks(c, body, 5);
    expect(chunk.updatedAt).toBe(100);
  });
});

describe("flush", () => {
  it("sends each changed record once, in order, and empties the outbox", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 2);
    await updateLocal(db, ["chatgpt:a"], { note: "one" }, 3);
    await updateLocal(db, ["chatgpt:a"], { note: "two" }, 4);
    await putBody(db, "chatgpt:a", { id: "a", title: "A", updatedAt: 2, messages: [{ role: "user", text: "hi", at: null, attachments: [] }] }, 5);
    await createFolder(db, "F", 6);
    const excerpt = await addExcerpt(db, { site: "chatgpt", conversationId: "a", url: "u", pageTitle: "p", text: "t", note: "" }, 7);
    await deleteExcerpt(db, excerpt.id, 8);
    const call = vi.fn<Call>(async () => ({}));
    expect(await flush(db, call)).toBe(5);
    expect(call.mock.calls.map(([type]) => type)).toEqual(["syncAccounts", "syncFolders", "syncConversations", "syncBody", "removeRecords"]);
    const conversations = call.mock.calls[2][1] as { items: { key: string; note: string; listedAt: number; localUpdatedAt: number }[] };
    expect(conversations.items).toEqual([expect.objectContaining({ key: "chatgpt:a", note: "two", listedAt: 2, localUpdatedAt: 4 })]);
    expect(call.mock.calls[3][1]).toMatchObject({ key: "chatgpt:a", fetchedAt: 5, chunk: 0, chunks: 1 });
    expect(call.mock.calls[4][1]).toEqual({ items: [{ kind: "excerpt", key: excerpt.id, at: 8 }] });
    expect(await outboxCount(db)).toBe(0);
  });

  it("keeps what failed in the outbox and sends it next time", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 2);
    const failing = vi.fn<Call>(async (type) => {
      if (type === "syncConversations") throw new Error("E_NOT_CONNECTED");
      return {};
    });
    await expect(flush(db, failing)).rejects.toThrow("E_NOT_CONNECTED");
    expect((await readOutbox(db, 10)).map((e) => e.kind)).toEqual(["conversation"]);
    const ok = vi.fn<Call>(async () => ({}));
    await flush(db, ok);
    expect(ok.mock.calls.map(([type]) => type)).toEqual(["syncConversations"]);
    expect(await outboxCount(db)).toBe(0);
  });

  it("sends every record for an 'all' entry, 200 conversations per message", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, Array.from({ length: 250 }, (_, i) => item(`c${i}`)), true, 2);
    await dropOutbox(db, (await readOutbox(db, 1000)).map((e) => e.seq!));
    await enqueueAll(db, 3);
    const call = vi.fn<Call>(async () => ({}));
    await flush(db, call);
    const sizes = call.mock.calls.filter(([type]) => type === "syncConversations").map(([, p]) => (p as { items: unknown[] }).items.length);
    expect(sizes).toEqual([200, 50]);
    expect(call.mock.calls[0][0]).toBe("syncAccounts");
    expect(await outboxCount(db)).toBe(0);
  });

  it("drops a message Stacker will always refuse and keeps flushing the rest, instead of blocking the outbox forever", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 2);
    await createFolder(db, "F", 3);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const call = vi.fn<Call>(async (type) => {
      if (type === "syncConversations") throw new BridgeError("E_REQUEST");
      return {};
    });
    expect(await flush(db, call)).toBe(2);
    expect(call.mock.calls.map(([type]) => type)).toEqual(["syncAccounts", "syncFolders", "syncConversations"]);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("dropped"));
    expect(await outboxCount(db)).toBe(0);
    warn.mockRestore();
  });

  it("still stops and retries later for a connection failure, even one reported as a BridgeError", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 2);
    await createFolder(db, "F", 3);
    const call = vi.fn<Call>(async (type) => {
      if (type === "syncConversations") throw new BridgeError("E_NOT_CONNECTED");
      return {};
    });
    await expect(flush(db, call)).rejects.toThrow("E_NOT_CONNECTED");
    expect((await readOutbox(db, 10)).map((e) => e.kind)).toEqual(["conversation"]);
    expect(await outboxCount(db)).toBe(1);
  });
});

describe("wireConversation note clamp", () => {
  it("clamps a conversation note to 20,000 characters in the wire record", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 2);
    await updateLocal(db, ["chatgpt:a"], { note: "x".repeat(MAX_NOTE_CHARS + 500) }, 3);
    const entries = await readOutbox(db, 100);
    const requests = await buildRequests(db, entries);
    const request = requests.find((r) => r.type === "syncConversations") as { payload: { items: { note: string }[] } } | undefined;
    expect(request?.payload.items[0].note.length).toBe(MAX_NOTE_CHARS);
  });
});
