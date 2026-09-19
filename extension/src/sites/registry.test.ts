import { describe, expect, it } from "vitest";
import type { SiteId } from "../shared/types";
import { conversationIdOfUrl, conversationUrl, siteOfUrl, SITES } from "./registry";

/** One conversation id per site, in the form its adapter uses. */
const SAMPLE_ID: Record<SiteId, string> = { chatgpt: "abc-1", claude: "k1", gemini: "c_aaa111" };

describe("registry", () => {
  it("recognises sites and conversation ids from URLs", () => {
    expect(siteOfUrl("https://chatgpt.com/c/abc-1")).toBe("chatgpt");
    expect(siteOfUrl("https://claude.ai/chat/k1")).toBe("claude");
    expect(siteOfUrl("https://example.com/")).toBeNull();
    expect(conversationIdOfUrl("chatgpt", "https://chatgpt.com/g/g-x/c/abc-1?model=x")).toBe("abc-1");
    expect(conversationIdOfUrl("claude", "https://claude.ai/chat/k1")).toBe("k1");
    expect(conversationIdOfUrl("claude", "https://claude.ai/new")).toBeNull();
    expect(conversationUrl("chatgpt", "abc")).toBe("https://chatgpt.com/c/abc");
    expect(siteOfUrl("https://gemini.google.com/app/aaa111")).toBe("gemini");
    expect(conversationIdOfUrl("gemini", "https://gemini.google.com/app/aaa111?hl=en")).toBe("c_aaa111");
    expect(conversationIdOfUrl("gemini", "https://gemini.google.com/app")).toBeNull();
    expect(conversationUrl("gemini", "c_aaa111")).toBe("https://gemini.google.com/app/aaa111");
  });

  it("finds every site's conversation id in the URL it builds for it", () => {
    for (const site of Object.keys(SITES) as SiteId[]) {
      const url = conversationUrl(site, SAMPLE_ID[site]);
      expect(siteOfUrl(url)).toBe(site);
      expect(conversationIdOfUrl(site, url)).toBe(SAMPLE_ID[site]);
    }
  });

  it("marks which sites were checked on a real account", () => {
    const verified = Object.fromEntries((Object.keys(SITES) as SiteId[]).map((s) => [s, SITES[s].verified]));
    expect(verified).toEqual({ chatgpt: true, claude: true, gemini: false });
  });
});
