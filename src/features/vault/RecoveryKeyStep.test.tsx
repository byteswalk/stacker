// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { ToastProvider } from "../../ui";
import { RecoveryKeyStep } from "./RecoveryKeyStep";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn(), reportFrontendError: vi.fn() }));

const KEY = "AB12-CD34-EF56-GH78-JK90-MN12-PQ34-RS56";
let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.mocked(invoke).mockResolvedValue(undefined);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

function setInput(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  setter.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("RecoveryKeyStep", () => {
  it("shows the key, copies through the backend and confirms with the last group", async () => {
    const onConfirmed = vi.fn();
    await act(async () => root.render(<ToastProvider><RecoveryKeyStep recoveryKey={KEY} onConfirmed={onConfirmed} onCancel={vi.fn()} /></ToastProvider>));
    expect(host.textContent).toContain(KEY);
    expect(host.textContent).toContain("微信收藏");

    const copy = [...host.querySelectorAll("button")].find((button) => button.textContent?.includes("复制"))!;
    await act(async () => copy.click());
    expect(invoke).toHaveBeenCalledWith("vault_copy_recovery");

    const done = [...host.querySelectorAll("button")].find((button) => button.textContent?.includes("完成"))!;
    expect(done.disabled).toBe(true);
    await act(async () => setInput(host.querySelector("input")!, "rs56"));
    expect(done.disabled).toBe(false);
    await act(async () => done.click());
    expect(invoke).toHaveBeenCalledWith("vault_confirm_recovery", { lastGroup: "rs56" });
    expect(onConfirmed).toHaveBeenCalled();
  });
});
