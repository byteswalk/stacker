import { describe, expect, it } from "vitest";
import { reviveError, serializeError, SiteError } from "../shared/types";
import { expectOk } from "./http";

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
