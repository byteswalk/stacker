import { BridgeError, type Bridge } from "./bridge";
import { outboxCount, type Db } from "./db";

export interface BridgeStatus {
  connected: boolean;
  pending: number;
  lastSyncAt: number | null;
  error: string;
  /** Stacker's appearance ("dark" | "light" | "system"), so both sides can match it. */
  theme: string | null;
}
/** The only Stacker requests pages may make; syncing stays inside the background. */
export type StackerCall = "saveExport" | "pullBackup" | "distillResults" | "setTheme";
export type BridgeMessage =
  | { type: "bridge-status"; connect: boolean; force: boolean }
  | { type: "bridge-flush" }
  | { type: "bridge-call"; call: StackerCall; payload: unknown };
export type BridgeReply = { ok: true; value: unknown } | { ok: false; error: string };

const CALLS: StackerCall[] = ["saveExport", "pullBackup", "distillResults", "setTheme"];

export function isBridgeMessage(m: unknown): m is BridgeMessage {
  const x = m as { type?: unknown; call?: unknown } | null;
  if (!x) return false;
  if (x.type === "bridge-status" || x.type === "bridge-flush") return true;
  return x.type === "bridge-call" && CALLS.includes(x.call as StackerCall);
}

export interface HandlerDeps {
  bridge: Bridge;
  openDb: () => Promise<Db>;
  flushSoon: (delayMs: number) => void;
  lastSyncAt: () => number | null;
}

/** Background side: answers the manage page and popup. */
export function createBridgeHandler(deps: HandlerDeps) {
  return async (m: BridgeMessage): Promise<BridgeReply> => {
    try {
      if (m.type === "bridge-flush") {
        deps.flushSoon(500);
        return { ok: true, value: null };
      }
      if (m.type === "bridge-call") return { ok: true, value: await deps.bridge.call(m.call, m.payload) };
      if (m.force) deps.bridge.reset();
      // One `status` answers both questions: whether Stacker is there, and what appearance it is on.
      let theme: string | null = null;
      if (m.connect || deps.bridge.connected()) {
        try {
          const reply = (await deps.bridge.call("status")) as { theme?: unknown } | null;
          if (typeof reply?.theme === "string") theme = reply.theme;
        } catch { /* reported below as not connected */ }
      }
      const pending = await outboxCount(await deps.openDb());
      if (pending && deps.bridge.connected()) deps.flushSoon(0);
      const status: BridgeStatus = { connected: deps.bridge.connected(), pending, lastSyncAt: deps.lastSyncAt(), error: deps.bridge.lastError(), theme };
      return { ok: true, value: status };
    } catch (e) {
      return { ok: false, error: e instanceof BridgeError ? e.code : e instanceof Error ? e.message : String(e) };
    }
  };
}

type Send = (message: BridgeMessage) => Promise<unknown>;
const viaRuntime: Send = (message) => chrome.runtime.sendMessage(message);

/** Page side: one message to the background, its reply as a value or a BridgeError. */
export async function ask<T>(message: BridgeMessage, send: Send = viaRuntime): Promise<T> {
  const reply = (await send(message)) as BridgeReply | undefined;
  if (!reply) throw new BridgeError("E_NOT_CONNECTED");
  if (!reply.ok) throw new BridgeError(reply.error);
  return reply.value as T;
}

export const bridgeStatus = (connect: boolean, force = false, send?: Send) =>
  ask<BridgeStatus>({ type: "bridge-status", connect, force }, send);
export const callStacker = (call: StackerCall, payload: unknown, send?: Send) =>
  ask<unknown>({ type: "bridge-call", call, payload }, send);
export function requestFlush(send: Send = viaRuntime): void {
  void send({ type: "bridge-flush" }).catch(() => {});
}
