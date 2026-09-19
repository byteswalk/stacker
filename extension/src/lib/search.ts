import type { SiteId } from "../shared/types";
import { bodyIsFresh, type Conversation, type StoredBody } from "./db";

export interface Filter {
  text: string; inBody: boolean; site: SiteId | ""; account: string;
  folder: string; tag: string; from: number | null; to: number | null;
  body: "any" | "read" | "unread"; favorite: boolean; showRemoved: boolean;
}

export const EMPTY_FILTER: Filter = {
  text: "", inBody: false, site: "", account: "", folder: "", tag: "", from: null, to: null,
  body: "any", favorite: false, showRemoved: false,
};

export function bodyText(body: StoredBody): string {
  return body.messages.map((m) => m.text).join("\n").toLowerCase();
}

export function applyFilter(list: Conversation[], f: Filter, bodies: Map<string, string>): Conversation[] {
  const needle = f.text.trim().toLowerCase();
  return list
    .filter((c) => f.showRemoved || c.removedAt === null)
    .filter((c) => !f.site || c.site === f.site)
    .filter((c) => !f.account || c.account === f.account)
    .filter((c) => !f.folder || (f.folder === "none" ? c.folderId === null : c.folderId === f.folder))
    .filter((c) => !f.tag || c.tags.includes(f.tag))
    .filter((c) => !f.favorite || c.favorite)
    .filter((c) => f.from === null || c.updatedAt >= f.from)
    .filter((c) => f.to === null || c.updatedAt <= f.to)
    .filter((c) => f.body === "any" || (f.body === "read") === bodyIsFresh(c))
    .filter((c) => !needle || c.title.toLowerCase().includes(needle) || c.note.toLowerCase().includes(needle)
      || (f.inBody && (bodies.get(c.key) ?? "").includes(needle)))
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

export function allTags(list: Conversation[]): string[] {
  return [...new Set(list.flatMap((c) => c.tags))].sort();
}
