// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { ToastHost, ToastProvider } from "../../ui";
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

  async function renderStep(onConfirmed = vi.fn(), onCancel = vi.fn()) {
    await act(async () => root.render(<ToastProvider><RecoveryKeyStep recoveryKey={KEY} onConfirmed={onConfirmed} onCancel={onCancel} /><ToastHost /></ToastProvider>));
  }
  const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text))!;

  it("marks the key box as untranslatable", async () => {
    await renderStep();
    const box = [...host.querySelectorAll("div")].find((el) => el.children.length === 0 && el.textContent === KEY)!;
    expect(box.getAttribute("translate")).toBe("no");
  });

  it("cancels the pending key on the backend before calling onCancel", async () => {
    const onCancel = vi.fn();
    await renderStep(vi.fn(), onCancel);
    await act(async () => button("取消").click());
    expect(invoke).toHaveBeenCalledWith("vault_cancel_pending");
    expect(onCancel).toHaveBeenCalled();
    expect(vi.mocked(invoke).mock.invocationCallOrder[0]).toBeLessThan(onCancel.mock.invocationCallOrder[0]);
  });

  it("enables done only for exactly four characters", async () => {
    await renderStep();
    const input = host.querySelector("input")!;
    for (const [text, enabled] of [["a", false], ["ab", false], ["abc", false], ["abcd", true]] as const) {
      await act(async () => setInput(input, text));
      expect(button("完成").disabled).toBe(!enabled);
    }
  });

  it("shows the mapped error and stays put when confirming fails", async () => {
    const onConfirmed = vi.fn();
    await renderStep(onConfirmed);
    vi.mocked(invoke).mockRejectedValueOnce("E_VAULT_CONFIRM");
    await act(async () => setInput(host.querySelector("input")!, "zzzz"));
    await act(async () => button("完成").click());
    expect(document.body.textContent).toContain("输入的字符与恢复密钥最后一组不一致。");
    expect(onConfirmed).not.toHaveBeenCalled();
  });
});
