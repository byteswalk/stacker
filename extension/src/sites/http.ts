import { SiteError } from "../shared/types";

export interface FetchInit {
  method?: "GET" | "POST" | "PATCH" | "DELETE";
  /** Sent as JSON. */
  body?: unknown;
  /** Sent url-encoded (application/x-www-form-urlencoded) instead of `body`. */
  form?: Record<string, string>;
  headers?: Record<string, string>;
}
/** `text` is the raw reply, for sites that do not answer in plain JSON; `json` is null when the text is not JSON. */
export interface FetchResult { status: number; json: unknown; retryAfter: string | null; text?: string }
export type FetchJson = (url: string, init?: FetchInit) => Promise<FetchResult>;

/** Same-origin request from the site's page, so the page's sign-in cookies apply. */
export const browserFetchJson: FetchJson = async (url, init = {}) => {
  const form = init.form ? new URLSearchParams(init.form).toString() : undefined;
  const json = form === undefined && init.body !== undefined ? JSON.stringify(init.body) : undefined;
  const contentType = form !== undefined ? "application/x-www-form-urlencoded;charset=UTF-8" : json !== undefined ? "application/json" : null;
  let res: Response;
  try {
    res = await fetch(url, {
      method: init.method ?? "GET",
      credentials: "include",
      headers: { ...(contentType ? { "Content-Type": contentType } : {}), ...init.headers },
      body: form ?? json,
    });
  } catch (e) {
    throw new SiteError("E_NET", e instanceof Error ? e.message : String(e));
  }
  const text = await res.text();
  let parsed: unknown = null;
  if (text) { try { parsed = JSON.parse(text); } catch { parsed = null; } }
  return { status: res.status, json: parsed, retryAfter: res.headers.get("Retry-After"), text };
};

export function expectOk(res: FetchResult): unknown {
  if (res.status >= 200 && res.status < 300) return res.json;
  if (res.status === 401 || res.status === 403) throw new SiteError("E_AUTH", String(res.status));
  if (res.status === 404) throw new SiteError("E_NOT_FOUND");
  if (res.status === 429) {
    const seconds = Number(res.retryAfter);
    throw new SiteError("E_RATE", "", Number.isFinite(seconds) && seconds > 0 ? seconds * 1000 : null);
  }
  throw new SiteError("E_HTTP", String(res.status));
}
