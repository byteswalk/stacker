import type { SiteId } from "../shared/types";
import { SITES } from "../sites/registry";

export interface SaveExcerpt { type: "save-excerpt"; site: SiteId; conversationId: string | null; url: string; pageTitle: string; text: string }

/** Runtime shape check on an untrusted `runtime.onMessage` payload before it is written to the DB. */
export function isValidSaveExcerpt(m: unknown): m is SaveExcerpt {
  if (!m || typeof m !== "object") return false;
  const o = m as Record<string, unknown>;
  return o.type === "save-excerpt" && typeof o.site === "string" && o.site in SITES && typeof o.text === "string";
}
