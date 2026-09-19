import "fake-indexeddb/auto";
import { describe, expect, it, vi } from "vitest";
import { SiteError } from "../shared/types";
import { getConversation, listConversations, mergeListing, openDb, upsertAccount, type Db } from "./db";
import { ACCOUNT_RECHECK_EVERY, runDeleteJob, type DeleteDeps } from "./deleteJob";
import { createPacer } from "./pacer";
import type { SiteApi } from "./siteClient";

let n = 0;
async function setup(ids: string[], remoteId = "u") {
  const db = await openDb(`del-${n++}`);
  const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "ChatGPT" }, 1);
  await mergeListing(db, account, ids.map((id) => ({ id, title: id, createdAt: 1, updatedAt: 2, archived: false })), true, 1);
  const api = {
    account: vi.fn(async () => ({ remoteId, label: "ChatGPT" })),
    read: vi.fn(async (_s: string, id: string) => ({ id, title: id, updatedAt: 2, messages: [{ role: "user", text: `q ${id}`, at: null, attachments: [] }] })),
    remove: vi.fn(async () => {}),
  } as unknown as SiteApi;
  const saved: string[] = [];
  const deps: DeleteDeps = {
    api, db, now: () => 50, pacer: createPacer({ gapMs: 0, sleep: async () => {} }),
    save: async (path) => { saved.push(path); }, aliasOf: () => "Work",
  };
  return { db, api, deps, saved, items: await listConversations(db) };
}
const run = (items: Awaited<ReturnType<typeof setup>>["items"], mode: "slim" | "full" | "direct", deps: DeleteDeps, signal = new AbortController().signal) =>
  runDeleteJob(items, mode, deps, signal, () => {});

async function removed(db: Db, key: string) { return (await getConversation(db, key))?.removedAt; }

describe("delete job", () => {
  it("exports slim Markdown before deleting each conversation", async () => {
    const { db, api, deps, saved, items } = await setup(["a", "b"]);
    const results = await run(items, "slim", deps);
    expect(results.map((r) => r.status)).toEqual(["done", "done"]);
    expect(saved).toHaveLength(2);
    expect(saved.every((p) => p.endsWith(".md"))).toBe(true);
    expect(api.remove).toHaveBeenCalledTimes(2);
    expect(await removed(db, "chatgpt:a")).toBe(50);
  });
  it("writes Markdown and raw JSON for a full backup, nothing for direct", async () => {
    const full = await setup(["a"]);
    await run(full.items, "full", full.deps);
    expect(full.saved.map((p) => p.split(".").pop())).toEqual(["md", "json"]);
    const direct = await setup(["a"]);
    await run(direct.items, "direct", direct.deps);
    expect(direct.saved).toEqual([]);
    expect(direct.api.read).not.toHaveBeenCalled();
  });
  it("never deletes when the export fails", async () => {
    const { deps, api, items } = await setup(["a"]);
    deps.save = async () => { throw new Error("disk full"); };
    const [r] = await run(items, "slim", deps);
    expect(r.status).toBe("failed");
    expect(api.remove).not.toHaveBeenCalled();
  });
  it("skips an account that is not the one signed in", async () => {
    const { deps, api, items } = await setup(["a"], "someone-else");
    const [r] = await run(items, "direct", deps);
    expect([r.status, r.error]).toEqual(["skipped", "E_ACCOUNT"]);
    expect(api.remove).not.toHaveBeenCalled();
  });
  it("retries after a rate limit and stops the job when the site changed", async () => {
    const { deps, api, items } = await setup(["a", "b", "c"]);
    let calls = 0;
    (api.remove as ReturnType<typeof vi.fn>).mockImplementation(async () => {
      calls++;
      if (calls === 1) throw new SiteError("E_RATE", "", 10);
      if (calls === 3) throw new SiteError("E_BROKEN", "page.items");
    });
    const results = await run(items, "direct", deps);
    expect(results.map((r) => [r.status, r.error])).toEqual([["done", ""], ["failed", "E_BROKEN"], ["skipped", "E_BROKEN"]]);
  });
  it("treats an already deleted conversation as done and honours abort", async () => {
    const gone = await setup(["a"]);
    (gone.api.remove as ReturnType<typeof vi.fn>).mockRejectedValue(new SiteError("E_NOT_FOUND"));
    expect((await run(gone.items, "direct", gone.deps))[0].status).toBe("done");
    const { deps, items } = await setup(["a", "b"]);
    const controller = new AbortController();
    controller.abort();
    expect((await run(items, "direct", deps, controller.signal)).map((r) => r.error)).toEqual(["E_CANCELLED", "E_CANCELLED"]);
  });
  it("does not remove when the signal is cancelled during export, and skips the rest", async () => {
    const { deps, api, items } = await setup(["a", "b"]);
    const controller = new AbortController();
    deps.save = async () => { controller.abort(); };
    const results = await run(items, "slim", deps, controller.signal);
    expect(results.map((r) => [r.status, r.error])).toEqual([["skipped", "E_CANCELLED"], ["skipped", "E_CANCELLED"]]);
    expect(api.remove).not.toHaveBeenCalled();
  });
  it("stops the job when reading a conversation keeps rate-limiting", async () => {
    const { deps, api, items } = await setup(["a", "b", "c"]);
    (api.read as ReturnType<typeof vi.fn>).mockRejectedValue(new SiteError("E_RATE", "", 1));
    const results = await run(items, "slim", deps);
    expect(results.map((r) => [r.status, r.error])).toEqual([
      ["failed", "E_RATE"], ["skipped", "E_RATE"], ["skipped", "E_RATE"],
    ]);
    expect(api.remove).not.toHaveBeenCalled();
  });
  it("stops the job when a stop code is thrown while exporting", async () => {
    const { deps, api, items } = await setup(["a", "b"]);
    (api.read as ReturnType<typeof vi.fn>).mockRejectedValue(new SiteError("E_BROKEN", "page.items"));
    const results = await run(items, "slim", deps);
    expect(results.map((r) => [r.status, r.error])).toEqual([["failed", "E_BROKEN"], ["skipped", "E_BROKEN"]]);
    expect(api.remove).not.toHaveBeenCalled();
  });
  it("skips only the account that is not currently signed in when a job spans two accounts", async () => {
    const db = await openDb(`del-${n++}`);
    const acc1 = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "ChatGPT" }, 1);
    const acc2 = await upsertAccount(db, "chatgpt", { remoteId: "other", label: "ChatGPT" }, 1);
    await mergeListing(db, acc1, [{ id: "a", title: "a", createdAt: 1, updatedAt: 2, archived: false }], true, 1);
    await mergeListing(db, acc2, [{ id: "b", title: "b", createdAt: 1, updatedAt: 2, archived: false }], true, 1);
    const api = {
      account: vi.fn(async () => ({ remoteId: "u", label: "ChatGPT" })),
      read: vi.fn(async (_s: string, id: string) => ({ id, title: id, updatedAt: 2, messages: [] })),
      remove: vi.fn(async () => {}),
    } as unknown as SiteApi;
    const deps: DeleteDeps = {
      api, db, now: () => 50, pacer: createPacer({ gapMs: 0, sleep: async () => {} }),
      save: async () => {}, aliasOf: () => "Work",
    };
    const results = await run(await listConversations(db), "direct", deps);
    const byKey = new Map(results.map((r) => [r.key, r]));
    expect(byKey.get("chatgpt:a")?.status).toBe("done");
    expect([byKey.get("chatgpt:b")?.status, byKey.get("chatgpt:b")?.error]).toEqual(["skipped", "E_ACCOUNT"]);
    expect(api.remove).toHaveBeenCalledTimes(1);
  });
  it("re-checks the signed-in account every ACCOUNT_RECHECK_EVERY items and stops that account once it no longer matches", async () => {
    expect(ACCOUNT_RECHECK_EVERY).toBe(20);
    const ids = Array.from({ length: 21 }, (_, i) => `id${String(i).padStart(2, "0")}`);
    const { deps, api, items } = await setup(ids);
    let calls = 0;
    (api.account as ReturnType<typeof vi.fn>).mockImplementation(async () => {
      calls++;
      return calls === 1 ? { remoteId: "u", label: "ChatGPT" } : { remoteId: "other", label: "ChatGPT" };
    });
    const results = await run(items, "direct", deps);
    expect(results.slice(0, 20).map((r) => r.status)).toEqual(Array(20).fill("done"));
    expect([results[20].status, results[20].error]).toEqual(["skipped", "E_ACCOUNT"]);
    expect(api.account).toHaveBeenCalledTimes(2);
  });
});
