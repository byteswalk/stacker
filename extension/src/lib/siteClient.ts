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

export function createSiteApi(chromeApi: Pick<typeof chrome, "tabs"> = chrome): SiteApi {
  async function call<T>(site: SiteId, op: SiteOp, arg: string | null): Promise<T> {
    const tabs = await chromeApi.tabs.query({ url: SITES[site].match });
    const tab = tabs.find((t) => typeof t.id === "number");
    if (!tab?.id) throw new SiteError("E_NO_TAB", site);
    const req: SiteRequest = { type: "site-rpc", site, op, arg };
    let res: SiteResponse | undefined;
    try {
      res = (await chromeApi.tabs.sendMessage(tab.id, req)) as SiteResponse | undefined;
    } catch {
      throw new SiteError("E_NO_AGENT", site);
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
