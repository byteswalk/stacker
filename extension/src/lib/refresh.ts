import type { SiteId } from "../shared/types";
import { mergeListing, upsertAccount, type Account, type Db } from "./db";
import type { Pacer } from "./pacer";
import { withPacing } from "./pacer";
import type { SiteApi } from "./siteClient";

export interface RefreshResult { account: Account; added: number; updated: number; removed: number; total: number }

/** Re-lists every conversation of the signed-in account; only a finished walk marks missing ones removed. */
export async function refreshIndex(api: SiteApi, db: Db, site: SiteId, pacer: Pacer, onPage: (total: number) => void, now: () => number = Date.now): Promise<RefreshResult> {
  const account = await upsertAccount(db, site, await withPacing(pacer, () => api.account(site)), now());
  const items = [];
  let cursor: string | null = null;
  do {
    const page = await withPacing(pacer, () => api.list(site, cursor));
    items.push(...page.items);
    onPage(items.length);
    cursor = page.next;
  } while (cursor);
  const counts = await mergeListing(db, account, items, true, now());
  return { account, ...counts, total: items.length };
}
