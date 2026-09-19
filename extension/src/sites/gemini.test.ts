import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import { gemini } from "./gemini";
import { appHtml, batchReply, chatNewestFirst, listFirst, listLast, signedOutHtml } from "./fixtures/gemini";

type Call = [string, FetchInit | undefined];
interface Rpc { id: string; payload: unknown }

const text = (body: string, status = 200): FetchResult => ({ status, json: null, retryAfter: null, text: body });

/** The rpc id and decoded payload of a batchexecute call. */
function rpcOf(init?: FetchInit): Rpc {
  const [[[id, payload]]] = JSON.parse(init?.form?.["f.req"] ?? "[[[]]]") as [[[string, string]]];
  return { id, payload: JSON.parse(payload) };
}

function fake(route: (rpc: Rpc) => string | FetchResult, html = appHtml, calls: Call[] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    if (url === "https://gemini.google.com/app") return text(html);
    const out = route(rpcOf(init));
    return typeof out === "string" ? text(out) : out;
  });
}

const routes = ({ id, payload }: Rpc): string => {
  const p = payload as unknown[];
  if (id === "MaZiqc") return batchReply(id, p[1] === "tok-2" ? listLast : listFirst);
  if (id === "hNvQHb") return batchReply(id, [chatNewestFirst, null]);
  if (id === "GzXR5e") return batchReply(id, []);
  return "";
};
const batchCalls = (calls: Call[]) => calls.filter(([url]) => url.includes("/batchexecute"));
const pageLoads = (calls: Call[]) => calls.filter(([url]) => url === "https://gemini.google.com/app");

describe("gemini adapter", () => {
  it("names the account by the user id from the page and never keeps the email", async () => {
    const account = await gemini(fake(routes)).account();
    expect(account).toEqual({ remoteId: "108000000000000000001", label: "Gemini" });
    expect(JSON.stringify(account)).not.toContain("example.com");
  });

  it("treats a page without the sign-in token as signed out", async () => {
    await expect(gemini(fake(routes, signedOutHtml)).account()).rejects.toThrow("E_AUTH");
    await expect(gemini(fake(routes, signedOutHtml)).list(null)).rejects.toThrow("E_AUTH");
  });

  it("lists page by page with the token Gemini returns, fetching the page tokens once", async () => {
    const calls: Call[] = [];
    const a = gemini(fake(routes, appHtml, calls));
    const first = await a.list(null);
    expect(first.items.map((i) => [i.id, i.title])).toEqual([["c_aaa111", "Trip plan"], ["c_bbb222", ""]]);
    expect(first.items[0]).toMatchObject({ createdAt: 1_789_000_000_500, updatedAt: 1_789_000_000_500, archived: false });
    expect(first.next).toBe("tok-2");
    const second = await a.list(first.next);
    expect(second).toEqual({ items: [{ id: "c_ccc333", title: "Old chat", createdAt: 1_787_000_000_000, updatedAt: 1_787_000_000_000, archived: false }], next: null });
    expect(batchCalls(calls).map(([, init]) => rpcOf(init))).toEqual([
      { id: "MaZiqc", payload: [100, null, [0, null, 1]] },
      { id: "MaZiqc", payload: [100, "tok-2", [0, null, 1]] },
    ]);
    expect(pageLoads(calls)).toHaveLength(1);
  });

  it("sends the tokens the way the page does", async () => {
    const calls: Call[] = [];
    await gemini(fake(routes, appHtml, calls)).list(null);
    const [url, init] = batchCalls(calls)[0];
    const query = new URL(url).searchParams;
    expect(query.get("rpcids")).toBe("MaZiqc");
    expect(query.get("bl")).toBe("boq_assistant-bard-web-server_20260915.08_p0");
    expect(query.get("f.sid")).toBe("-1234567890123456789");
    expect(query.get("_reqid")).toMatch(/^\d{5}$/);
    expect(init).toMatchObject({ method: "POST", form: { at: "AKlEn5_tok:1789000000000" }, headers: { "X-Same-Domain": "1" } });
  });

  it("reads the turns oldest first, prompt before answer", async () => {
    const body = await gemini(fake(routes)).read("c_aaa111");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([
      ["user", "Where to go in Kyoto?"], ["assistant", "Try Arashiyama."],
      ["user", "And in winter?"], ["assistant", "Snowy and quiet."],
    ]);
    expect(body.messages[0].at).toBe(1_789_000_000_000);
    expect(body.updatedAt).toBe(1_789_000_100_000);
    expect(body.id).toBe("c_aaa111");
  });

  it("follows a continuation token when a chat has more turns", async () => {
    const calls: Call[] = [];
    const paged = ({ id, payload }: Rpc): string => {
      const p = payload as unknown[];
      if (id !== "hNvQHb") return "";
      return batchReply(id, p[2] === "more-1" ? [[chatNewestFirst[1]], null] : [[chatNewestFirst[0]], "more-1"]);
    };
    const body = await gemini(fake(paged, appHtml, calls)).read("c_aaa111");
    expect(body.messages.map((m) => m.text)).toEqual(["Where to go in Kyoto?", "Try Arashiyama.", "And in winter?", "Snowy and quiet."]);
    expect(batchCalls(calls).map(([, init]) => rpcOf(init).payload)).toEqual([
      ["c_aaa111", 1000, null, 1, [0], [4], null, 1],
      ["c_aaa111", 1000, "more-1", 1, [0], [4], null, 1],
    ]);
  });

  it("reports a chat that returns nothing as not found", async () => {
    await expect(gemini(fake(({ id }) => batchReply(id, null))).read("c_gone")).rejects.toThrow("E_NOT_FOUND");
  });

  it("reports a chat whose first page is well-formed but empty as not found", async () => {
    await expect(gemini(fake(({ id }) => batchReply(id, [[], null]))).read("c_gone")).rejects.toThrow("E_NOT_FOUND");
  });

  it("deletes with GzXR5e, has no archive, and links without the c_ prefix", async () => {
    const calls: Call[] = [];
    const a = gemini(fake(routes, appHtml, calls));
    await a.remove("c_aaa111");
    expect(batchCalls(calls).map(([, init]) => rpcOf(init))).toEqual([{ id: "GzXR5e", payload: ["c_aaa111"] }]);
    expect(a.archive).toBeUndefined();
    expect(a.conversationUrl("c_aaa111")).toBe("https://gemini.google.com/app/aaa111");
  });

  it("flags a non-array delete reply as broken", async () => {
    await expect(gemini(fake(({ id }) => batchReply(id, { ok: true }))).remove("c_aaa111")).rejects.toThrow("E_BROKEN");
  });

  it("fetches fresh page tokens once when the old ones are refused", async () => {
    const calls: Call[] = [];
    let refusals = 1;
    const a = gemini(fake(({ id }) => (refusals-- > 0 ? text("", 400) : batchReply(id, listLast)), appHtml, calls));
    expect((await a.list(null)).items.map((i) => i.id)).toEqual(["c_ccc333"]);
    expect(pageLoads(calls)).toHaveLength(2);
    await expect(gemini(fake(() => text("", 400))).list(null)).rejects.toThrow("E_HTTP: 400");
  });

  it("flags a changed shape as broken", async () => {
    await expect(gemini(fake(() => batchReply("MaZiqc", { chats: [] }))).list(null)).rejects.toThrow("E_BROKEN");
    await expect(gemini(fake(() => batchReply("MaZiqc", [null, null, [["aaa111", "x", null, null, null, [1, 0]]]]))).list(null)).rejects.toThrow("E_BROKEN");
    await expect(gemini(fake(() => "<html></html>")).list(null)).rejects.toThrow("E_BROKEN");
    await expect(gemini(fake(() => batchReply("hNvQHb", [[chatNewestFirst[0].slice(0, 3)], null]))).read("c_aaa111")).rejects.toThrow("E_BROKEN");
  });

  it("keeps the user message when a turn's answer text is missing, but still requires turn[3] to be an array", async () => {
    const noAnswer = [["c_aaa111", "r_1"], ["c_aaa111", "r_1", "rc_1"], [["Only a question?"], 1, null, 0], [], [1_789_000_000, 0]];
    const body = await gemini(fake(({ id }) => batchReply(id, [[noAnswer], null]))).read("c_aaa111");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([["user", "Only a question?"]]);

    const notArray = [["c_aaa111", "r_1"], ["c_aaa111", "r_1", "rc_1"], [["Only a question?"], 1, null, 0], "nope", [1_789_000_000, 0]];
    await expect(gemini(fake(({ id }) => batchReply(id, [[notArray], null]))).read("c_aaa111")).rejects.toThrow("E_BROKEN");
  });
});
