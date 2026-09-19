import type { SiteId } from "../shared/types";
import { chatgpt } from "./chatgpt";
import { claude } from "./claude";
import type { AdapterFactory } from "./types";

export const SITES: Record<SiteId, { label: string; factory: AdapterFactory; origin: string; match: string }> = {
  chatgpt: { label: "ChatGPT", factory: chatgpt, origin: "https://chatgpt.com", match: "https://chatgpt.com/*" },
  claude: { label: "Claude", factory: claude, origin: "https://claude.ai", match: "https://claude.ai/*" },
};

export function siteOfUrl(url: string): SiteId | null {
  const origin = (() => { try { return new URL(url).origin; } catch { return ""; } })();
  return (Object.keys(SITES) as SiteId[]).find((id) => SITES[id].origin === origin) ?? null;
}

const ID_PATTERN: Record<SiteId, RegExp> = {
  chatgpt: /\/c\/([A-Za-z0-9-]+)/,
  claude: /\/chat\/([A-Za-z0-9-]+)/,
};

export function conversationIdOfUrl(site: SiteId, url: string): string | null {
  const path = (() => { try { return new URL(url).pathname; } catch { return ""; } })();
  return ID_PATTERN[site].exec(path)?.[1] ?? null;
}

const URL_OF: Record<SiteId, (id: string) => string> = {
  chatgpt: (id) => `https://chatgpt.com/c/${id}`,
  claude: (id) => `https://claude.ai/chat/${id}`,
};

export function conversationUrl(site: SiteId, id: string): string {
  return URL_OF[site](id);
}
