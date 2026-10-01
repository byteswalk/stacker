// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../invoke";
import { ToastProvider } from "../ui";
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

async function render() {
  await act(async () => root.render(<ToastProvider><Vault /></ToastProvider>));
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
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
});
