import { SiteError, type Message, type RemoteConversation, type Role } from "../shared/types";
import { arr, obj, optStr, str, time } from "./guards";
import { expectOk, type FetchInit } from "./http";
import { NO_PAGE } from "./page";
import type { AdapterFactory } from "./types";

const ORIGIN = "https://grok.com";
const PAGE = 60;

function fileNames(v: unknown): string[] {
  if (!Array.isArray(v)) return [];
  return v
    .map((f) => (typeof f === "object" && f ? optStr((f as Record<string, unknown>).fileName) || optStr((f as Record<string, unknown>).name) : ""))
    .filter(Boolean);
}

function roleOf(sender: string, path: string): Role {
  const s = sender.toLowerCase();
  if (s === "human" || s === "user") return "user";
  if (s === "assistant") return "assistant";
  throw new SiteError("E_BROKEN", path);
}

/**
 * Response ids from the root to the last node, which is the one on screen; other branches are left out.
 * The first message's parent is Grok's own root, which is never among the nodes, so a parent that is
 * not listed ends the walk rather than meaning a broken reply.
 */
export function currentBranchIds(nodes: Record<string, unknown>[]): string[] {
  if (!nodes.length) return [];
  const parentOf = new Map(nodes.map((n, i) => [str(n.responseId, `responseNodes[${i}].responseId`), optStr(n.parentResponseId) || null]));
  const chain: string[] = [];
  let id: string | null = str(nodes[nodes.length - 1].responseId, "responseNodes[-1].responseId");
  while (id && parentOf.has(id) && !chain.includes(id)) {
    chain.push(id);
    id = parentOf.get(id) ?? null;
  }
  return chain.reverse();
}

export const grok: AdapterFactory = (fetchJson, page = NO_PAGE) => {
  async function get(path: string, init: FetchInit = {}): Promise<unknown> {
    return expectOk(await fetchJson(`${ORIGIN}${path}`, init));
  }
  const conversationPath = (id: string) => `/rest/app-chat/conversations/${encodeURIComponent(id)}`;

  return {
    site: "grok",
    origin: ORIGIN,
    conversationUrl: (id) => `${ORIGIN}/c/${id}`,

    /** No known profile endpoint: a one-item listing proves the sign-in, the x-userid cookie names the account. */
    async account() {
      arr(obj(await get("/rest/app-chat/conversations?pageSize=1"), "conversations").conversations, "conversations.conversations");
      return { remoteId: page.cookie("x-userid") || "default", label: "Grok" };
    },

    /** Cursor is the page token Grok returned with the previous page. */
    async list(cursor) {
      const query = new URLSearchParams({ pageSize: String(PAGE) });
      if (cursor) query.set("pageToken", cursor);
      const res = obj(await get(`/rest/app-chat/conversations?${query}`), "page");
      const items: RemoteConversation[] = arr(res.conversations, "page.conversations").map((raw, i) => {
        const c = obj(raw, `conversations[${i}]`);
        const createdAt = time(c.createTime, `conversations[${i}].createTime`);
        return {
          id: str(c.conversationId, `conversations[${i}].conversationId`),
          title: optStr(c.title),
          createdAt,
          updatedAt: c.modifyTime == null ? createdAt : time(c.modifyTime, `conversations[${i}].modifyTime`),
          archived: false,
        };
      });
      return { items, next: optStr(res.nextPageToken) || null };
    },

    async read(id) {
      const tree = obj(await get(`${conversationPath(id)}/response-node`), "response-node");
      const nodes = arr(tree.responseNodes, "responseNodes").map((n, i) => obj(n, `responseNodes[${i}]`));
      const chain = currentBranchIds(nodes);
      // A real conversation always has at least one response node; an empty chain means it is gone.
      if (!chain.length) throw new SiteError("E_NOT_FOUND", id);
      const loaded = obj(await get(`${conversationPath(id)}/load-responses`, { method: "POST", body: { responseIds: chain } }), "load-responses");
      const byId = new Map(arr(loaded.responses, "responses").map((r, i) => {
        const response = obj(r, `responses[${i}]`);
        return [str(response.responseId, `responses[${i}].responseId`), response] as const;
      }));
      const messages = chain.flatMap((rid): Message[] => {
        const r = byId.get(rid);
        if (!r) throw new SiteError("E_BROKEN", `responses.${rid}`);
        const role = roleOf(str(r.sender, `responses.${rid}.sender`), `responses.${rid}.sender`);
        const text = optStr(r.message).trim();
        const attachments = fileNames(r.fileAttachments);
        if (!text && !attachments.length) return [];
        return [{ role, text, at: r.createTime == null ? null : time(r.createTime, `responses.${rid}.createTime`), attachments }];
      });
      const updatedAt = messages.reduce((max, m) => Math.max(max, m.at ?? 0), 0);
      return { id, title: "", updatedAt, messages };
    },

    async remove(id) {
      await get(conversationPath(id), { method: "DELETE" });
    },
  };
};
