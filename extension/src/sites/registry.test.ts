import { describe, expect, it } from "vitest";
import { conversationIdOfUrl, conversationUrl, siteOfUrl } from "./registry";

describe("registry", () => {
  it("recognises sites and conversation ids from URLs", () => {
    expect(siteOfUrl("https://chatgpt.com/c/abc-1")).toBe("chatgpt");
    expect(siteOfUrl("https://claude.ai/chat/k1")).toBe("claude");
    expect(siteOfUrl("https://example.com/")).toBeNull();
    expect(conversationIdOfUrl("chatgpt", "https://chatgpt.com/g/g-x/c/abc-1?model=x")).toBe("abc-1");
    expect(conversationIdOfUrl("claude", "https://claude.ai/chat/k1")).toBe("k1");
    expect(conversationIdOfUrl("claude", "https://claude.ai/new")).toBeNull();
    expect(conversationUrl("chatgpt", "abc")).toBe("https://chatgpt.com/c/abc");
  });
});
