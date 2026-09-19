import { reviveError, SiteError, type ListPage, type RemoteAccount, type RemoteBody, type SiteId } from "../shared/types";
import type { SiteOp, SiteRequest, SiteResponse } from "../content/agent";
import { SITES } from "../sites/registry";

export interface SiteApi {
  account(site: SiteId): Promise<RemoteAccount>;
  list(site: SiteId, cursor: string | null): Promise<ListPage>;
  read(site: SiteId, id: string): Promise<RemoteBody>;
  remove(site: SiteId, id: string): Promise<void>;
  archive(site: SiteId, id: string): Promise<void>;
  canArchive(site: SiteId): boolean;
}

/** Tab ids worth trying, best first: skips discarded tabs, prefers the active tab, then fully loaded ones. */
export function usableTabs(tabs: chrome.tabs.Tab[]): number[] {
  const rank = (t: chrome.tabs.Tab) => (t.active ? 0 : 2) + (t.status === "complete" ? 0 : 1);
  return tabs
    .filter((t) => typeof t.id === "number" && !t.discarded)
    .sort((a, b) => rank(a) - rank(b))
    .map((t) => t.id as number);
}

export function createSiteApi(chromeApi: Pick<typeof chrome, "tabs"> = chrome): SiteApi {
  async function call<T>(site: SiteId, op: SiteOp, arg: string | null): Promise<T> {
    const candidates = usableTabs(await chromeApi.tabs.query({ url: SITES[site].match }));
    if (!candidates.length) throw new SiteError("E_NO_TAB", site);
    const req: SiteRequest = { type: "site-rpc", site, op, arg };
    let res: SiteResponse | undefined;
    for (const id of candidates) {
      try {
        res = (await chromeApi.tabs.sendMessage(id, req)) as SiteResponse | undefined;
      } catch {
        res = undefined;
      }
      if (res) break;
    }
    if (!res) throw new SiteError("E_NO_AGENT", site);
    if (!res.ok) throw reviveError(res.error);
    return res.value as T;
  }
  return {
    account: (site) => call(site, "account", null),
    list: (site, cursor) => call(site, "list", cursor),
    read: (site, id) => call(site, "read", id),
    remove: async (site, id) => { await call(site, "remove", id); },
    archive: async (site, id) => { await call(site, "archive", id); },
    canArchive: (site) => site === "chatgpt",
  };
}
