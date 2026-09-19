import { describe, expect, it } from "vitest";
import { SiteError } from "../shared/types";
import { arr, obj, str, time } from "./guards";

describe("guards", () => {
  it("reads times given as ISO text, seconds or milliseconds", () => {
    expect(time("2026-09-19T00:00:00Z", "t")).toBe(Date.UTC(2026, 8, 19));
    expect(time(1_789_776_000, "t")).toBe(1_789_776_000_000);
    expect(time(1_789_776_000_123, "t")).toBe(1_789_776_000_123);
  });
  it("rejects an unexpected shape as E_BROKEN naming the path", () => {
    expect(() => obj(null, "root")).toThrow(SiteError);
    try { str(5, "items[0].id"); } catch (e) { expect((e as SiteError).code).toBe("E_BROKEN"); expect((e as SiteError).detail).toBe("items[0].id"); }
    expect(() => arr({}, "items")).toThrow("E_BROKEN: items");
    expect(() => time("yesterday", "t")).toThrow("E_BROKEN: t");
  });
});
