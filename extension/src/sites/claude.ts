import { SiteError, type Message } from "../shared/types";
import { arr, obj, optStr, str, time } from "./guards";
import { expectOk, type FetchInit } from "./http";
import type { AdapterFactory } from "./types";

const ORIGIN = "https://claude.ai";
const PAGE = 100;

function messageText(m: Record<string, unknown>): string {
  if (Array.isArray(m.content)) {
    const parts = m.content
      .map((b) => (typeof b === "object" && b ? (b as Record<string, unknown>) : {}))
      .filter((b) => b.type === "text")
      .map((b) => optStr(b.text));
    if (parts.length) return parts.join("\n").trim();
  }
  return optStr(m.text).trim();
}

function fileNames(v: unknown): string[] {
  return Array.isArray(v) ? v.map((f) => optStr((f as Record<string, unknown>)?.file_name)).filter(Boolean) : [];
}

/** The chain from the current leaf back to the root; with no leaf, the messages in order. */
function branch(messages: Record<string, unknown>[], leaf: string | null): Record<string, unknown>[] {
  if (!leaf) return messages;
  const byId = new Map(messages.map((m) => [optStr(m.uuid), m]));
  const chain: Record<string, unknown>[] = [];
  const seen = new Set<string>();
  let id: string | null = leaf;
  while (id && byId.has(id) && !seen.has(id)) {
    seen.add(id);
    const m: Record<string, unknown> = byId.get(id)!;
    chain.push(m);
    id = optStr(m.parent_message_uuid) || null;
  }
  if (leaf && chain.length === 0) {
    throw new SiteError("E_BROKEN", "current_leaf_message_uuid");
  }
  return chain.reverse();
}

export const claude: AdapterFactory = (fetchJson) => {
  let org: string | null = null;

  async function get(path: string, init: FetchInit = {}): Promise<unknown> {
    return expectOk(await fetchJson(`${ORIGIN}${path}`, init));
  }
  async function orgId(): Promise<string> {
    if (org) return org;
    const list = arr(await get("/api/organizations"), "organizations").map((o, i) => obj(o, `organizations[${i}]`));
    if (!list.length) throw new SiteError("E_AUTH", "no organization");
    const chat = list.find((o) => Array.isArray(o.capabilities) && o.capabilities.includes("chat")) ?? list[0];
    org = str(chat.uuid, "organizations[].uuid");
    return org;
  }

  return {
    site: "claude",
    origin: ORIGIN,
    conversationUrl: (id) => `${ORIGIN}/chat/${id}`,

    async account() {
      org = null;
      const remoteId = await orgId();
      let label = "Claude";
      try {
        const acc = obj(await get("/api/account"), "account");
        label = optStr(acc.display_name) || optStr(acc.full_name) || "Claude";
      } catch {
        // A failed or unexpected /api/account response only costs the display name.
      }
      return { remoteId, label };
    },

    async list(cursor) {
      const offset = Number(cursor ?? 0) || 0;
      const rows = arr(await get(`/api/organizations/${await orgId()}/chat_conversations?limit=${PAGE}&offset=${offset}`), "chat_conversations");
      const items = rows.map((raw, i) => {
        const c = obj(raw, `chat_conversations[${i}]`);
        return {
          id: str(c.uuid, `chat_conversations[${i}].uuid`),
          title: optStr(c.name),
          createdAt: time(c.created_at, `chat_conversations[${i}].created_at`),
          updatedAt: time(c.updated_at, `chat_conversations[${i}].updated_at`),
          archived: c.is_archived === true,
        };
      });
      // A server that ignores `limit` returns everything at once.
      return { items, next: rows.length === PAGE ? String(offset + PAGE) : null };
    },

    async read(id) {
      const c = obj(await get(`/api/organizations/${await orgId()}/chat_conversations/${encodeURIComponent(id)}?tree=True&rendering_mode=messages&render_all_tools=true`), "conversation");
      const all = arr(c.chat_messages, "conversation.chat_messages").map((m, i) => obj(m, `chat_messages[${i}]`));
      const uuidToIndex = new Map(all.map((m, i) => [optStr(m.uuid), i]));
      const branched = branch(all, optStr(c.current_leaf_message_uuid) || null);
      const messages: Message[] = branched.flatMap((m): Message[] => {
        const originalIndex = uuidToIndex.get(optStr(m.uuid)) ?? 0;
        const originalPath = `chat_messages[${originalIndex}]`;
        const sender = str(m.sender, `${originalPath}.sender`);
        if (sender !== "human" && sender !== "assistant") throw new SiteError("E_BROKEN", `${originalPath}.sender`);
        const text = messageText(m);
        const attachments = [...fileNames(m.attachments), ...fileNames(m.files)];
        if (!text && !attachments.length) return [];
        return [{ role: sender === "human" ? "user" : "assistant", text, at: m.created_at == null ? null : time(m.created_at, `${originalPath}.created_at`), attachments }];
      });
      return { id, title: optStr(c.name), updatedAt: time(c.updated_at, "conversation.updated_at"), messages };
    },

    async remove(id) {
      await get(`/api/organizations/${await orgId()}/chat_conversations/${encodeURIComponent(id)}`, { method: "DELETE" });
    },
  };
};
