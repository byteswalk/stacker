import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import { claude } from "./claude";
import { conversation, listFull, listTail, orgs } from "./fixtures/claude";

function fake(route: (url: string, init?: FetchInit) => unknown, calls: [string, FetchInit | undefined][] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    const json = route(url, init);
    return json === undefined ? { status: 404, json: null, retryAfter: null } : { status: init?.method === "DELETE" ? 204 : 200, json, retryAfter: null };
  });
}
const routes = (url: string, init?: FetchInit) => {
  if (init?.method === "DELETE") return null;
  if (url.endsWith("/api/organizations")) return orgs;
  if (url.includes("chat_conversations/k1")) return conversation;
  if (url.includes("offset=0")) return listFull;
  if (url.includes("offset=100")) return listTail;
  return undefined;
};

describe("claude adapter", () => {
  it("uses the chat organization as the account", async () => {
    expect(await claude(fake(routes)).account()).toEqual({ remoteId: "org-chat", label: "Claude" });
  });
  it("pages by offset until a short page", async () => {
    const a = claude(fake(routes));
    const first = await a.list(null);
    expect(first.items).toHaveLength(100);
    expect(first.next).toBe("100");
    const second = await a.list(first.next);
    expect(second.items.map((i) => i.id)).toEqual(["k100"]);
    expect(second.next).toBeNull();
  });
  it("reads the branch ending at the current leaf, text blocks only", async () => {
    const body = await claude(fake(routes)).read("k1");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([["user", "Help me refactor"], ["assistant", "Here is the plan."]]);
    expect(body.messages[0].attachments).toEqual(["a.ts"]);
  });
  it("deletes with DELETE and has no archive", async () => {
    const calls: [string, FetchInit | undefined][] = [];
    const a = claude(fake(routes, calls));
    await a.remove("k1");
    expect(calls.some(([url, init]) => init?.method === "DELETE" && url.endsWith("/chat_conversations/k1"))).toBe(true);
    expect(a.archive).toBeUndefined();
  });
  it("flags a changed shape as broken", async () => {
    const a = claude(fake((url) => (url.endsWith("/api/organizations") ? orgs : { conversations: [] })));
    await expect(a.list(null)).rejects.toThrow("E_BROKEN");
  });
});
