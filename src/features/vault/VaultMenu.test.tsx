// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { ToastHost, ToastProvider } from "../../ui";
import { AutoLockDialog } from "./VaultMenu";

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
