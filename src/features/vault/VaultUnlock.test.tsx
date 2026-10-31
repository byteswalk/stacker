// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../../invoke";
import { ToastHost, ToastProvider } from "../../ui";
import { VaultUnlock } from "./VaultUnlock";
import type { VaultStatus } from "./api";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn(), reportFrontendError: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn(), open: vi.fn() }));

const KEY = "AB12-CD34-EF56-GH78-JK90-MN12-PQ34-RS56";
const status = (waitSeconds: number): VaultStatus => ({ exists: true, state: "locked", waitSeconds, pending: false });
let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.mocked(invoke).mockImplementation(async (command) => command === "vault_recovery_file" ? "my-key.txt" : command === "vault_read_recovery_file" ? KEY : undefined);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

const button = (label: string) => [...host.querySelectorAll("button")].find((item) => item.textContent?.includes(label))!;
const render = (seconds: number, onChanged = vi.fn()) => act(async () => root.render(<ToastProvider><VaultUnlock status={status(seconds)} onChanged={onChanged} /><ToastHost /></ToastProvider>));

function type(input: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("VaultUnlock", () => {
  it("counts the wait down second by second, holds the button, and re-reads the status at zero", async () => {
    vi.useFakeTimers();
    const onChanged = vi.fn();
    await render(30, onChanged);
    await act(async () => type(host.querySelector("input")!, "a-password"));
    expect(host.querySelector(".vault-warn")?.textContent).toContain("30");
    expect(button("解锁").disabled).toBe(true);

    await act(async () => { vi.advanceTimersByTime(1000); });
    expect(host.querySelector(".vault-warn")?.textContent).toContain("29");
    await act(async () => { vi.advanceTimersByTime(18_000); });
    expect(host.querySelector(".vault-warn")?.textContent).toContain("11");
    expect(onChanged).not.toHaveBeenCalled();

    await act(async () => { vi.advanceTimersByTime(11_000); });
    expect(host.querySelector(".vault-warn")).toBeNull();
    expect(button("解锁").disabled).toBe(false);
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it("names the recovery key's file only when the key is being used, and reads the key from a file", async () => {
    vi.mocked(open).mockResolvedValue("E:\\keys\\my-key.txt");
    await render(0);
    expect(host.textContent).not.toContain("my-key.txt");
    await act(async () => button("使用恢复密钥").click());
    expect(host.textContent).toContain("my-key.txt");

    await act(async () => button("从文件读取").click());
    expect(invoke).toHaveBeenCalledWith("vault_read_recovery_file", { path: "E:\\keys\\my-key.txt" });
    expect(host.querySelector<HTMLInputElement>(".vault-keyin-group input")!.value).toBe("AB12");
    expect(button("解锁").disabled).toBe(false);
  });

  it("does not guess a file name for a key that was never saved to one", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    await render(0);
    await act(async () => button("使用恢复密钥").click());
    expect(host.textContent).not.toContain("恢复密钥文件名");
  });
});
