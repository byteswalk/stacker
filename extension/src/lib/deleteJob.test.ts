import "fake-indexeddb/auto";
import { describe, expect, it, vi } from "vitest";
import { SiteError } from "../shared/types";
import { getConversation, listConversations, mergeListing, openDb, upsertAccount, type Db } from "./db";
import { runDeleteJob, type DeleteDeps } from "./deleteJob";
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
});
