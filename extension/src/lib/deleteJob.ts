import { SiteError, type ErrorCode } from "../shared/types";
import { conversationUrl } from "../sites/registry";
import { accountKey, markRemoved, putBody, type Conversation, type Db, type StoredBody } from "./db";
import { exportFileName, toMarkdown } from "./markdown";
import { withPacing, type Pacer } from "./pacer";
import type { SiteApi } from "./siteClient";

export type DeleteMode = "slim" | "full" | "direct";
export type ItemStatus = "pending" | "done" | "failed" | "skipped";
export interface ItemResult { key: string; title: string; status: ItemStatus; error: string }
export interface DeleteDeps {
  api: SiteApi; db: Db; pacer: Pacer; now: () => number;
  save: (path: string, text: string, mime: string) => Promise<void>;
  aliasOf: (accountKey: string) => string;
}

const STOP: ErrorCode[] = ["E_BROKEN", "E_AUTH", "E_NO_TAB", "E_NO_AGENT"];
export const ACCOUNT_RECHECK_EVERY = 20;
const code = (e: unknown): string => (e instanceof SiteError ? e.code : "E_NET");
const errorText = (e: unknown): string => (e instanceof SiteError ? e.code : e instanceof Error ? e.message : String(e));

export async function runDeleteJob(items: Conversation[], mode: DeleteMode, deps: DeleteDeps, signal: AbortSignal, onProgress: (results: ItemResult[]) => void): Promise<ItemResult[]> {
  const results: ItemResult[] = items.map((c) => ({ key: c.key, title: c.title, status: "pending", error: "" }));
  const report = () => onProgress(results.map((r) => ({ ...r })));
  const skipRest = (from: number, error: string) => {
    for (let i = from; i < results.length; i++) if (results[i].status === "pending") results[i] = { ...results[i], status: "skipped", error };
  };

  async function exportOne(c: Conversation): Promise<void> {
    // Always the live body: the conversation may have new messages since the last read.
    const fresh = await withPacing(deps.pacer, () => deps.api.read(c.site, c.id), signal);
    await putBody(deps.db, c.key, fresh, deps.now());
    const body: StoredBody = { ...fresh, key: c.key };
    const url = conversationUrl(c.site, c.id);
    const alias = deps.aliasOf(c.account);
    await deps.save(exportFileName(c, "md"), toMarkdown(c, alias, body, mode === "full" ? "full" : "slim", url), "text/markdown");
    if (mode === "full") await deps.save(exportFileName(c, "json"), JSON.stringify(body, null, 2), "application/json");
  }

  const checked = new Map<string, boolean>();
  const processed = new Map<string, number>();
  for (let i = 0; i < items.length; i++) {
    if (signal.aborted) { skipRest(i, "E_CANCELLED"); break; }
    const c = items[i];
    try {
      if (!checked.has(c.account)) {
        const remote = await withPacing(deps.pacer, () => deps.api.account(c.site), signal);
        checked.set(c.account, accountKey(c.site, remote.remoteId) === c.account);
        processed.set(c.account, 0);
      } else if (checked.get(c.account)) {
        const seen = processed.get(c.account) ?? 0;
        if (seen > 0 && seen % ACCOUNT_RECHECK_EVERY === 0) {
          const remote = await withPacing(deps.pacer, () => deps.api.account(c.site), signal);
          checked.set(c.account, accountKey(c.site, remote.remoteId) === c.account);
        }
      }
      if (checked.get(c.account)) processed.set(c.account, (processed.get(c.account) ?? 0) + 1);
      if (!checked.get(c.account)) {
        results[i] = { ...results[i], status: "skipped", error: "E_ACCOUNT" };
        report();
        continue;
      }
      if (mode !== "direct") {
        try { await exportOne(c); } catch (e) {
          const err = code(e);
          if (STOP.includes(err as ErrorCode) || err === "E_CANCELLED" || err === "E_RATE") throw e;
          results[i] = { ...results[i], status: "failed", error: errorText(e) };
          report();
          continue;
        }
      }
      try {
        await withPacing(deps.pacer, () => deps.api.remove(c.site, c.id), signal);
      } catch (e) {
        if (code(e) !== "E_NOT_FOUND") throw e;
      }
      await markRemoved(deps.db, c.key, deps.now());
      results[i] = { ...results[i], status: "done", error: "" };
    } catch (e) {
      const err = code(e);
      if (err === "E_CANCELLED") { skipRest(i, "E_CANCELLED"); report(); break; }
      results[i] = { ...results[i], status: "failed", error: err };
      if (STOP.includes(err as ErrorCode) || err === "E_RATE") { skipRest(i + 1, err); report(); break; }
    }
    report();
  }
  report();
  return results;
}
