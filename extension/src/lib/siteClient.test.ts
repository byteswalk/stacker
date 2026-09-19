import { describe, expect, it, vi } from "vitest";
import { createSiteApi } from "./siteClient";

function tabs(found: { id: number }[], reply: unknown) {
  return {
    query: vi.fn(async () => found),
    sendMessage: vi.fn(async () => reply),
  } as unknown as typeof chrome.tabs;
}

describe("site client", () => {
  it("sends the op to a tab of that site and returns the value", async () => {
    const t = tabs([{ id: 7 }], { ok: true, value: { remoteId: "u", label: "ChatGPT" } });
    const api = createSiteApi({ tabs: t });
    expect(await api.account("chatgpt")).toEqual({ remoteId: "u", label: "ChatGPT" });
    expect(t.query).toHaveBeenCalledWith({ url: "https://chatgpt.com/*" });
    expect(t.sendMessage).toHaveBeenCalledWith(7, { type: "site-rpc", site: "chatgpt", op: "account", arg: null });
  });
  it("reports a missing tab, a tab without the agent, and revives site errors", async () => {
    await expect(createSiteApi({ tabs: tabs([], null) }).account("claude")).rejects.toThrow("E_NO_TAB");
    const noAgent = { query: vi.fn(async () => [{ id: 1 }]), sendMessage: vi.fn(async () => { throw new Error("Receiving end does not exist."); }) } as unknown as typeof chrome.tabs;
    await expect(createSiteApi({ tabs: noAgent }).account("claude")).rejects.toThrow("E_NO_AGENT");
    const failing = tabs([{ id: 1 }], { ok: false, error: { code: "E_AUTH", detail: "401", retryAfterMs: null } });
    await expect(createSiteApi({ tabs: failing }).read("claude", "k")).rejects.toThrow("E_AUTH: 401");
  });
  it("knows which sites can archive", () => {
    const api = createSiteApi({ tabs: tabs([], null) });
    expect(api.canArchive("chatgpt")).toBe(true);
    expect(api.canArchive("claude")).toBe(false);
  });
});
