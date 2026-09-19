import { SiteError } from "../shared/types";

export const GEMINI_ORIGIN = "https://gemini.google.com";

/** The page tokens every batchexecute call needs, and the signed-in user's id. */
export interface GeminiTokens { at: string; bl: string; fsid: string; userId: string }

/** One string value of WIZ_global_data as it appears in the page's HTML, with JSON escapes decoded. */
function field(html: string, key: string): string | null {
  const m = new RegExp(String.raw`"${key}":"((?:[^"\\]|\\.)*)"`).exec(html);
  if (!m) return null;
  try { return JSON.parse(`"${m[1]}"`) as string; } catch { return null; }
}

/**
 * Reads the tokens out of the /app HTML: a content script runs in an isolated world and cannot see the page's
 * WIZ_global_data. Only these four keys are read; the email (oPEP7c) never is.
 */
export function extractTokens(html: string): GeminiTokens {
  const at = field(html, "SNlM0e");
  if (!at) throw new SiteError("E_AUTH", "no SNlM0e");
  const need = (key: string): string => {
    const value = field(html, key);
    if (!value) throw new SiteError("E_BROKEN", key);
    return value;
  };
  return { at, bl: need("cfb2h"), fsid: need("FdrFJe"), userId: need("S06Grb") };
}

export function batchUrl(rpcId: string, tokens: GeminiTokens, reqId: number): string {
  const query = new URLSearchParams({ rpcids: rpcId, "source-path": "/app", bl: tokens.bl, "f.sid": tokens.fsid, hl: "en", _reqid: String(reqId), rt: "c" });
  return `${GEMINI_ORIGIN}/_/BardChatUi/data/batchexecute?${query}`;
}

export function batchForm(rpcId: string, payload: unknown, tokens: GeminiTokens): Record<string, string> {
  return { "f.req": JSON.stringify([[[rpcId, JSON.stringify(payload), null, "generic"]]]), at: tokens.at };
}

/** The rpc's result: the "wrb.fr" entry for `rpcId`, its JSON string parsed; null when the rpc returned nothing. */
export function parseBatch(text: string, rpcId: string): unknown {
  for (const line of text.split("\n")) {
    if (!line.includes('"wrb.fr"')) continue;
    let outer: unknown;
    try { outer = JSON.parse(line); } catch { continue; }
    if (!Array.isArray(outer)) continue;
    for (const entry of outer) {
      if (!Array.isArray(entry) || entry[0] !== "wrb.fr" || entry[1] !== rpcId) continue;
      if (typeof entry[2] === "string") {
        try { return JSON.parse(entry[2]); } catch { throw new SiteError("E_BROKEN", `${rpcId} reply`); }
      }
      if (Array.isArray(entry[5]) && entry[5].length) throw new SiteError("E_HTTP", `${rpcId} status ${JSON.stringify(entry[5])}`);
      return null;
    }
  }
  throw new SiteError("E_BROKEN", `${rpcId} reply`);
}
