import { describe, expect, it } from "vitest";
import { isValidSaveExcerpt, type SaveExcerpt } from "./saveExcerpt";

const base: SaveExcerpt = { type: "save-excerpt", site: "claude", conversationId: "k1", url: "https://claude.ai/chat/k1", pageTitle: "Refactor - Claude", text: "hello" };

describe("isValidSaveExcerpt", () => {
  it("accepts a well-shaped message for a known site", () => {
    expect(isValidSaveExcerpt(base)).toBe(true);
  });
  it("rejects an unknown site", () => {
    expect(isValidSaveExcerpt({ ...base, site: "bogus" })).toBe(false);
  });
  it("rejects a non-string text", () => {
    expect(isValidSaveExcerpt({ ...base, text: 42 })).toBe(false);
  });
  it("rejects messages of the wrong shape or type", () => {
    expect(isValidSaveExcerpt(null)).toBe(false);
    expect(isValidSaveExcerpt("nope")).toBe(false);
    expect(isValidSaveExcerpt({ type: "other" })).toBe(false);
  });
});
