import { describe, expect, it } from "vitest";
import { shouldRemember, siteTabOf } from "./lastTab";

describe("siteTabOf", () => {
  it("keeps only conversation pages of supported sites", () => {
    expect(siteTabOf({ id: 3, url: "https://claude.ai/chat/k1", title: "Refactor - Claude" })).toEqual({ tabId: 3, site: "claude", url: "https://claude.ai/chat/k1", title: "Refactor - Claude" });
    expect(siteTabOf({ id: 3, url: "https://claude.ai/new" })).toBeNull();
    expect(siteTabOf({ id: 3, url: "https://example.com/c/x" })).toBeNull();
    expect(siteTabOf({ url: "https://chatgpt.com/c/x" })).toBeNull();
  });
});

describe("shouldRemember", () => {
  it("only remembers the active tab, and only when its url or title changed", () => {
    expect(shouldRemember({ url: "https://claude.ai/chat/k1" }, { active: true })).toBe(true);
    expect(shouldRemember({ title: "Refactor - Claude" }, { active: true })).toBe(true);
    expect(shouldRemember({ url: "https://claude.ai/chat/k1" }, { active: false })).toBe(false);
    expect(shouldRemember({}, { active: true })).toBe(false);
  });
});
