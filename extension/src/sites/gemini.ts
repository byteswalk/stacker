import { SiteError, type Message, type RemoteConversation } from "../shared/types";
import { batchForm, batchUrl, extractTokens, GEMINI_ORIGIN, parseBatch, type GeminiTokens } from "./batchexecute";
import { arr, optStr, str } from "./guards";
import { expectOk } from "./http";
import type { AdapterFactory } from "./types";

const PAGE = 100;
/** Turns asked for per read; chats seen live had far fewer, so one page is the usual case. */
const TURNS = 1000;
const MAX_READ_PAGES = 50;
const RPC = { list: "MaZiqc", read: "hNvQHb", remove: "GzXR5e" } as const;

/** Gemini's chat ids carry a "c_" prefix that its page URLs leave out. */
export function geminiUrl(id: string): string {
  return `${GEMINI_ORIGIN}/app/${id.replace(/^c_/, "")}`;
}

/** Follows array indexes, failing as E_BROKEN at the first step that is not an array. */
function dig(v: unknown, path: string, ...indexes: number[]): unknown {
  let cur = v;
  let at = path;
  for (const i of indexes) {
    cur = arr(cur, at)[i];
    at = `${at}[${i}]`;
  }
  return cur;
}

/** [unixSeconds, nanos] → milliseconds. */
function stamp(v: unknown, path: string): number {
  const [seconds, nanos] = arr(v, path);
  if (typeof seconds !== "number" || !Number.isFinite(seconds)) throw new SiteError("E_BROKEN", path);
  return seconds * 1000 + (typeof nanos === "number" ? Math.floor(nanos / 1e6) : 0);
}

/** One turn is the user's prompt and the first candidate of the model's answer. */
function turnMessages(turn: unknown, path: string): Message[] {
  const at = stamp(dig(turn, path, 4), `${path}[4]`);
  const prompt = str(dig(turn, path, 2, 0, 0), `${path}[2][0][0]`).trim();
  const answer = str(dig(turn, path, 3, 0, 0, 1, 0), `${path}[3][0][0][1][0]`).trim();
  const out: Message[] = [];
  if (prompt) out.push({ role: "user", text: prompt, at, attachments: [] });
  if (answer) out.push({ role: "assistant", text: answer, at, attachments: [] });
  return out;
}

export const gemini: AdapterFactory = (fetchJson) => {
  let tokens: GeminiTokens | null = null;

  async function loadTokens(): Promise<GeminiTokens> {
    const res = await fetchJson(`${GEMINI_ORIGIN}/app`);
    expectOk(res);
    tokens = extractTokens(res.text ?? "");
    return tokens;
  }

  async function rpc(id: string, payload: unknown, retried = false): Promise<unknown> {
    const current = tokens ?? (await loadTokens());
    const reqId = 10_000 + Math.floor(Math.random() * 90_000);
    const res = await fetchJson(batchUrl(id, current, reqId), { method: "POST", form: batchForm(id, payload, current), headers: { "X-Same-Domain": "1" } });
    try {
      expectOk(res);
    } catch (e) {
      // Page tokens expire while a tab stays open; the site then answers 400 or 401. Fetch fresh ones once.
      const stale = e instanceof SiteError && (e.code === "E_AUTH" || (e.code === "E_HTTP" && e.detail === "400"));
      if (!stale || retried) throw e;
      tokens = null;
      return rpc(id, payload, true);
    }
    return parseBatch(res.text ?? "", id);
  }

  return {
    site: "gemini",
    origin: GEMINI_ORIGIN,
    conversationUrl: geminiUrl,

    async account() {
      const fresh = await loadTokens();
      return { remoteId: fresh.userId, label: "Gemini" };
    },

    /** Cursor is the page token Gemini returned with the previous page. */
    async list(cursor) {
      const reply = arr(await rpc(RPC.list, [PAGE, cursor, [0, null, 1]]), "list");
      const chats = reply[2] == null ? [] : arr(reply[2], "list[2]");
      const items: RemoteConversation[] = chats.map((raw, i) => {
        const path = `list[2][${i}]`;
        const id = str(dig(raw, path, 0), `${path}[0]`);
        if (!id.startsWith("c_")) throw new SiteError("E_BROKEN", `${path}[0]`);
        // The listing carries one time only: the last activity.
        const at = stamp(dig(raw, path, 5), `${path}[5]`);
        return { id, title: optStr(dig(raw, path, 1)), createdAt: at, updatedAt: at, archived: false };
      });
      const next = typeof reply[1] === "string" && reply[1] ? reply[1] : null;
      return { items, next };
    },

    async read(id) {
      const turns: unknown[] = [];
      const pageTokens = new Set<string>();
      let pageToken: string | null = null;
      for (let n = 0; n < MAX_READ_PAGES; n++) {
        const reply = await rpc(RPC.read, [id, TURNS, pageToken, 1, [0], [4], null, 1]);
        if (reply == null) throw new SiteError("E_NOT_FOUND", id);
        const batch = dig(reply, "read", 0);
        const got = batch == null ? [] : arr(batch, "read[0]");
        turns.push(...got);
        const next = dig(reply, "read", 1);
        if (typeof next !== "string" || !next || !got.length || pageTokens.has(next)) break;
        pageTokens.add(next);
        pageToken = next;
      }
      // Gemini sends the newest turn first.
      const messages = turns.map((turn, i) => turnMessages(turn, `read[0][${i}]`)).reverse().flat();
      const updatedAt = messages.reduce((max, m) => Math.max(max, m.at ?? 0), 0);
      return { id, title: "", updatedAt, messages };
    },

    async remove(id) {
      await rpc(RPC.remove, [id]);
    },
  };
};
