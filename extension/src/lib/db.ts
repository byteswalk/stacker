import { openDB, type DBSchema, type IDBPDatabase } from "idb";
import type { RemoteAccount, RemoteBody, RemoteConversation, SiteId } from "../shared/types";
import { SITES } from "../sites/registry";

export interface Account { key: string; site: SiteId; remoteId: string; alias: string; lastSeen: number }
export interface LocalFields { folderId: string | null; tags: string[]; favorite: boolean; note: string }
export interface Conversation extends LocalFields {
  key: string; site: SiteId; account: string; id: string; title: string;
  createdAt: number; updatedAt: number; archived: boolean;
  bodyFetchedAt: number | null; bodyUpdatedAt: number | null; removedAt: number | null;
}
export interface StoredBody extends RemoteBody { key: string }
export interface Folder { id: string; name: string; createdAt: number }
export interface Excerpt { id: string; site: SiteId; conversationId: string | null; url: string; pageTitle: string; text: string; note: string; createdAt: number }

interface Schema extends DBSchema {
  accounts: { key: string; value: Account };
  conversations: { key: string; value: Conversation; indexes: { account: string } };
  bodies: { key: string; value: StoredBody };
  folders: { key: string; value: Folder };
  excerpts: { key: string; value: Excerpt; indexes: { conversation: [SiteId, string] } };
}
export type Db = IDBPDatabase<Schema>;

export function openDb(name = "stacker-web"): Promise<Db> {
  return openDB<Schema>(name, 1, {
    upgrade(db) {
      db.createObjectStore("accounts", { keyPath: "key" });
      db.createObjectStore("conversations", { keyPath: "key" }).createIndex("account", "account");
      db.createObjectStore("bodies", { keyPath: "key" });
      db.createObjectStore("folders", { keyPath: "id" });
      db.createObjectStore("excerpts", { keyPath: "id" }).createIndex("conversation", ["site", "conversationId"]);
    },
  });
}

export const conversationKey = (site: SiteId, id: string) => `${site}:${id}`;
export const accountKey = (site: SiteId, remoteId: string) => `${site}:${remoteId}`;
const newId = () => crypto.randomUUID();

export async function upsertAccount(db: Db, site: SiteId, remote: RemoteAccount, now: number): Promise<Account> {
  const key = accountKey(site, remote.remoteId);
  const existing = await db.get("accounts", key);
  if (existing) {
    const next = { ...existing, lastSeen: now };
    await db.put("accounts", next);
    return next;
  }
  const sameSite = (await db.getAll("accounts")).filter((a) => a.site === site).length;
  const account: Account = { key, site, remoteId: remote.remoteId, alias: sameSite ? `${SITES[site].label} ${sameSite + 1}` : SITES[site].label, lastSeen: now };
  await db.put("accounts", account);
  return account;
}

export const listAccounts = (db: Db) => db.getAll("accounts");

export async function renameAccount(db: Db, key: string, alias: string): Promise<void> {
  const a = await db.get("accounts", key);
  if (a) await db.put("accounts", { ...a, alias: alias.trim() || a.alias });
}

/** Refreshes titles and times, keeps local fields; a complete listing marks the rest removed. */
export async function mergeListing(db: Db, account: Account, items: RemoteConversation[], complete: boolean, now: number) {
  const tx = db.transaction("conversations", "readwrite");
  const counts = { added: 0, updated: 0, removed: 0 };
  const seen = new Set<string>();
  for (const item of items) {
    const key = conversationKey(account.site, item.id);
    seen.add(key);
    const old = await tx.store.get(key);
    if (!old) counts.added++;
    else if (old.updatedAt !== item.updatedAt || old.title !== item.title || old.archived !== item.archived || old.removedAt !== null) counts.updated++;
    await tx.store.put({
      folderId: null, tags: [], favorite: false, note: "", bodyFetchedAt: null, bodyUpdatedAt: null,
      ...old,
      key, site: account.site, account: account.key, id: item.id, title: item.title,
      createdAt: item.createdAt, updatedAt: item.updatedAt, archived: item.archived, removedAt: null,
    });
  }
  if (complete) {
    for (const c of await tx.store.index("account").getAll(account.key)) {
      if (!seen.has(c.key) && c.removedAt === null) {
        counts.removed++;
        await tx.store.put({ ...c, removedAt: now });
      }
    }
  }
  await tx.done;
  return counts;
}

export const listConversations = (db: Db) => db.getAll("conversations");
export const getConversation = (db: Db, key: string) => db.get("conversations", key);

export async function updateLocal(db: Db, keys: string[], patch: Partial<LocalFields>): Promise<void> {
  const tx = db.transaction("conversations", "readwrite");
  for (const key of keys) {
    const c = await tx.store.get(key);
    if (c) await tx.store.put({ ...c, ...patch });
  }
  await tx.done;
}

export async function addTag(db: Db, keys: string[], tag: string): Promise<void> {
  const clean = tag.trim();
  if (!clean) return;
  const tx = db.transaction("conversations", "readwrite");
  for (const key of keys) {
    const c = await tx.store.get(key);
    if (c && !c.tags.includes(clean)) await tx.store.put({ ...c, tags: [...c.tags, clean] });
  }
  await tx.done;
}

export async function putBody(db: Db, key: string, body: RemoteBody, now: number): Promise<void> {
  const tx = db.transaction(["bodies", "conversations"], "readwrite");
  await tx.objectStore("bodies").put({ ...body, key });
  const c = await tx.objectStore("conversations").get(key);
  if (c) await tx.objectStore("conversations").put({ ...c, bodyFetchedAt: now, bodyUpdatedAt: body.updatedAt });
  await tx.done;
}

export const getBody = (db: Db, key: string) => db.get("bodies", key);

export function bodyIsFresh(c: Conversation): boolean {
  return c.bodyUpdatedAt !== null && c.bodyUpdatedAt >= c.updatedAt;
}

export async function markRemoved(db: Db, key: string, now: number): Promise<void> {
  const c = await db.get("conversations", key);
  if (c) await db.put("conversations", { ...c, removedAt: now });
}

export const listFolders = (db: Db) => db.getAll("folders");

export async function createFolder(db: Db, name: string, now: number): Promise<Folder> {
  const folder = { id: newId(), name: name.trim(), createdAt: now };
  await db.put("folders", folder);
  return folder;
}

export async function renameFolder(db: Db, id: string, name: string): Promise<void> {
  const f = await db.get("folders", id);
  if (f && name.trim()) await db.put("folders", { ...f, name: name.trim() });
}

export async function deleteFolder(db: Db, id: string): Promise<void> {
  const tx = db.transaction(["folders", "conversations"], "readwrite");
  await tx.objectStore("folders").delete(id);
  for (const c of await tx.objectStore("conversations").getAll()) {
    if (c.folderId === id) await tx.objectStore("conversations").put({ ...c, folderId: null });
  }
  await tx.done;
}

export async function addExcerpt(db: Db, e: Omit<Excerpt, "id" | "createdAt">, now: number): Promise<Excerpt> {
  const excerpt = { ...e, id: newId(), createdAt: now };
  await db.put("excerpts", excerpt);
  return excerpt;
}

export async function listExcerpts(db: Db, site?: SiteId, conversationId?: string): Promise<Excerpt[]> {
  let all: Excerpt[];
  if (site && conversationId) {
    all = await db.getAllFromIndex("excerpts", "conversation", [site, conversationId]);
  } else if (site) {
    const allExcerpts = await db.getAll("excerpts");
    all = allExcerpts.filter((e) => e.site === site);
  } else {
    all = await db.getAll("excerpts");
  }
  return all.sort((a, b) => b.createdAt - a.createdAt);
}

export const deleteExcerpt = (db: Db, id: string) => db.delete("excerpts", id);
