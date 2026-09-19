import type { SiteId } from "../shared/types";
import type { Conversation } from "./db";
import type { ItemResult } from "./deleteJob";

/** Sites whose interface was found changed (E_BROKEN), with the time; cleared by a later successful refresh. */
export const BROKEN_SITES_KEY = "brokenSites";
export type BrokenSites = Partial<Record<SiteId, number>>;
export interface SessionArea {
  get(key: string): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
}

export function createBrokenSites(area: SessionArea = chrome.storage.session) {
  const all = async (): Promise<BrokenSites> => ((await area.get(BROKEN_SITES_KEY))[BROKEN_SITES_KEY] as BrokenSites | undefined) ?? {};
  return {
    all,
    async mark(sites: SiteId[], at: number): Promise<BrokenSites> {
      const next = { ...(await all()) };
      for (const s of sites) next[s] = at;
      if (sites.length) await area.set({ [BROKEN_SITES_KEY]: next });
      return next;
    },
    async clear(site: SiteId): Promise<BrokenSites> {
      const next = { ...(await all()) };
      if (!(site in next)) return next;
      delete next[site];
      await area.set({ [BROKEN_SITES_KEY]: next });
      return next;
    },
  };
}

/** Sites where a delete job itself failed with E_BROKEN (items merely skipped because of it are not counted). */
export function brokenSitesIn(items: Conversation[], results: ItemResult[]): SiteId[] {
  const siteOf = new Map(items.map((c) => [c.key, c.site]));
  const sites = results.filter((r) => r.status === "failed" && r.error === "E_BROKEN").map((r) => siteOf.get(r.key));
  return [...new Set(sites.filter((s): s is SiteId => !!s))];
}
