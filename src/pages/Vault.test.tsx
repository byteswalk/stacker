// @vitest-environment jsdom
import { act, useEffect, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../invoke";
import { listen } from "@tauri-apps/api/event";
import { ToastHost, ToastProvider, useToast } from "../ui";
import Vault from "./Vault";

vi.mock("../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn(), reportFrontendError: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));

let host: HTMLDivElement;
let root: Root;

function answer(state: string, entries: unknown[] = []) {
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "vault_status") return { exists: state !== "missing", state, waitSeconds: 0, pending: false };
    if (command === "settings_get") return { vault_auto_lock_minutes: 10, vault_scan_dirs: [] };
    if (command === "vault_list") return entries;
    if (command === "vault_discover_status") return { running: false, cancelled: false, truncated: false, files: 0, findings: [] };
    return undefined;
  });
}

const ENTRY = {
  id: "e1", title: "Coding Plan", platform: "火山方舟", kind: "token_plan", fields: [], expiresAt: null, tags: [], note: "",
  favorite: false, createdAt: 1, updatedAt: 1, deletedAt: null, historyCount: 0, ssh: null,
};

const probe: { push: (msg: string, kind?: "ok" | "err" | "info") => void } = { push: () => undefined };
function ToastProbe() { const push = useToast(); useEffect(() => { probe.push = push; }, [push]); return null; }

async function render(extra: ReactNode = null) {
  await act(async () => root.render(<ToastProvider>{extra}<Vault /></ToastProvider>));
}

const listCalls = () => vi.mocked(invoke).mock.calls.filter(([command]) => command === "vault_list").length;
const lockedHandler = () => vi.mocked(listen).mock.calls[0][1] as (event: { payload: string }) => unknown;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.mocked(listen).mockImplementation(async () => () => undefined);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

describe("Vault page", () => {
  it("asks for a master password when no vault exists", async () => {
    answer("missing");
    await render();
    expect(host.textContent).toContain("设置主密码");
    expect(host.textContent).toContain("从备份文件恢复");
  });

  it("shows only the unlock form while locked", async () => {
    answer("locked", [{ id: "x", title: "should not show" }]);
    await render();
    expect(host.textContent).toContain("解锁保管库");
    expect(host.textContent).not.toContain("should not show");
    expect(vi.mocked(invoke).mock.calls.some(([command]) => command === "vault_list")).toBe(false);
  });

  it("lists entries once unlocked", async () => {
    answer("unlocked", [{
      id: "e1", title: "Coding Plan", platform: "火山方舟", kind: "token_plan", fields: [], expiresAt: null, tags: [], note: "",
      favorite: false, createdAt: 1, updatedAt: 1, deletedAt: null, historyCount: 0, ssh: null,
    }]);
    await render();
    expect(host.textContent).toContain("Coding Plan");
    expect(host.textContent).toContain("发现");
  });

  it("does not reload entries when a toast is pushed", async () => {
    answer("unlocked", [ENTRY]);
    await render(<ToastProbe />);
    expect(listCalls()).toBe(1);
    await act(async () => probe.push("anything", "info"));
    await act(async () => probe.push("something else", "info"));
    expect(host.textContent).toContain("Coding Plan");
    expect(listCalls()).toBe(1);
  });

  it("tries a failing entry list once and shows one error", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "vault_status") return { exists: true, state: "unlocked", waitSeconds: 0, pending: false };
      if (command === "vault_list") throw new Error("E_VAULT_IO");
      return undefined;
    });
    await render(<ToastHost />);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
    expect(listCalls()).toBe(1);
    expect(host.querySelectorAll(".toast")).toHaveLength(1);
  });

  it("drops the workspace and shows the unlock form when vault-locked fires", async () => {
    answer("unlocked", [ENTRY]);
    await render();
    expect(host.textContent).toContain("Coding Plan");
    answer("locked");
    await act(async () => { await lockedHandler()({ payload: "idle" }); });
    expect(host.textContent).toContain("解锁保管库");
    expect(host.textContent).not.toContain("Coding Plan");
    expect(host.textContent).not.toContain("新建");
  });

  it("unlistens when listen resolves after the page unmounted", async () => {
    const unlisten = vi.fn();
    let resolveListen: (value: () => void) => void = () => undefined;
    vi.mocked(listen).mockImplementation(() => new Promise((resolve) => { resolveListen = resolve as never; }));
    answer("locked");
    await render();
    act(() => root.unmount());
    expect(unlisten).not.toHaveBeenCalled();
    await act(async () => resolveListen(unlisten));
    expect(unlisten).toHaveBeenCalledTimes(1);
    root = createRoot(host);
  });

  it("opens recovering vaults on the new-password step and cancels through the backend", async () => {
    answer("recovering", [ENTRY]);
    await render();
    expect(host.textContent).toContain("设置新的主密码");
    expect(host.textContent).not.toContain("Coding Plan");
    const statusCalls = () => vi.mocked(invoke).mock.calls.filter(([command]) => command === "vault_status").length;
    const before = statusCalls();
    const cancel = [...host.querySelectorAll("button")].find((button) => button.textContent === "取消")!;
    await act(async () => cancel.click());
    const commands = vi.mocked(invoke).mock.calls.map(([command]) => command);
    expect(commands.lastIndexOf("vault_cancel_pending")).toBeGreaterThan(-1);
    expect(statusCalls()).toBe(before + 1);
    expect(commands.indexOf("vault_cancel_pending")).toBeLessThan(commands.lastIndexOf("vault_status"));
  });
});
