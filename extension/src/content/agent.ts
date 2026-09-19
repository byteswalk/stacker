import { serializeError, SiteError, type SerializedError, type SiteId } from "../shared/types";
import { browserFetchJson } from "../sites/http";
import { siteOfUrl, SITES } from "../sites/registry";
import type { Adapter } from "../sites/types";

export type SiteOp = "account" | "list" | "read" | "remove" | "archive";
export interface SiteRequest { type: "site-rpc"; site: SiteId; op: SiteOp; arg: string | null }
export type SiteResponse = { ok: true; value: unknown } | { ok: false; error: SerializedError };

export function createAgent(adapter: Adapter) {
  return async (req: SiteRequest): Promise<SiteResponse> => {
    try {
      if (req.site !== adapter.site) throw new SiteError("E_NO_AGENT", req.site);
      const arg = req.arg ?? "";
      let value: unknown;
      switch (req.op) {
        case "account": value = await adapter.account(); break;
        case "list": value = await adapter.list(req.arg); break;
        case "read": value = await adapter.read(arg); break;
        case "remove": await adapter.remove(arg); value = null; break;
        case "archive":
          if (!adapter.archive) throw new SiteError("E_HTTP", "archive unsupported");
          await adapter.archive(arg); value = null; break;
      }
      return { ok: true, value };
    } catch (e) {
      return { ok: false, error: serializeError(e) };
    }
  };
}

/** Runs in the site's page: requests are same-origin and carry the page's sign-in. */
export function installAgent(): void {
  const site = siteOfUrl(location.href);
  if (!site) return;
  const agent = createAgent(SITES[site].factory(browserFetchJson));
  chrome.runtime.onMessage.addListener((message: unknown, _sender, reply) => {
    const req = message as SiteRequest;
    if (req?.type !== "site-rpc") return false;
    void agent(req).then(reply);
    return true;
  });
}
