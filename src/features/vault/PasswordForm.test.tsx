// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PasswordForm } from "./PasswordForm";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
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

async function fill(password: string, confirm: string) {
  const inputs = host.querySelectorAll("input");
  await act(async () => setInput(inputs[0], password));
  await act(async () => setInput(inputs[1], confirm));
  await act(async () => host.querySelector<HTMLButtonElement>("button[type=submit]")!.click());
}

describe("PasswordForm", () => {
  it("rejects a password shorter than 9 characters", async () => {
    const onSubmit = vi.fn();
    await act(async () => root.render(<PasswordForm submitLabel="创建" busy={false} onSubmit={onSubmit} />));
    await fill("short-pw", "short-pw");
    expect(host.textContent).toContain("主密码至少需要 9 个字符。");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("rejects mismatching confirmation", async () => {
    const onSubmit = vi.fn();
    await act(async () => root.render(<PasswordForm submitLabel="创建" busy={false} onSubmit={onSubmit} />));
    await fill("correct horse battery", "correct horse batterz");
    expect(host.textContent).toContain("两次输入的主密码不一致。");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("submits a valid matching password", async () => {
    const onSubmit = vi.fn();
    await act(async () => root.render(<PasswordForm submitLabel="创建" busy={false} onSubmit={onSubmit} />));
    await fill("correct horse battery", "correct horse battery");
    expect(onSubmit).toHaveBeenCalledWith("correct horse battery");
  });
});
