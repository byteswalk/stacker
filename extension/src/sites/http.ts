import { SiteError } from "../shared/types";

export interface FetchInit { method?: "GET" | "POST" | "PATCH" | "DELETE"; body?: unknown; headers?: Record<string, string> }
export interface FetchResult { status: number; json: unknown; retryAfter: string | null }
export type FetchJson = (url: string, init?: FetchInit) => Promise<FetchResult>;

/** Same-origin request from the site's page, so the page's sign-in cookies apply. */
export const browserFetchJson: FetchJson = async (url, init = {}) => {
  let res: Response;
  try {
    res = await fetch(url, {
      method: init.method ?? "GET",
      credentials: "include",
      headers: { ...(init.body !== undefined ? { "Content-Type": "application/json" } : {}), ...init.headers },
      body: init.body !== undefined ? JSON.stringify(init.body) : undefined,
    });
  } catch (e) {
    throw new SiteError("E_NET", e instanceof Error ? e.message : String(e));
  }
  const text = await res.text();
  let json: unknown = null;
  if (text) { try { json = JSON.parse(text); } catch { json = null; } }
  return { status: res.status, json, retryAfter: res.headers.get("Retry-After") };
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
