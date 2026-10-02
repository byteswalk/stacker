import { PENDING_MS, siteOf, type LoginChoice, type LoginMessage, type PageAnswer } from "./messages";

/** The content script for logins runs on every site, but only once the user grants that. */
export const LOGIN_SCRIPT_ID = "stacker-logins";
export const LOGIN_MATCHES = ["https://*/*", "http://*/*"];
const NEVER_KEY = "loginNever";

interface Store {
  get(key: string): Promise<unknown>;
  set(key: string, value: unknown): Promise<void>;
  remove(key: string): Promise<void>;
}

export interface LoginDeps {
  call(type: string, payload: unknown): Promise<unknown>;
  /** Per browser session, in memory: a submitted login waiting for an answer. */
  session: Store;
  /** Kept: the sites the user said never to ask about. */
  local: Store;
  now(): number;
}

export interface LoginSender { id?: string; url?: string; tab?: { id?: number; title?: string } }

interface Pending { host: string; url: string; user: string; password: string; title: string; at: number }

const pendingKey = (tabId: number) => `loginPending:${tabId}`;

/**
 * The background's half of logins. The page's address always comes from the browser's own
 * record of the sending tab, never from what the page says, so a site can only ever ask for
 * or offer its own logins.
 */
export function createLoginHandler(deps: LoginDeps, extensionId: string) {
  async function logins(url: string): Promise<LoginChoice[]> {
    try {
      const result = (await deps.call("loginsFor", { url })) as { logins?: LoginChoice[] };
      return Array.isArray(result?.logins) ? result.logins : [];
    } catch {
      return [];
    }
  }
  async function never(): Promise<string[]> {
    const list = await deps.local.get(NEVER_KEY);
    return Array.isArray(list) ? list.filter((item): item is string => typeof item === "string") : [];
  }
  async function pending(tabId: number, host: string): Promise<Pending | null> {
    const item = (await deps.session.get(pendingKey(tabId))) as Pending | undefined;
    if (!item || item.host !== host || deps.now() - item.at > PENDING_MS) return null;
    return item;
  }

  return async function handle(message: LoginMessage, sender: LoginSender): Promise<unknown> {
    const host = siteOf(sender.url);
    const tabId = sender.tab?.id;
    if (sender.id !== extensionId || !host || typeof tabId !== "number" || !sender.url) return { ok: false, error: "E_REQUEST" };
    const url = sender.url;

    switch (message.type) {
      case "logins-page": {
        const waiting = await pending(tabId, host);
        const answer: PageAnswer = { pending: waiting && { user: waiting.user, host }, logins: await logins(url) };
        return answer;
      }
      case "logins-captured": {
        if ((await never()).includes(host)) return { prompt: false };
        // Already kept with this password: nothing to ask.
        const known = await logins(url);
        if (known.some((item) => item.user === message.user)) {
          try {
            const kept = (await deps.call("loginPassword", { url, user: message.user })) as { password?: string };
            if (kept?.password === message.password) return { prompt: false };
          } catch { /* not readable: ask */ }
        }
        const item: Pending = { host, url, user: message.user, password: message.password, title: message.title.slice(0, 200), at: deps.now() };
        await deps.session.set(pendingKey(tabId), item);
        return { prompt: true };
      }
      case "logins-decide": {
        const waiting = await pending(tabId, host);
        await deps.session.remove(pendingKey(tabId));
        if (!waiting) return { ok: false, error: "E_EXPIRED" };
        if (message.choice === "never") {
          await deps.local.set(NEVER_KEY, [...new Set([...(await never()), host])]);
          return { ok: true };
        }
        if (message.choice === "dismiss") return { ok: true };
        try {
          await deps.call("loginSave", { url: waiting.url, user: waiting.user, password: waiting.password, title: waiting.title, fill: message.choice === "save-fill" });
          return { ok: true };
        } catch (e) {
          return { ok: false, error: e instanceof Error ? e.message : String(e) };
        }
      }
      case "logins-fill": {
        try {
          const result = (await deps.call("loginPassword", { url, user: message.user })) as { password?: string };
          return typeof result?.password === "string" ? { ok: true, password: result.password } : { ok: false };
        } catch {
          return { ok: false };
        }
      }
    }
  };
}

/** Registers the content script while the all-sites permission is granted, and removes it once it is not. */
export async function syncLoginScript(): Promise<void> {
  const granted = await chrome.permissions.contains({ origins: LOGIN_MATCHES });
  const existing = await chrome.scripting.getRegisteredContentScripts({ ids: [LOGIN_SCRIPT_ID] });
  if (granted && !existing.length) {
    await chrome.scripting.registerContentScripts([{ id: LOGIN_SCRIPT_ID, matches: LOGIN_MATCHES, js: ["logins.js"], runAt: "document_idle", allFrames: false, persistAcrossSessions: true }]);
  } else if (!granted && existing.length) {
    await chrome.scripting.unregisterContentScripts({ ids: [LOGIN_SCRIPT_ID] });
  }
}
