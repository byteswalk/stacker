import { describe, expect, it } from "vitest";
import type { Conversation } from "./db";
import { allTags, applyFilter, bodyText, EMPTY_FILTER } from "./search";

const c = (id: string, patch: Partial<Conversation> = {}): Conversation => ({
  key: `chatgpt:${id}`, site: "chatgpt", account: "chatgpt:u", id, title: id, createdAt: 0, updatedAt: 0, archived: false,
  folderId: null, tags: [], favorite: false, note: "", bodyFetchedAt: null, bodyUpdatedAt: null, removedAt: null,
  listedAt: 0, localUpdatedAt: 0, ...patch,
});

describe("search", () => {
  const list = [
    c("Kyoto trip", { updatedAt: 3, tags: ["travel"], favorite: true, bodyUpdatedAt: 3 }),
    c("Rust build", { updatedAt: 5, folderId: "f1", site: "claude", key: "claude:r", account: "claude:o" }),
    c("Old", { updatedAt: 1, removedAt: 9 }),
  ];
  const bodies = new Map([["chatgpt:Kyoto trip", bodyText({ key: "chatgpt:Kyoto trip", id: "k", title: "", updatedAt: 3, messages: [{ role: "assistant", text: "Temples in Higashiyama", at: null, attachments: [] }] })]]);

  it("sorts newest first and hides removed ones unless asked", () => {
    expect(applyFilter(list, EMPTY_FILTER, bodies).map((x) => x.id)).toEqual(["Rust build", "Kyoto trip"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, showRemoved: true }, bodies)).toHaveLength(3);
  });
  it("matches titles, and bodies only when asked", () => {
    expect(applyFilter(list, { ...EMPTY_FILTER, text: "higashi" }, bodies)).toEqual([]);
    expect(applyFilter(list, { ...EMPTY_FILTER, text: "higashi", inBody: true }, bodies).map((x) => x.id)).toEqual(["Kyoto trip"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, text: "RUST" }, bodies).map((x) => x.id)).toEqual(["Rust build"]);
  });
  it("filters by site, folder, tag, favorite, body state and time", () => {
    expect(applyFilter(list, { ...EMPTY_FILTER, site: "claude" }, bodies).map((x) => x.id)).toEqual(["Rust build"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, folder: "none" }, bodies).map((x) => x.id)).toEqual(["Kyoto trip"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, tag: "travel", favorite: true }, bodies)).toHaveLength(1);
    expect(applyFilter(list, { ...EMPTY_FILTER, body: "unread" }, bodies).map((x) => x.id)).toEqual(["Rust build"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, from: 4 }, bodies).map((x) => x.id)).toEqual(["Rust build"]);
    expect(allTags(list)).toEqual(["travel"]);
  });
});
