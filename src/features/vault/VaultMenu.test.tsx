// @vitest-environment jsdom
import { StrictMode, act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { ToastHost, ToastProvider } from "../../ui";
import { AutoLockDialog, ResetRecoveryDialog } from "./VaultMenu";

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

const choices = () => [...document.body.querySelectorAll<HTMLButtonElement>(".seg button")];
const render = () => act(async () => root.render(<ToastProvider><AutoLockDialog onClose={vi.fn()} /><ToastHost /></ToastProvider>));

describe("AutoLockDialog", () => {
  it("keeps the choices disabled until settings load and re-reads scan dirs when saving", async () => {
    let release!: (value: unknown) => void;
    const settings = { vault_auto_lock_minutes: 10, vault_scan_dirs: ["D:\\old"] };
    vi.mocked(invoke).mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    await render();
    expect(choices().length).toBe(4);
    expect(choices().every((b) => b.disabled)).toBe(true);

    // First load resolves with the old folders.
    await act(async () => release(settings));
    expect(choices().every((b) => !b.disabled)).toBe(true);
    expect(choices().find((b) => b.classList.contains("on"))?.textContent).toContain("10");

    // Folders change elsewhere before the click: the save must send the freshly re-read list.
    const fresh = { vault_auto_lock_minutes: 10, vault_scan_dirs: ["D:\\fresh"] };
    vi.mocked(invoke).mockImplementation((command: string) =>
      Promise.resolve(command === "settings_get" ? fresh : { vault_auto_lock_minutes: 30, vault_scan_dirs: ["D:\\fresh"] }));
    await act(async () => choices().find((b) => b.textContent?.includes("30"))!.click());
    expect(invoke).toHaveBeenCalledWith("settings_set_vault", { autoLockMinutes: 30, scanDirs: ["D:\\fresh"] });
    expect(choices().find((b) => b.classList.contains("on"))?.textContent).toContain("30");
  });

  it("stays disabled and shows the error when settings fail to load", async () => {
    vi.mocked(invoke).mockRejectedValue("E_VAULT_IO");
    await render();
    expect(choices().every((b) => b.disabled)).toBe(true);
    expect(document.body.textContent).toContain("读写文件失败");
  });
});

describe("ResetRecoveryDialog", () => {
  const renderReset = () => act(async () => root.render(<StrictMode><ToastProvider><ResetRecoveryDialog onClose={vi.fn()} /><ToastHost /></ToastProvider></StrictMode>));
  const calls = (command: string) => vi.mocked(invoke).mock.calls.filter(([name]) => name === command).length;

  async function issueKey() {
    vi.mocked(invoke).mockImplementation(async (command: string) => (command === "vault_reset_recovery" ? "AAAA-BBBB" : undefined));
    await renderReset();
    const input = document.body.querySelector<HTMLInputElement>("input[type=password]")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "master-password");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => [...document.body.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "继续")!.click());
    expect(calls("vault_reset_recovery")).toBe(1);
  }

  it("does not cancel anything when it unmounts before a key was issued (StrictMode safe)", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    await renderReset();
    act(() => root.unmount());
    root = createRoot(host);
    expect(calls("vault_cancel_pending")).toBe(0);
  });

  it("cancels the pending rotation when it unmounts after a key was issued but not confirmed", async () => {
    await issueKey();
    expect(calls("vault_cancel_pending")).toBe(0);
    act(() => root.unmount());
    root = createRoot(host);
    expect(calls("vault_cancel_pending")).toBe(1);
  });
});
