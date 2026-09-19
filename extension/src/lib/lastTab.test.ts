import { describe, expect, it } from "vitest";
import { siteTabOf } from "./lastTab";

describe("siteTabOf", () => {
  it("keeps only conversation pages of supported sites", () => {
    expect(siteTabOf({ id: 3, url: "https://claude.ai/chat/k1", title: "Refactor - Claude" })).toEqual({ tabId: 3, site: "claude", url: "https://claude.ai/chat/k1", title: "Refactor - Claude" });
    expect(siteTabOf({ id: 3, url: "https://claude.ai/new" })).toBeNull();
    expect(siteTabOf({ id: 3, url: "https://example.com/c/x" })).toBeNull();
    expect(siteTabOf({ url: "https://chatgpt.com/c/x" })).toBeNull();
  });
});
