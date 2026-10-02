// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { ToastHost, ToastProvider } from "../../ui";
import { useVaultLockNotice } from "./useVaultLockNotice";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn(), reportFrontendError: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

function Notice() { useVaultLockNotice(); return null; }

let host: HTMLDivElement;
let root: Root;
let fire: (reason: string) => Promise<void>;
let fireEvent: (name: string, payload: unknown) => Promise<void>;
let unlisten: ReturnType<typeof vi.fn>;

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  unlisten = vi.fn();
  const handlers = new Map<string, (event: { payload: unknown }) => unknown>();
  vi.mocked(listen).mockImplementation((async (name: string, callback: (event: { payload: unknown }) => unknown) => { handlers.set(name, callback); return unlisten; }) as never);
  fireEvent = (name, payload) => act(async () => { await handlers.get(name)?.({ payload }); });
  fire = (reason) => fireEvent("vault-locked", reason);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<ToastProvider><ToastHost /><Notice /></ToastProvider>));
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

describe("useVaultLockNotice", () => {
  it("names the configured idle minutes", async () => {
    vi.mocked(invoke).mockResolvedValue({ vault_auto_lock_minutes: 5, vault_scan_dirs: [] });
    await fire("idle");
    expect(host.textContent).toContain("空闲 5 分钟，保管库已锁定。");
  });

  it("explains session and sleep locks, and falls back to 10 minutes when settings fail", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("boom"));
    await fire("session");
    expect(host.textContent).toContain("Windows 已锁屏，保管库已锁定。");
    await fire("sleep");
    expect(host.textContent).toContain("系统曾进入睡眠，保管库已锁定。");
    await fire("idle");
    expect(host.textContent).toContain("空闲 10 分钟，保管库已锁定。");
  });

  it("says how many logins came in from the browser", async () => {
    await fireEvent("vault-inbox", 2);
    expect(host.textContent).toContain("已从浏览器收进 2 条登录。");
  });
});
