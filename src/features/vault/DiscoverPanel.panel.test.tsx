// @vitest-environment jsdom
import { open } from "@tauri-apps/plugin-dialog";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { ToastHost, ToastProvider } from "../../ui";
import type { DiscoverStatus, Finding } from "./api";
import { DiscoverPanel } from "./DiscoverPanel";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn(), reportFrontendError: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

function finding(id: number, status: Finding["status"]): Finding {
  return { id, source: "dotenv", location: `D:\\p${id}\\.env`, name: `KEY_${id}`, preview: "abcd•••", platform: "GitHub", kind: "token", risks: [], status };
}
const scanned: DiscoverStatus = { running: false, cancelled: false, truncated: false, files: 7, findings: [finding(1, "new"), finding(2, "in_vault")] };
const idle: DiscoverStatus = { ...scanned, findings: [] };

function mockBackend(handlers: Record<string, (args: unknown) => unknown>) {
  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    const handler = handlers[command];
    if (!handler) throw new Error(`unexpected ${command}`);
    return handler(args);
  });
}
const render = () => act(async () => root.render(<ToastProvider><DiscoverPanel onImported={vi.fn()} /><ToastHost /></ToastProvider>));
const checkboxes = () => [...document.body.querySelectorAll<HTMLInputElement>(".vault-field input[type=checkbox]")];
const button = (text: string) => [...document.body.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.includes(text))!;

describe("DiscoverPanel results", () => {
  it("pre-selects nothing, locks in-vault findings and imports only the checked one", async () => {
    mockBackend({
      settings_get: () => ({ vault_auto_lock_minutes: 10, vault_scan_dirs: [] }),
      vault_discover_status: () => scanned,
      vault_discover_import: () => 1,
    });
    await render();
    expect(checkboxes().length).toBe(2);
    expect(checkboxes().every((box) => !box.checked)).toBe(true);
    expect(checkboxes()[0].disabled).toBe(false);
    expect(checkboxes()[1].disabled).toBe(true);
    expect(button("导入所选").disabled).toBe(true);

    await act(async () => checkboxes()[0].click());
    expect(checkboxes()[0].checked).toBe(true);
    await act(async () => button("导入所选").click());
    expect(invoke).toHaveBeenCalledWith("vault_discover_import", { items: [{ id: 1, platform: "GitHub", kind: "token" }], notePrefix: "来源：" });
    expect(checkboxes().every((box) => !box.checked)).toBe(true);
  });
});

describe("DiscoverPanel project folders", () => {
  it("re-reads settings before saving and sends the fresh auto-lock minutes", async () => {
    let reads = 0;
    mockBackend({
      settings_get: () => (++reads === 1
        ? { vault_auto_lock_minutes: 10, vault_scan_dirs: [] }
        : { vault_auto_lock_minutes: 30, vault_scan_dirs: ["D:\\other"] }),
      vault_discover_status: () => idle,
      settings_set_vault: (args) => ({ vault_auto_lock_minutes: (args as { autoLockMinutes: number }).autoLockMinutes, vault_scan_dirs: (args as { scanDirs: string[] }).scanDirs }),
    });
    vi.mocked(open).mockResolvedValue("  D:\\proj  ");
    await render();
    await act(async () => button("添加文件夹").click());
    expect(invoke).toHaveBeenCalledWith("settings_set_vault", { autoLockMinutes: 30, scanDirs: ["D:\\other", "D:\\proj"] });
    expect(document.body.textContent).toContain("D:\\proj");
  });

  it("skips a folder that is already listed", async () => {
    mockBackend({
      settings_get: () => ({ vault_auto_lock_minutes: 10, vault_scan_dirs: ["D:\\proj"] }),
      vault_discover_status: () => idle,
    });
    vi.mocked(open).mockResolvedValue("D:\\proj ");
    await render();
    await act(async () => button("添加文件夹").click());
    expect(invoke).not.toHaveBeenCalledWith("settings_set_vault", expect.anything());
  });

  it("keeps folder buttons disabled and toasts when settings fail to load", async () => {
    vi.mocked(invoke).mockImplementation((command: string) =>
      command === "settings_get" ? Promise.reject("E_VAULT_IO") : Promise.resolve(idle));
    await render();
    expect(button("添加文件夹").disabled).toBe(true);
    expect(document.body.textContent).toContain("读写文件失败");
  });

  it("toasts when the folder dialog fails", async () => {
    mockBackend({
      settings_get: () => ({ vault_auto_lock_minutes: 10, vault_scan_dirs: [] }),
      vault_discover_status: () => idle,
    });
    vi.mocked(open).mockRejectedValue("E_VAULT_IO");
    await render();
    await act(async () => button("添加文件夹").click());
    expect(document.body.textContent).toContain("读写文件失败");
  });
});
