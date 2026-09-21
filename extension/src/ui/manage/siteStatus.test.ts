// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import { afterEach, describe, expect, it } from "vitest";
import { SITES } from "../../sites/registry";
import { deleteBlockReason, siteName } from "./siteStatus";

// Every site is verified today; the gate still has to hold for the next one added, so a site is
// marked unverified for these checks and put back afterwards.
const original = { grok: SITES.grok.verified, deepseek: SITES.deepseek.verified };
const unverify = () => { SITES.grok.verified = false; SITES.deepseek.verified = false; };
afterEach(() => { SITES.grok.verified = original.grok; SITES.deepseek.verified = original.deepseek; });

describe("site status", () => {
  it("blocks deleting on an unverified site whatever else is known", () => {
    unverify();
    expect(deleteBlockReason("grok", {})).toBe("未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除");
    expect(deleteBlockReason("deepseek", { deepseek: 1 })).toBe("未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除");
  });
  it("blocks a verified site only while its interface is marked changed", () => {
    expect(deleteBlockReason("chatgpt", {})).toBeNull();
    expect(deleteBlockReason("claude", { claude: 5 })).toBe("接口已变化，请先刷新该站点");
  });
  it("names unverified sites as such", () => {
    unverify();
    expect(siteName("chatgpt")).toBe("ChatGPT");
    expect(siteName("grok")).toBe("Grok（未实测）");
  });
});
