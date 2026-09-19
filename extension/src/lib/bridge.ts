/** Native messaging with Stacker: connect on demand, one request per id, standalone when unavailable. */
export const HOST_NAME = "com.stacker.webchat";
export const REQUEST_TIMEOUT_MS = 30_000;
export const IDLE_CLOSE_MS = 120_000;
export const RETRY_AFTER_MS = 30_000;

export class BridgeError extends Error {
  code: string;
  constructor(code: string) {
    super(code);
    this.code = code;
  }
}

export interface NativePort {
  postMessage(message: unknown): void;
  disconnect(): void;
  onMessage: { addListener(fn: (message: unknown) => void): void };
  onDisconnect: { addListener(fn: (reason: string) => void): void };
}

export interface HelloResult {
  app: string;
  version: string;
  protocol: number;
  counts: { accounts: number; conversations: number; bodies: number; folders: number; excerpts: number };
}

export interface BridgeOptions {
  timeoutMs?: number;
  idleMs?: number;
  retryMs?: number;
  now?: () => number;
  version?: string;
  onConnected?: (hello: HelloResult) => void;
}

export interface Bridge {
  call(type: string, payload?: unknown): Promise<unknown>;
  connected(): boolean;
  lastError(): string;
  /** Forget a recent failure so the next call tries at once. */
  reset(): void;
}

interface Pending { resolve: (value: unknown) => void; reject: (error: unknown) => void; timer: ReturnType<typeof setTimeout> }

export function createBridge(connect: () => NativePort, options: BridgeOptions = {}): Bridge {
  const timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS;
  const idleMs = options.idleMs ?? IDLE_CLOSE_MS;
  const retryMs = options.retryMs ?? RETRY_AFTER_MS;
  const now = options.now ?? Date.now;
  let port: NativePort | null = null;
  let ready = false;
  let opening: Promise<void> | null = null;
  let failedAt = Number.NEGATIVE_INFINITY;
  let error = "";
  let seq = 0;
  let idleTimer: ReturnType<typeof setTimeout> | undefined;
  const pending = new Map<string, Pending>();

  function lost(reason: string) {
    port = null;
    ready = false;
    error = reason || "E_NOT_CONNECTED";
    failedAt = now();
    clearTimeout(idleTimer);
    for (const p of pending.values()) { clearTimeout(p.timer); p.reject(new BridgeError("E_NOT_CONNECTED")); }
    pending.clear();
  }

  function send(p: NativePort, type: string, payload: unknown): Promise<unknown> {
    const id = String(++seq);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new BridgeError("E_TIMEOUT")); }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      try {
        p.postMessage({ id, type, payload });
      } catch {
        clearTimeout(timer);
        pending.delete(id);
        reject(new BridgeError("E_NOT_CONNECTED"));
      }
    });
  }

  function onMessage(message: unknown) {
    const m = message as { id?: unknown; ok?: unknown; result?: unknown; error?: unknown } | null;
    if (!m || typeof m.id !== "string") return;
    const entry = pending.get(m.id);
    if (!entry) return;
    pending.delete(m.id);
    clearTimeout(entry.timer);
    if (m.ok === true) entry.resolve(m.result);
    else entry.reject(new BridgeError(typeof m.error === "string" ? m.error : "E_BRIDGE"));
  }

  async function open(): Promise<void> {
    let p: NativePort;
    try {
      p = connect();
    } catch (e) {
      lost(e instanceof Error ? e.message : String(e));
      throw new BridgeError("E_NOT_CONNECTED");
    }
    port = p;
    p.onMessage.addListener(onMessage);
    p.onDisconnect.addListener((reason) => { if (port === p) lost(reason); });
    try {
      const hello = (await send(p, "hello", { version: options.version ?? "" })) as HelloResult;
      if (port !== p) throw new BridgeError("E_NOT_CONNECTED");
      ready = true;
      error = "";
      options.onConnected?.(hello);
    } catch (e) {
      if (port === p) {
        p.disconnect();
        lost(e instanceof BridgeError ? e.code : String(e));
      }
      throw new BridgeError("E_NOT_CONNECTED");
    }
  }

  async function ensure(): Promise<NativePort> {
    if (port && ready) return port;
    if (!opening) {
      if (now() - failedAt < retryMs) throw new BridgeError("E_NOT_CONNECTED");
      opening = open().finally(() => { opening = null; });
    }
    await opening;
    if (!port) throw new BridgeError("E_NOT_CONNECTED");
    return port;
  }

  function touch() {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => {
      if (pending.size) { touch(); return; }
      const p = port;
      port = null;
      ready = false;
      p?.disconnect();
    }, idleMs);
  }

  return {
    async call(type, payload = {}) {
      const p = await ensure();
      touch();
      return send(p, type, payload);
    },
    connected: () => port !== null && ready,
    lastError: () => error,
    reset: () => { failedAt = Number.NEGATIVE_INFINITY; },
  };
}

/** Chrome's port, adapted: the disconnect reason comes from `chrome.runtime.lastError`. */
export function connectChrome(): NativePort {
  const port = chrome.runtime.connectNative(HOST_NAME);
  return {
    postMessage: (message) => port.postMessage(message),
    disconnect: () => port.disconnect(),
    onMessage: { addListener: (fn) => port.onMessage.addListener((message: unknown) => fn(message)) },
    onDisconnect: { addListener: (fn) => port.onDisconnect.addListener(() => fn(chrome.runtime.lastError?.message ?? "")) },
  };
}
