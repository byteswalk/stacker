import { SiteError, type SiteId } from "../shared/types";
import { mergeListing, upsertAccount, type Account, type Db } from "./db";
import type { Pacer } from "./pacer";
import { withPacing } from "./pacer";
import type { SiteApi } from "./siteClient";

export interface RefreshResult { account: Account; added: number; updated: number; removed: number; total: number }

/** Re-lists every conversation of the signed-in account; only a finished walk marks missing ones removed. */
export async function refreshIndex(api: SiteApi, db: Db, site: SiteId, pacer: Pacer, onPage: (total: number) => void, now: () => number = Date.now): Promise<RefreshResult> {
  const account = await upsertAccount(db, site, await withPacing(pacer, () => api.account(site)), now());
  const items = [];
  const seen = new Set<string>();
  let cursor: string | null = null;
  do {
    const page = await withPacing(pacer, () => api.list(site, cursor));
    // A server that ignores the paging cursor repeats a page: stop rather than loop or fake completeness.
    if (page.items.length && page.items.every((i) => seen.has(i.id))) throw new SiteError("E_BROKEN", "paging");
    page.items.forEach((i) => seen.add(i.id));
    items.push(...page.items);
    onPage(items.length);
    cursor = page.next;
  } while (cursor);
  const counts = await mergeListing(db, account, items, true, now());
  return { account, ...counts, total: items.length };
}
