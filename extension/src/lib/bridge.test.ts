import { afterEach, describe, expect, it, vi } from "vitest";
import { createBridge, type HelloResult, type NativePort } from "./bridge";

type Sent = { id: string; type: string; payload: unknown };
type Reply = (req: Sent) => unknown;

const HELLO: HelloResult = { app: "stacker", version: "0.3.3", protocol: 1, counts: { accounts: 0, conversations: 0, bodies: 0, folders: 0, excerpts: 0 } };

class FakePort implements NativePort {
  reply: Reply;
  sent: Sent[] = [];
  disconnected = false;
  private messageListeners: ((m: unknown) => void)[] = [];
  private disconnectListeners: ((reason: string) => void)[] = [];
  constructor(reply: Reply) { this.reply = reply; }
  onMessage = { addListener: (fn: (m: unknown) => void) => { this.messageListeners.push(fn); } };
  onDisconnect = { addListener: (fn: (reason: string) => void) => { this.disconnectListeners.push(fn); } };
  postMessage(message: unknown) {
    const req = message as Sent;
    this.sent.push(req);
    const answer = this.reply(req);
    if (answer !== undefined) queueMicrotask(() => this.messageListeners.forEach((l) => l(answer)));
  }
  disconnect() { this.disconnected = true; }
  drop(reason: string) { this.disconnectListeners.forEach((l) => l(reason)); }
}

function fakeHost(reply: Reply) {
  const ports: FakePort[] = [];
  const connect = vi.fn(() => { const p = new FakePort(reply); ports.push(p); return p; });
  return { connect, ports };
}
const answering: Reply = (req) => ({ id: req.id, ok: true, result: req.type === "hello" ? HELLO : { echo: req.type } });

afterEach(() => { vi.useRealTimers(); });

describe("bridge", () => {
  it("says hello once, then sends each request with its own id", async () => {
    const { connect, ports } = fakeHost(answering);
    const onConnected = vi.fn();
    const bridge = createBridge(connect, { onConnected });
    expect(await bridge.call("status")).toEqual({ echo: "status" });
    expect(await bridge.call("pullBackup", { section: "accounts", offset: 0 })).toEqual({ echo: "pullBackup" });
    expect(connect).toHaveBeenCalledTimes(1);
    expect(ports[0].sent.map((m) => m.type)).toEqual(["hello", "status", "pullBackup"]);
    expect(new Set(ports[0].sent.map((m) => m.id)).size).toBe(3);
    expect(bridge.connected()).toBe(true);
    expect(onConnected).toHaveBeenCalledWith(HELLO);
  });

  it("passes Stacker's error code through", async () => {
    const { connect } = fakeHost((req) => req.type === "hello" ? answering(req) : { id: req.id, ok: false, error: "E_PATH" });
    const bridge = createBridge(connect);
    await expect(bridge.call("saveExport", { path: "../x.md" })).rejects.toMatchObject({ code: "E_PATH" });
    expect(bridge.connected()).toBe(true);
  });

  it("treats a missing host as standalone and waits before trying again", async () => {
    let now = 0;
    const { connect, ports } = fakeHost(() => undefined);
    const bridge = createBridge(connect, { now: () => now, retryMs: 1000 });
    const first = bridge.call("status");
    ports[0].drop("Specified native messaging host not found.");
    await expect(first).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
    expect(bridge.connected()).toBe(false);
    expect(bridge.lastError()).toContain("not found");
    await expect(bridge.call("status")).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
    expect(connect).toHaveBeenCalledTimes(1);
    bridge.reset();
    const retried = bridge.call("status");
    ports[1].drop("Specified native messaging host not found.");
    await expect(retried).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
    now = 1000;
    const later = bridge.call("status");
    ports[2].drop("gone");
    await expect(later).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
    expect(connect).toHaveBeenCalledTimes(3);
  });

  it("times out when Stacker does not answer", async () => {
    vi.useFakeTimers();
    const { connect } = fakeHost((req) => req.type === "hello" ? answering(req) : undefined);
    const bridge = createBridge(connect, { timeoutMs: 100 });
    const pending = bridge.call("status");
    const check = expect(pending).rejects.toMatchObject({ code: "E_TIMEOUT" });
    await vi.advanceTimersByTimeAsync(100);
    await check;
  });

  it("closes an idle connection and reconnects on the next call", async () => {
    vi.useFakeTimers();
    const { connect, ports } = fakeHost(answering);
    const bridge = createBridge(connect, { idleMs: 1000 });
    await bridge.call("status");
    await vi.advanceTimersByTimeAsync(1000);
    expect(ports[0].disconnected).toBe(true);
    expect(bridge.connected()).toBe(false);
    await bridge.call("status");
    expect(connect).toHaveBeenCalledTimes(2);
    expect(ports[1].sent[0].type).toBe("hello");
  });
});
