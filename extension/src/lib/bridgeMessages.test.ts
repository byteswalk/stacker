import "fake-indexeddb/auto";
import { describe, expect, it, vi } from "vitest";
import { BridgeError, type Bridge } from "./bridge";
import { ask, createBridgeHandler, isBridgeMessage, type BridgeStatus } from "./bridgeMessages";
import { openDb } from "./db";

let n = 0;
function fakeBridge(up = false, fail?: BridgeError) {
  let connected = up;
  const bridge: Bridge = {
    call: vi.fn(async (type: string) => { if (fail) throw fail; connected = true; return { echo: type }; }),
    connected: () => connected,
    lastError: () => (connected ? "" : "host not found"),
    reset: vi.fn(),
  };
  return bridge;
}
function handler(bridge: Bridge) {
  const flushSoon = vi.fn();
  const name = `bm-${n++}`;
  const handle = createBridgeHandler({ bridge, openDb: () => openDb(name), flushSoon, lastSyncAt: () => 42 });
  return { handle, flushSoon };
}

describe("bridge messages", () => {
  it("accepts only the messages pages may send", () => {
    expect(isBridgeMessage({ type: "bridge-status", connect: true, force: false })).toBe(true);
    expect(isBridgeMessage({ type: "bridge-flush" })).toBe(true);
    expect(isBridgeMessage({ type: "bridge-call", call: "saveExport", payload: {} })).toBe(true);
    expect(isBridgeMessage({ type: "bridge-call", call: "distillResults", payload: {} })).toBe(true);
    expect(isBridgeMessage({ type: "bridge-call", call: "setTheme", payload: { theme: "dark" } })).toBe(true);
    expect(isBridgeMessage({ type: "bridge-call", call: "syncAccounts", payload: {} })).toBe(false);
    expect(isBridgeMessage({ type: "save-excerpt" })).toBe(false);
    expect(isBridgeMessage(null)).toBe(false);
  });

  it("connects on a status check and flushes what is pending", async () => {
    const bridge = fakeBridge();
    const { handle, flushSoon } = handler(bridge);
    const reply = await handle({ type: "bridge-status", connect: true, force: true });
    expect(bridge.reset).toHaveBeenCalled();
    expect(bridge.call).toHaveBeenCalledWith("status");
    // A fresh database holds the upgrade's "all" entry.
    expect(reply).toEqual({ ok: true, value: { connected: true, pending: 1, lastSyncAt: 42, error: "", theme: null } satisfies BridgeStatus });
    expect(flushSoon).toHaveBeenCalledWith(0);
  });

  it("carries Stacker's appearance so the pages can match it", async () => {
    const bridge = fakeBridge(true);
    bridge.call = vi.fn(async (type: string) => (type === "status" ? { theme: "light" } : {}));
    const { handle } = handler(bridge);
    const reply = await handle({ type: "bridge-status", connect: false, force: false });
    // Already connected: the appearance is fetched even when no connection attempt was asked for.
    expect(bridge.call).toHaveBeenCalledWith("status");
    expect(reply).toMatchObject({ ok: true, value: { connected: true, theme: "light" } });
  });

  it("reports no appearance when Stacker does not answer with one", async () => {
    const bridge = fakeBridge(true);
    bridge.call = vi.fn(async () => ({ counts: {} }));
    const { handle } = handler(bridge);
    expect(await handle({ type: "bridge-status", connect: false, force: false }))
      .toMatchObject({ ok: true, value: { theme: null } });
  });

  it("reports standalone without trying when not asked to connect", async () => {
    const bridge = fakeBridge();
    const { handle, flushSoon } = handler(bridge);
    const reply = await handle({ type: "bridge-status", connect: false, force: false });
    expect(bridge.call).not.toHaveBeenCalled();
    expect(reply).toMatchObject({ ok: true, value: { connected: false, error: "host not found" } });
    expect(flushSoon).not.toHaveBeenCalled();
  });

  it("passes calls through and maps bridge errors to their code", async () => {
    const ok = handler(fakeBridge(true));
    expect(await ok.handle({ type: "bridge-call", call: "pullBackup", payload: { section: "accounts", offset: 0 } }))
      .toEqual({ ok: true, value: { echo: "pullBackup" } });
    const failing = handler(fakeBridge(true, new BridgeError("E_PATH")));
    expect(await failing.handle({ type: "bridge-call", call: "saveExport", payload: {} })).toEqual({ ok: false, error: "E_PATH" });
    expect(await ok.handle({ type: "bridge-flush" })).toEqual({ ok: true, value: null });
    expect(ok.flushSoon).toHaveBeenCalledWith(500);
  });

  it("turns a reply into a value or a BridgeError on the page side", async () => {
    expect(await ask({ type: "bridge-flush" }, async () => ({ ok: true, value: 7 }))).toBe(7);
    await expect(ask({ type: "bridge-flush" }, async () => ({ ok: false, error: "E_TIMEOUT" }))).rejects.toMatchObject({ code: "E_TIMEOUT" });
    await expect(ask({ type: "bridge-flush" }, async () => undefined)).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
  });
});
