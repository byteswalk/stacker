// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import { describe, expect, it } from "vitest";
import { deleteBlockReason, siteName } from "./siteStatus";

describe("site status", () => {
  it("blocks deleting on an unverified site whatever else is known", () => {
    expect(deleteBlockReason("grok", {})).toBe("未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除");
    expect(deleteBlockReason("deepseek", { deepseek: 1 })).toBe("未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除");
  });
  it("blocks a verified site only while its interface is marked changed", () => {
    expect(deleteBlockReason("chatgpt", {})).toBeNull();
    expect(deleteBlockReason("claude", { claude: 5 })).toBe("接口已变化，请先刷新该站点");
  });
  it("names unverified sites as such", () => {
    expect(siteName("chatgpt")).toBe("ChatGPT");
    expect(siteName("grok")).toBe("Grok（未实测）");
  });
});
