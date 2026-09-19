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
  it("skips discarded tabs and tries the active, then loaded tabs first", async () => {
    const found = [
      { id: 1, status: "loading" },
      { id: 2, status: "complete" },
      { id: 3, status: "complete", discarded: true, active: true },
      { id: 4, status: "complete", active: true },
    ];
    const t = { query: vi.fn(async () => found), sendMessage: vi.fn(async () => ({ ok: true, value: 1 })) } as unknown as typeof chrome.tabs;
    await createSiteApi({ tabs: t }).account("chatgpt");
    expect(t.sendMessage).toHaveBeenCalledTimes(1);
    expect((t.sendMessage as ReturnType<typeof vi.fn>).mock.calls[0][0]).toBe(4);
  });
  it("falls through to the next tab when one has no agent, and reports E_NO_AGENT only when all fail", async () => {
    const found = [{ id: 1, active: true, status: "complete" }, { id: 2, status: "complete" }, { id: 3, status: "loading" }];
    const replies: Record<number, () => unknown> = {
      1: () => { throw new Error("Receiving end does not exist."); },
      2: () => undefined,
      3: () => ({ ok: true, value: "from-3" }),
    };
    const t = { query: vi.fn(async () => found), sendMessage: vi.fn(async (id: number) => replies[id]()) } as unknown as typeof chrome.tabs;
    expect(await createSiteApi({ tabs: t }).read("chatgpt", "x")).toBe("from-3");
    expect((t.sendMessage as ReturnType<typeof vi.fn>).mock.calls.map((c) => c[0])).toEqual([1, 2, 3]);
    replies[3] = () => undefined;
    await expect(createSiteApi({ tabs: t }).read("chatgpt", "x")).rejects.toThrow("E_NO_AGENT");
    const onlyDiscarded = { query: vi.fn(async () => [{ id: 5, discarded: true }]), sendMessage: vi.fn() } as unknown as typeof chrome.tabs;
    await expect(createSiteApi({ tabs: onlyDiscarded }).account("claude")).rejects.toThrow("E_NO_TAB");
    expect(onlyDiscarded.sendMessage).not.toHaveBeenCalled();
  });
  it("knows which sites can archive", () => {
    const api = createSiteApi({ tabs: tabs([], null) });
    expect(api.canArchive("chatgpt")).toBe(true);
    expect(api.canArchive("claude")).toBe(false);
  });
  it("refuses to remove from an unverified site without sending anything", async () => {
    const t = tabs([{ id: 1 }], { ok: true, value: null });
    await expect(createSiteApi({ tabs: t }).remove("grok", "x")).rejects.toThrow("E_BROKEN");
    expect(t.query).not.toHaveBeenCalled();
    expect(t.sendMessage).not.toHaveBeenCalled();
  });
});
