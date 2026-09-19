import { afterEach, describe, expect, it, vi } from "vitest";
import { reviveError, serializeError, SiteError } from "../shared/types";
import { browserFetchJson, expectOk } from "./http";

const res = (status: number, retryAfter: string | null = null) => ({ status, json: { ok: true }, retryAfter });

describe("expectOk", () => {
  it("maps statuses to error codes", () => {
    expect(expectOk(res(200))).toEqual({ ok: true });
    for (const [status, code] of [[401, "E_AUTH"], [403, "E_AUTH"], [404, "E_NOT_FOUND"], [500, "E_HTTP"]] as const) {
      expect(() => expectOk(res(status))).toThrow(code);
    }
  });
  it("carries Retry-After on 429 in milliseconds", () => {
    try { expectOk(res(429, "7")); } catch (e) { expect((e as SiteError).code).toBe("E_RATE"); expect((e as SiteError).retryAfterMs).toBe(7000); }
  });
  it("round-trips errors across messaging", () => {
    const e = reviveError(serializeError(new SiteError("E_RATE", "x", 5)));
    expect([e.code, e.detail, e.retryAfterMs]).toEqual(["E_RATE", "x", 5]);
    expect(serializeError(new Error("boom")).code).toBe("E_NET");
  });
});

describe("browserFetchJson", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("sends a form url-encoded and keeps the raw text of a reply that is not JSON", async () => {
    const fetchMock = vi.fn(async () => new Response(")]}'\n[1]", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const res = await browserFetchJson("https://gemini.google.com/x", { method: "POST", form: { "f.req": "[1]", at: "a b" }, headers: { "X-Same-Domain": "1" } });
    expect(res).toEqual({ status: 200, json: null, retryAfter: null, text: ")]}'\n[1]" });
    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.body).toBe("f.req=%5B1%5D&at=a+b");
    expect(init.headers).toEqual({ "Content-Type": "application/x-www-form-urlencoded;charset=UTF-8", "X-Same-Domain": "1" });
    expect(init.credentials).toBe("include");
  });

  it("still sends a body as JSON and parses a JSON reply", async () => {
    const fetchMock = vi.fn(async () => new Response('{"ok":true}', { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const res = await browserFetchJson("https://grok.com/x", { method: "POST", body: { a: 1 } });
    expect(res.json).toEqual({ ok: true });
    expect(res.text).toBe('{"ok":true}');
    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.body).toBe('{"a":1}');
    expect(init.headers).toEqual({ "Content-Type": "application/json" });
  });
});
