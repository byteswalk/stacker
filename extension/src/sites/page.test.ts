import { describe, expect, it } from "vitest";
import { NO_PAGE, pageOf } from "./page";

describe("page access", () => {
  it("reads one cookie by its exact name, decoded", () => {
    const page = pageOf({ cookie: "a=1; x-userid=u%2042; xx-userid=no" }, () => ({ getItem: () => null }));
    expect(page.cookie("x-userid")).toBe("u 42");
    expect(page.cookie("missing")).toBeNull();
  });

  it("reads localStorage and treats a blocked storage as empty", () => {
    const page = pageOf({ cookie: "" }, () => ({ getItem: (k: string) => (k === "userToken" ? '{"value":"t"}' : null) }));
    expect(page.storage("userToken")).toBe('{"value":"t"}');
    expect(page.storage("other")).toBeNull();
    const blocked = pageOf({ cookie: "" }, () => { throw new Error("SecurityError"); });
    expect(blocked.storage("userToken")).toBeNull();
  });

  it("has an empty stand-in for callers without a page", () => {
    expect(NO_PAGE.cookie("x")).toBeNull();
    expect(NO_PAGE.storage("x")).toBeNull();
  });
});
