import type { Message } from "../shared/types";
import {
  dropOutbox, getBody, getConversation, listAccounts, listConversations, listExcerpts, listFolders, readOutbox,
  type Account, type Conversation, type Db, type Excerpt, type Folder, type OutboxEntry, type OutboxKind, type StoredBody,
} from "./db";

export type Call = (type: string, payload: unknown) => Promise<unknown>;

/** Records per message, as Stacker accepts them. */
export const MAX_BATCH = 200;
/** Stays under Chrome's 1 MB native-message limit with room for the envelope. */
export const MAX_BYTES = 900_000;
/** A single message longer than this is clipped so its chunk still fits. */
export const MAX_MESSAGE_CHARS = 250_000;
export const FLUSH_LIMIT = 2000;

const encoder = new TextEncoder();
export const byteSize = (value: unknown) => encoder.encode(JSON.stringify(value)).length;
const stamp = (value: number | undefined) => value ?? 0;

export function batches<T>(items: T[], max = MAX_BATCH, bytes = MAX_BYTES): T[][] {
  const out: T[][] = [];
  let current: T[] = [];
  let size = 0;
  for (const item of items) {
    const itemSize = byteSize(item) + 1;
    if (current.length && (current.length >= max || size + itemSize > bytes)) {
      out.push(current);
      current = [];
      size = 0;
    }
    current.push(item);
    size += itemSize;
  }
  if (current.length) out.push(current);
  return out;
}

const wireAccount = (a: Account) => ({
  key: a.key, site: a.site, remoteId: a.remoteId, name: a.name, alias: a.alias, lastSeen: a.lastSeen, localUpdatedAt: stamp(a.localUpdatedAt),
});
const wireConversation = (c: Conversation) => ({
  key: c.key, site: c.site, account: c.account, id: c.id, title: c.title, createdAt: c.createdAt, updatedAt: c.updatedAt,
  archived: c.archived, removedAt: c.removedAt, listedAt: stamp(c.listedAt), folderId: c.folderId, tags: c.tags,
  favorite: c.favorite, note: c.note, localUpdatedAt: stamp(c.localUpdatedAt),
});
const wireFolder = (f: Folder) => ({ id: f.id, name: f.name, createdAt: f.createdAt, localUpdatedAt: stamp(f.localUpdatedAt) });
const wireExcerpt = (e: Excerpt) => ({
  id: e.id, site: e.site, conversationId: e.conversationId, url: e.url, pageTitle: e.pageTitle, text: e.text, note: e.note,
  createdAt: e.createdAt, localUpdatedAt: stamp(e.localUpdatedAt),
});

export interface BodyChunk {
  key: string; site: string; account: string; id: string; title: string; updatedAt: number; fetchedAt: number;
  chunk: number; chunks: number; messages: Message[];
}

const clip = (m: Message): Message =>
  m.text.length > MAX_MESSAGE_CHARS ? { ...m, text: `${m.text.slice(0, MAX_MESSAGE_CHARS)}\n…[truncated]` } : m;

/** One `syncBody` message per chunk; a long body is split by message index so each chunk fits. */
export function bodyChunks(c: Conversation, body: StoredBody, fetchedAt: number): BodyChunk[] {
  const head = { key: c.key, site: c.site, account: c.account, id: c.id, title: body.title, updatedAt: body.updatedAt, fetchedAt };
  const groups = batches(body.messages.map(clip), Number.MAX_SAFE_INTEGER, MAX_BYTES - byteSize(head) - 100);
  const parts = groups.length ? groups : [[]];
  return parts.map((messages, chunk) => ({ ...head, chunk, chunks: parts.length, messages }));
}

type BatchType = "syncAccounts" | "syncFolders" | "syncConversations" | "syncExcerpts" | "removeRecords";
export type Outgoing =
  | { type: BatchType; payload: { items: unknown[] }; seqs: number[] }
  | { type: "syncBody"; key: string; seqs: number[] };

const isDefined = <T,>(value: T | undefined): value is T => value !== undefined;

/** Turns queued keys into messages, reading each record as it is now. Order matters: folders and conversations before bodies. */
export async function buildRequests(db: Db, entries: OutboxEntry[]): Promise<Outgoing[]> {
  const all = entries.some((e) => e.kind === "all");
  const keysOf = (kind: OutboxKind) => [...new Set(entries.filter((e) => e.kind === kind).map((e) => e.key))];
  const seqsOf = (kinds: OutboxKind[], keys: string[]) =>
    entries.filter((e) => kinds.includes(e.kind) && keys.includes(e.key)).map((e) => e.seq!);
  const out: Outgoing[] = [];
  const addBatches = <T,>(type: BatchType, kinds: OutboxKind[], records: T[], keyOf: (r: T) => string, wire: (r: T) => unknown) => {
    for (const group of batches(records.map((r) => ({ key: keyOf(r), wire: wire(r) })))) {
      out.push({ type, payload: { items: group.map((g) => g.wire) }, seqs: seqsOf(kinds, group.map((g) => g.key)) });
    }
  };

  const accounts = all ? await listAccounts(db) : (await Promise.all(keysOf("account").map((k) => db.get("accounts", k)))).filter(isDefined);
  addBatches("syncAccounts", ["account"], accounts, (a) => a.key, wireAccount);
  const folders = all ? await listFolders(db) : (await Promise.all(keysOf("folder").map((k) => db.get("folders", k)))).filter(isDefined);
  addBatches("syncFolders", ["folder"], folders, (f) => f.id, wireFolder);
  const conversations = all ? await listConversations(db) : (await Promise.all(keysOf("conversation").map((k) => getConversation(db, k)))).filter(isDefined);
  addBatches("syncConversations", ["conversation"], conversations, (c) => c.key, wireConversation);
  const bodyKeys = all ? conversations.filter((c) => c.bodyFetchedAt !== null).map((c) => c.key) : keysOf("body");
  for (const key of bodyKeys) out.push({ type: "syncBody", key, seqs: seqsOf(["body"], [key]) });
  const excerpts = all ? await listExcerpts(db) : (await Promise.all(keysOf("excerpt").map((k) => db.get("excerpts", k)))).filter(isDefined);
  addBatches("syncExcerpts", ["excerpt"], excerpts, (e) => e.id, wireExcerpt);
  const removals = entries
    .filter((e) => e.kind === "removeFolder" || e.kind === "removeExcerpt")
    .map((e) => ({ kind: e.kind === "removeFolder" ? "folder" : "excerpt", key: e.key, at: e.at, seq: e.seq! }));
  for (const group of batches(removals)) {
    out.push({ type: "removeRecords", payload: { items: group.map(({ kind, key, at }) => ({ kind, key, at })) }, seqs: group.map((r) => r.seq) });
  }

  const allSeqs = entries.filter((e) => e.kind === "all").map((e) => e.seq!);
  if (allSeqs.length && out.length) out[out.length - 1].seqs.push(...allSeqs);
  return out;
}

async function loadBodyChunks(db: Db, key: string): Promise<BodyChunk[]> {
  const [c, body] = await Promise.all([getConversation(db, key), getBody(db, key)]);
  return c && body ? bodyChunks(c, body, c.bodyFetchedAt ?? Date.now()) : [];
}

/**
 * Sends queued changes to Stacker in order. Each message's queue entries are dropped only after
 * Stacker accepted it; the first failure stops the flush and leaves the rest queued.
 */
export async function flush(db: Db, call: Call, limit = FLUSH_LIMIT): Promise<number> {
  let sent = 0;
  for (;;) {
    const entries = await readOutbox(db, limit);
    if (!entries.length) return sent;
    const outgoing = await buildRequests(db, entries);
    const covered = new Set(outgoing.flatMap((o) => o.seqs));
    // Entries whose record no longer exists (e.g. an excerpt added then deleted) have nothing to send.
    await dropOutbox(db, entries.map((e) => e.seq!).filter((s) => !covered.has(s)));
    for (const o of outgoing) {
      if (o.type === "syncBody") {
        for (const chunk of await loadBodyChunks(db, o.key)) { await call("syncBody", chunk); sent++; }
      } else {
        await call(o.type, o.payload);
        sent++;
      }
      await dropOutbox(db, o.seqs);
    }
    if (entries.length < limit) return sent;
  }
}
