import type { SiteId } from "../shared/types";
import { conversationIdOfUrl, siteOfUrl } from "../sites/registry";

export interface SiteTab { tabId: number; site: SiteId; url: string; title: string }
export const LAST_TAB_KEY = "lastSiteTab";

export function siteTabOf(tab: { id?: number; url?: string; title?: string }): SiteTab | null {
  if (typeof tab.id !== "number" || !tab.url) return null;
  const site = siteOfUrl(tab.url);
  if (!site || !conversationIdOfUrl(site, tab.url)) return null;
  return { tabId: tab.id, site, url: tab.url, title: tab.title ?? "" };
}
