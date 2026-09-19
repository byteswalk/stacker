import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import type { PageAccess } from "./page";
import { deepseek } from "./deepseek";
import { history, ok, pageFirst, pageLast, serverError, signedOut, user } from "./fixtures/deepseek";

type Call = [string, FetchInit | undefined];

function fake(route: (url: string, init?: FetchInit) => unknown, calls: Call[] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    const json = route(url, init);
    return json === undefined ? { status: 404, json: null, retryAfter: null } : { status: 200, json, retryAfter: null };
  });
}
const routes = (url: string) => {
  if (url.endsWith("/api/v0/users/current")) return user;
  if (url.includes("/chat_session/fetch_page")) return url.includes("lte_cursor") ? pageLast : pageFirst;
  if (url.includes("/chat/history_messages")) return history;
  if (url.endsWith("/chat_session/delete")) return ok(null);
  return undefined;
};
const signedIn: PageAccess = { cookie: () => null, storage: (key) => (key === "userToken" ? JSON.stringify({ value: "tok-ds", __version: "0" }) : null) };

describe("deepseek adapter", () => {
  it("names the account by user id, sends the page's token, and never keeps the email or phone", async () => {
    const calls: Call[] = [];
    const account = await deepseek(fake(routes, calls), signedIn).account();
    expect(account).toEqual({ remoteId: "ds-user-1", label: "DeepSeek" });
    expect(JSON.stringify(account)).not.toMatch(/example\.com|13800000000/);
    expect(calls[0][1]?.headers).toEqual({ Authorization: "Bearer tok-ds", "x-client-platform": "web" });
  });

  it("is signed out without a token, or when the site refuses it", async () => {
    const calls: Call[] = [];
    await expect(deepseek(fake(routes, calls)).account()).rejects.toThrow("E_AUTH");
    expect(calls).toEqual([]);
    await expect(deepseek(fake(() => signedOut), signedIn).account()).rejects.toThrow("E_AUTH");
    const refused = vi.fn(async (): Promise<FetchResult> => ({ status: 401, json: null, retryAfter: null }));
    await expect(deepseek(refused, signedIn).account()).rejects.toThrow("E_AUTH");
  });

  it("treats any other failure code or wrapper change as broken", async () => {
    await expect(deepseek(fake(() => serverError), signedIn).account()).rejects.toThrow("E_BROKEN");
    await expect(deepseek(fake(() => ({ code: 0, msg: "", data: { biz_code: 7, biz_data: null } })), signedIn).account()).rejects.toThrow("E_BROKEN");
    await expect(deepseek(fake(() => ok({ sessions: [] })), signedIn).list(null)).rejects.toThrow("E_BROKEN");
  });

  it("pages with the last session as cursor and drops the repeated boundary session", async () => {
    const calls: Call[] = [];
    const a = deepseek(fake(routes, calls), signedIn);
    const first = await a.list(null);
    expect(first.items.map((i) => [i.id, i.title])).toEqual([["s-1", "Pinned plan"], ["s-2", "Sorting"]]);
    expect(first.items[0]).toMatchObject({ createdAt: 1_788_000_000_500, updatedAt: 1_789_000_000_250, archived: false });
    expect(first.next).toBe("false|1788900000.75|s-2");
    const second = await a.list(first.next);
    expect(second.items.map((i) => i.id)).toEqual(["s-3"]);
    expect(second.next).toBeNull();
    const query = new URL(calls[1][0]).searchParams;
    expect([query.get("count"), query.get("lte_cursor.pinned"), query.get("lte_cursor.updated_at")]).toEqual(["100", "false", "1788900000.75"]);
  });

  it("stops when the cursor does not move", async () => {
    const stuck = deepseek(fake(() => pageFirst), signedIn);
    await expect(stuck.list("false|1788900000.75|s-2")).rejects.toThrow("E_BROKEN: paging");
  });

  it("reads the branch on screen, answers without the thinking", async () => {
    const body = await deepseek(fake(routes), signedIn).read("s-1");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([["user", "Plan a sprint"], ["assistant", "Two weeks, three goals."], ["user", "Shorter?"]]);
    expect(body.messages[0].attachments).toEqual(["backlog.csv"]);
    expect(body).toMatchObject({ id: "s-1", title: "Pinned plan", updatedAt: 1_789_000_000_250 });
  });

  it("fails on a current message that is not in the chat", async () => {
    const lost = ok({ chat_session: { id: "s-1", title: "", updated_at: 1, current_message_id: 99 }, chat_messages: [] });
    await expect(deepseek(fake(() => lost), signedIn).read("s-1")).rejects.toThrow("E_BROKEN");
  });

  it("reports a chat session with no messages as not found", async () => {
    const empty = ok({ chat_session: { id: "s-1", title: "", updated_at: 1, current_message_id: null }, chat_messages: [] });
    await expect(deepseek(fake(() => empty), signedIn).read("s-1")).rejects.toThrow("E_NOT_FOUND");
  });

  it("deletes with POST and has no archive", async () => {
    const calls: Call[] = [];
    const a = deepseek(fake(routes, calls), signedIn);
    await a.remove("s-1");
    expect(calls[0][0]).toBe("https://chat.deepseek.com/api/v0/chat_session/delete");
    expect(calls[0][1]).toMatchObject({ method: "POST", body: { chat_session_id: "s-1" } });
    expect(a.archive).toBeUndefined();
    expect(a.conversationUrl("s-1")).toBe("https://chat.deepseek.com/a/chat/s/s-1");
  });
});
