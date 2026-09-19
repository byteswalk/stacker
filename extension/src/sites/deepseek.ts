import { SiteError, type Message, type RemoteConversation } from "../shared/types";
import { arr, obj, optStr, str, time } from "./guards";
import { expectOk, type FetchInit } from "./http";
import { NO_PAGE } from "./page";
import type { AdapterFactory } from "./types";

const ORIGIN = "https://chat.deepseek.com";
const PAGE = 100;
/** A failure code whose message is about the sign-in means the token is no longer accepted. */
const AUTH_MESSAGE = /token|auth|login|sign/i;

const idOf = (v: unknown, path: string): string => (typeof v === "number" && Number.isFinite(v) ? String(v) : str(v, path));

function fileNames(v: unknown): string[] {
  return Array.isArray(v) ? v.map((f) => optStr((f as Record<string, unknown>)?.file_name)).filter(Boolean) : [];
}

/** The request and the answer; the model's thinking and other fragments are left out. */
function messageText(m: Record<string, unknown>): string {
  if (Array.isArray(m.fragments) && m.fragments.length) {
    return m.fragments
      .map((f) => (typeof f === "object" && f ? (f as Record<string, unknown>) : {}))
      .filter((f) => f.type === "REQUEST" || f.type === "RESPONSE")
      .map((f) => optStr(f.content))
      .join("\n")
      .trim();
  }
  return optStr(m.content).trim();
}

/** The chain from the message on screen back to the root. */
function branch(messages: Record<string, unknown>[], leaf: string): Record<string, unknown>[] {
  const byId = new Map(messages.map((m, i) => [idOf(m.message_id, `chat_messages[${i}].message_id`), m]));
  const chain: Record<string, unknown>[] = [];
  const seen = new Set<string>();
  let id: string | null = leaf;
  while (id && !seen.has(id)) {
    const m = byId.get(id);
    if (!m) throw new SiteError("E_BROKEN", `chat_messages.${id}`);
    seen.add(id);
    chain.push(m);
    id = m.parent_id == null ? null : idOf(m.parent_id, `chat_messages.${id}.parent_id`);
  }
  return chain.reverse();
}

export const deepseek: AdapterFactory = (fetchJson, page = NO_PAGE) => {
  /** The page keeps its sign-in token in localStorage; it is read for each request and never stored by the extension. */
  function token(): string {
    const raw = page.storage("userToken");
    if (!raw) throw new SiteError("E_AUTH", "no userToken");
    let value: unknown;
    try { value = (JSON.parse(raw) as { value?: unknown } | null)?.value; } catch { throw new SiteError("E_BROKEN", "userToken"); }
    if (typeof value !== "string" || !value) throw new SiteError("E_AUTH", "no userToken");
    return value;
  }

  async function api(path: string, init: FetchInit = {}): Promise<unknown> {
    const headers = { ...init.headers, Authorization: `Bearer ${token()}`, "x-client-platform": "web" };
    const res = obj(expectOk(await fetchJson(`${ORIGIN}${path}`, { ...init, headers })), "response");
    if (res.code !== 0) {
      const msg = optStr(res.msg);
      throw new SiteError(AUTH_MESSAGE.test(msg) ? "E_AUTH" : "E_BROKEN", `code ${String(res.code)}${msg ? `: ${msg}` : ""}`);
    }
    const data = obj(res.data, "data");
    if (data.biz_code !== 0) throw new SiteError("E_BROKEN", `biz_code ${String(data.biz_code)}`);
    return data.biz_data;
  }

  return {
    site: "deepseek",
    origin: ORIGIN,
    conversationUrl: (id) => `${ORIGIN}/a/chat/s/${id}`,

    async account() {
      const me = obj(await api("/api/v0/users/current"), "users/current");
      return { remoteId: idOf(me.id, "users/current.id"), label: "DeepSeek" };
    },

    /** Cursor is `<pinned>|<updated_at>|<id>` of the previous page's last session; the site's cursor is inclusive, so that session is dropped. */
    async list(cursor) {
      const query = new URLSearchParams({ count: String(PAGE) });
      let boundary = "";
      if (cursor) {
        const [pinned, updatedAt, lastId] = cursor.split("|");
        query.set("lte_cursor.pinned", pinned);
        query.set("lte_cursor.updated_at", updatedAt);
        boundary = lastId ?? "";
      }
      const data = obj(await api(`/api/v0/chat_session/fetch_page?${query}`), "fetch_page");
      const rows = arr(data.chat_sessions, "chat_sessions").map((s, i) => obj(s, `chat_sessions[${i}]`));
      const items: RemoteConversation[] = rows
        .map((s, i) => {
          const updatedAt = time(s.updated_at, `chat_sessions[${i}].updated_at`);
          return {
            id: str(s.id, `chat_sessions[${i}].id`),
            title: optStr(s.title),
            createdAt: s.inserted_at == null ? updatedAt : time(s.inserted_at, `chat_sessions[${i}].inserted_at`),
            updatedAt,
            archived: false,
          };
        })
        .filter((c) => c.id !== boundary);
      const last = rows.at(-1);
      const next = data.has_more === true && last ? `${last.pinned === true}|${String(last.updated_at)}|${str(last.id, "chat_sessions[-1].id")}` : null;
      // An inclusive cursor that does not move would page forever.
      if (next !== null && next === cursor) throw new SiteError("E_BROKEN", "paging");
      return { items, next };
    },

    async read(id) {
      const data = obj(await api(`/api/v0/chat/history_messages?chat_session_id=${encodeURIComponent(id)}`), "history_messages");
      const session = obj(data.chat_session, "chat_session");
      const all = arr(data.chat_messages, "chat_messages").map((m, i) => obj(m, `chat_messages[${i}]`));
      const leaf = session.current_message_id == null ? null : idOf(session.current_message_id, "chat_session.current_message_id");
      const selected = leaf ? branch(all, leaf) : all;
      // A real chat session always has at least one message; an empty selection means it is gone.
      if (!selected.length) throw new SiteError("E_NOT_FOUND", id);
      const messages = selected.flatMap((m): Message[] => {
        const path = `chat_messages.${String(m.message_id)}`;
        const role = str(m.role, `${path}.role`).toUpperCase();
        if (role !== "USER" && role !== "ASSISTANT") throw new SiteError("E_BROKEN", `${path}.role`);
        const text = messageText(m);
        const attachments = fileNames(m.files);
        if (!text && !attachments.length) return [];
        return [{ role: role === "USER" ? "user" : "assistant", text, at: m.inserted_at == null ? null : time(m.inserted_at, `${path}.inserted_at`), attachments }];
      });
      return { id, title: optStr(session.title), updatedAt: time(session.updated_at, "chat_session.updated_at"), messages };
    },

    async remove(id) {
      await api("/api/v0/chat_session/delete", { method: "POST", body: { chat_session_id: id } });
    },
  };
};
