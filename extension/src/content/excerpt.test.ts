// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { excerptPayload } from "./excerpt";

describe("excerptPayload", () => {
  it("builds a save message from the page and the selection", () => {
    expect(excerptPayload("https://chatgpt.com/c/abc", "Trip - ChatGPT", "  Kyoto in autumn  ")).toEqual({
      type: "save-excerpt", site: "chatgpt", conversationId: "abc", url: "https://chatgpt.com/c/abc", pageTitle: "Trip - ChatGPT", text: "Kyoto in autumn",
    });
    expect(excerptPayload("https://claude.ai/new", "", "some text")?.conversationId).toBeNull();
    expect(excerptPayload("https://claude.ai/chat/k", "", " a ")).toBeNull();
    expect(excerptPayload("https://example.com/", "", "hello")).toBeNull();
  });
});
