import { SiteError, type Message, type RemoteConversation, type Role } from "../shared/types";
import { arr, bool, obj, optStr, str, time } from "./guards";
import { expectOk, type FetchInit } from "./http";
import type { AdapterFactory } from "./types";

const ORIGIN = "https://chatgpt.com";
const PAGE = 100;
const ROLES: Role[] = ["user", "assistant", "system", "tool"];

/** Messages from the root to the node on screen; other branches are left out. */
export function currentBranch(mappingValue: unknown, currentNode: string): Message[] {
  const mapping = obj(mappingValue, "mapping");
  const chain: Record<string, unknown>[] = [];
  const seen = new Set<string>();
  let id: string | null = currentNode;
  while (id && !seen.has(id)) {
    seen.add(id);
    const node = obj(mapping[id], `mapping.${id}`);
    chain.push(node);
    id = typeof node.parent === "string" ? node.parent : null;
  }
  return chain.reverse().flatMap((node): Message[] => {
    if (node.message == null) return [];
    const message = obj(node.message, "message");
    const metadata = typeof message.metadata === "object" && message.metadata ? (message.metadata as Record<string, unknown>) : {};
    if (metadata.is_visually_hidden_from_conversation === true) return [];
    const role = str(obj(message.author, "message.author").role, "message.author.role") as Role;
    if (!ROLES.includes(role)) throw new SiteError("E_BROKEN", "message.author.role");
    const content = obj(message.content, "message.content");
    const parts = Array.isArray(content.parts) ? content.parts : [];
    const text = [...parts.filter((p): p is string => typeof p === "string"), optStr(content.text)].filter(Boolean).join("\n").trim();
    const attachments = Array.isArray(metadata.attachments)
      ? metadata.attachments.map((a) => optStr((a as Record<string, unknown>)?.name)).filter(Boolean)
      : [];
    if (!text && !attachments.length) return [];
    return [{ role, text, at: message.create_time == null ? null : time(message.create_time, "message.create_time"), attachments }];
  });
}

export const chatgpt: AdapterFactory = (fetchJson) => {
  let token: string | null = null;

  async function auth(): Promise<string> {
    if (token) return token;
    const session = obj(expectOk(await fetchJson(`${ORIGIN}/api/auth/session`)), "session");
    if (typeof session.accessToken !== "string" || !session.accessToken) throw new SiteError("E_AUTH", "no session");
    token = session.accessToken;
    return token;
  }
  async function api(path: string, init: FetchInit = {}): Promise<unknown> {
    const bearer = await auth();
    return expectOk(await fetchJson(`${ORIGIN}${path}`, { ...init, headers: { ...init.headers, Authorization: `Bearer ${bearer}` } }));
  }

  return {
    site: "chatgpt",
    origin: ORIGIN,
    conversationUrl: (id) => `${ORIGIN}/c/${id}`,

    async account() {
      const session = obj(expectOk(await fetchJson(`${ORIGIN}/api/auth/session`)), "session");
      if (typeof session.accessToken !== "string" || !session.accessToken) throw new SiteError("E_AUTH", "no session");
      token = session.accessToken;
      return { remoteId: str(obj(session.user, "session.user").id, "session.user.id"), label: "ChatGPT" };
    },

    /** Cursor is `live:<offset>` or `archived:<offset>`; live pages come first. */
    async list(cursor) {
      const [phase, offsetText] = (cursor ?? "live:0").split(":");
      const offset = Number(offsetText) || 0;
      const archived = phase === "archived";
      const page = obj(await api(`/backend-api/conversations?offset=${offset}&limit=${PAGE}&order=updated${archived ? "&is_archived=true" : ""}`), "page");
      const items: RemoteConversation[] = arr(page.items, "page.items").map((raw, i) => {
        const item = obj(raw, `items[${i}]`);
        return {
          id: str(item.id, `items[${i}].id`),
          title: optStr(item.title),
          createdAt: time(item.create_time, `items[${i}].create_time`),
          updatedAt: time(item.update_time, `items[${i}].update_time`),
          archived: archived || bool(item.is_archived),
        };
      });
      const total = typeof page.total === "number" ? page.total : offset + items.length;
      const more = items.length > 0 && offset + items.length < total;
      const next = more ? `${phase}:${offset + items.length}` : archived ? null : "archived:0";
      return { items, next };
    },

    async read(id) {
      const conv = obj(await api(`/backend-api/conversation/${encodeURIComponent(id)}`), "conversation");
      return {
        id,
        title: optStr(conv.title),
        updatedAt: time(conv.update_time, "conversation.update_time"),
        messages: currentBranch(conv.mapping, str(conv.current_node, "conversation.current_node")),
      };
    },

    async remove(id) {
      await api(`/backend-api/conversation/${encodeURIComponent(id)}`, { method: "PATCH", body: { is_visible: false } });
    },

    async archive(id) {
      await api(`/backend-api/conversation/${encodeURIComponent(id)}`, { method: "PATCH", body: { is_archived: true } });
    },
  };
};
