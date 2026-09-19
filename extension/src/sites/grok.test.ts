import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import type { PageAccess } from "./page";
import { grok } from "./grok";
import { listFirst, listLast, responseNodes, responses } from "./fixtures/grok";

type Call = [string, FetchInit | undefined];

function fake(route: (url: string, init?: FetchInit) => unknown, calls: Call[] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    const json = route(url, init);
    return json === undefined ? { status: 404, json: null, retryAfter: null } : { status: 200, json, retryAfter: null };
  });
}
const routes = (url: string, init?: FetchInit) => {
  if (init?.method === "DELETE") return {};
  if (url.endsWith("/response-node")) return responseNodes;
  if (url.endsWith("/load-responses")) return responses;
  if (url.includes("pageToken=p2")) return listLast;
  if (url.includes("/rest/app-chat/conversations?")) return listFirst;
  return undefined;
};
const withCookies = (cookies: Record<string, string>): PageAccess => ({ cookie: (name) => cookies[name] ?? null, storage: () => null });

describe("grok adapter", () => {
  it("proves the sign-in with a one-item listing and names the account by the x-userid cookie", async () => {
    const calls: Call[] = [];
    expect(await grok(fake(routes, calls), withCookies({ "x-userid": "u-42" })).account()).toEqual({ remoteId: "u-42", label: "Grok" });
    expect(calls[0][0]).toBe("https://grok.com/rest/app-chat/conversations?pageSize=1");
    expect(await grok(fake(routes)).account()).toEqual({ remoteId: "default", label: "Grok" });
  });

  it("treats a refused listing as signed out", async () => {
    const refused = vi.fn(async (): Promise<FetchResult> => ({ status: 401, json: null, retryAfter: null }));
    await expect(grok(refused).account()).rejects.toThrow("E_AUTH");
  });

  it("pages with the token Grok returns", async () => {
    const calls: Call[] = [];
    const a = grok(fake(routes, calls));
    const first = await a.list(null);
    expect(first.items.map((i) => [i.id, i.title])).toEqual([["g-1", "Rust lifetimes"], ["g-2", ""]]);
    expect(first.items[0]).toMatchObject({ createdAt: Date.parse("2026-09-01T10:00:00.000Z"), updatedAt: Date.parse("2026-09-02T10:00:00.000Z"), archived: false });
    expect(first.items[1].updatedAt).toBe(first.items[1].createdAt);
    expect(first.next).toBe("p2");
    const second = await a.list(first.next);
    expect(second.items.map((i) => i.id)).toEqual(["g-3"]);
    expect(second.next).toBeNull();
    expect(calls.map(([url]) => url)).toEqual([
      "https://grok.com/rest/app-chat/conversations?pageSize=60",
      "https://grok.com/rest/app-chat/conversations?pageSize=60&pageToken=p2",
    ]);
  });

  it("reads only the branch ending at the last node", async () => {
    const calls: Call[] = [];
    const body = await grok(fake(routes, calls)).read("g-1");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([
      ["user", "Explain lifetimes"], ["assistant", "A lifetime is a scope."],
      ["user", "Example?"], ["assistant", "fn f<'a>(x: &'a str) {}"],
    ]);
    expect(body.messages[0].attachments).toEqual(["main.rs"]);
    expect(body.updatedAt).toBe(Date.parse("2026-09-01T10:01:05.000Z"));
    const load = calls.find(([url]) => url.endsWith("/load-responses"))!;
    expect(load[1]).toMatchObject({ method: "POST", body: { responseIds: ["r1", "r2", "r3", "r4"] } });
  });

  it("deletes with DELETE and has no archive", async () => {
    const calls: Call[] = [];
    const a = grok(fake(routes, calls));
    await a.remove("g-1");
    expect(calls).toEqual([["https://grok.com/rest/app-chat/conversations/g-1", { method: "DELETE" }]]);
    expect(a.archive).toBeUndefined();
    expect(a.conversationUrl("g-1")).toBe("https://grok.com/c/g-1");
  });

  it("flags a changed shape as broken", async () => {
    await expect(grok(fake(() => ({ items: [] }))).list(null)).rejects.toThrow("E_BROKEN");
    const oddSender = (url: string, init?: FetchInit) =>
      url.endsWith("/load-responses") ? { responses: responses.responses.map((r) => ({ ...r, sender: "system" })) } : routes(url, init);
    await expect(grok(fake(oddSender)).read("g-1")).rejects.toThrow("E_BROKEN");
    const missing = (url: string, init?: FetchInit) => (url.endsWith("/load-responses") ? { responses: [] } : routes(url, init));
    await expect(grok(fake(missing)).read("g-1")).rejects.toThrow("E_BROKEN");
    const dangling = (url: string, init?: FetchInit) =>
      url.endsWith("/response-node") ? { responseNodes: [{ responseId: "r9", sender: "human", parentResponseId: "nowhere" }] } : routes(url, init);
    await expect(grok(fake(dangling)).read("g-1")).rejects.toThrow("E_BROKEN");
  });

  it("reports a conversation with no response nodes as not found", async () => {
    const empty = (url: string, init?: FetchInit) => (url.endsWith("/response-node") ? { responseNodes: [] } : routes(url, init));
    await expect(grok(fake(empty)).read("g-gone")).rejects.toThrow("E_NOT_FOUND");
  });
});
