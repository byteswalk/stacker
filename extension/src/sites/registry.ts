import type { SiteId } from "../shared/types";
import { chatgpt } from "./chatgpt";
import { claude } from "./claude";
import { deepseek } from "./deepseek";
import { gemini, geminiUrl } from "./gemini";
import { grok } from "./grok";
import type { AdapterFactory } from "./types";

export interface SiteInfo {
  label: string;
  factory: AdapterFactory;
  origin: string;
  match: string;
  /** Checked against a real signed-in account. Unverified sites can be refreshed, read and exported, but never deleted from. */
  verified: boolean;
  /** The conversation id in a page path, in the form the adapter uses; null when the page is not a conversation. */
  idOfPath(path: string): string | null;
  /** The page of a conversation. */
  urlOf(id: string): string;
}

const firstGroup = (pattern: RegExp) => (path: string): string | null => pattern.exec(path)?.[1] ?? null;

export const SITES: Record<SiteId, SiteInfo> = {
  chatgpt: {
    label: "ChatGPT", factory: chatgpt, origin: "https://chatgpt.com", match: "https://chatgpt.com/*", verified: true,
    idOfPath: firstGroup(/\/c\/([A-Za-z0-9-]+)/), urlOf: (id) => `https://chatgpt.com/c/${id}`,
  },
  claude: {
    label: "Claude", factory: claude, origin: "https://claude.ai", match: "https://claude.ai/*", verified: true,
    idOfPath: firstGroup(/\/chat\/([A-Za-z0-9-]+)/), urlOf: (id) => `https://claude.ai/chat/${id}`,
  },
  gemini: {
    label: "Gemini", factory: gemini, origin: "https://gemini.google.com", match: "https://gemini.google.com/*", verified: false,
    idOfPath: (path) => {
      const hex = /\/app\/([0-9a-f]+)/.exec(path)?.[1];
      return hex ? `c_${hex}` : null;
    },
    urlOf: geminiUrl,
  },
  grok: {
    label: "Grok", factory: grok, origin: "https://grok.com", match: "https://grok.com/*", verified: false,
    idOfPath: firstGroup(/\/c\/([A-Za-z0-9-]+)/), urlOf: (id) => `https://grok.com/c/${id}`,
  },
  deepseek: {
    label: "DeepSeek", factory: deepseek, origin: "https://chat.deepseek.com", match: "https://chat.deepseek.com/*", verified: false,
    idOfPath: firstGroup(/\/a\/chat\/s\/([A-Za-z0-9-]+)/), urlOf: (id) => `https://chat.deepseek.com/a/chat/s/${id}`,
  },
};

export function siteOfUrl(url: string): SiteId | null {
  const origin = (() => { try { return new URL(url).origin; } catch { return ""; } })();
  return (Object.keys(SITES) as SiteId[]).find((id) => SITES[id].origin === origin) ?? null;
}

export function conversationIdOfUrl(site: SiteId, url: string): string | null {
  const path = (() => { try { return new URL(url).pathname; } catch { return ""; } })();
  return SITES[site].idOfPath(path);
}

export function conversationUrl(site: SiteId, id: string): string {
  return SITES[site].urlOf(id);
}
