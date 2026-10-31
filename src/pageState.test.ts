import { describe, expect, it } from "vitest";
import { DEFAULT_PAGE, PAGE_IDS } from "./pageState";

describe("page state", () => {
  it("opens on the overview, which is one of the pages", () => {
    expect(DEFAULT_PAGE).toBe("overview");
    expect(PAGE_IDS).toContain(DEFAULT_PAGE);
  });
});
