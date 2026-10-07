// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { TransferImport } from "./TransferDialogs";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => "C:\\move.zip"), save: vi.fn() }));

const preview = (codexRunning: boolean) => ({
  machine: "PC-A", createdAt: 1791385530,
  sessions: [
    { agent: "codex", id: "t1", title: "Plan the upgrade", cwd: "E:\\app", updatedAt: 1, present: false },
    { agent: "claude", id: "c1", title: "Fix the build", cwd: "E:\\app", updatedAt: 1, present: true },
  ],
  projects: [{ path: "E:\\app", name: "app", included: true, files: 12, bytes: 4096, existsHere: false }],
  accounts: [{ agent: "codex", same: false, packed: true, here: true }, { agent: "claude", same: true, packed: true, here: true }],
  codexRunning, codexReady: true,
});

let host: HTMLDivElement;
let root: Root;
let running = true;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  running = true;
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation((async (command: string) => {
    if (command === "sessions_transfer_preview") return preview(running);
    if (command === "sessions_transfer_import") return {
      imported: [{ agent: "codex", id: "t1", title: "Plan the upgrade", cwd: "D:\\work\\app", resume: "codex resume t1 -C \"D:\\work\\app\"" }],
      present: ["Fix the build"], stripped: ["codex"], dropped: 3, extracted: 12, kept: 0, backup: "C:\\backups\\x",
    };
    return null;
  }) as typeof invoke);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const text = () => document.body.textContent ?? "";
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((item) => item.textContent?.includes(label))!;

describe("importing a session move package", () => {
  it("says what each account means, waits for Codex to close, and imports into the place given", async () => {
    await act(async () => { root.render(<TransferImport onClose={() => {}} />); });
    await act(async () => { button("选择文件").click(); });
    expect(text()).toContain("两台电脑登录的不是同一个账号");
    expect(text()).toContain("两台电脑登录的是同一个账号，完整导入。");
    expect(text()).toContain("这台电脑已有");
    // Codex runs: nothing can be imported until it is closed.
    expect(button("导入 1 个会话").disabled).toBe(true);
    running = false;
    await act(async () => { button("我已退出 Codex，重新检查").click(); });
    expect(button("导入 1 个会话").disabled).toBe(false);

    const input = document.querySelector<HTMLInputElement>(".transfer-target input.ip")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "D:\\work\\app");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => { button("导入 1 个会话").click(); });
    expect(invoke).toHaveBeenCalledWith("sessions_transfer_import", { path: "C:\\move.zip", targets: [{ from: "E:\\app", to: "D:\\work\\app", extract: true }] });
    expect(text()).toContain("codex resume t1");
    expect(text()).toContain("已去掉只有原账号能用的加密推理和思考签名（3 处）");
  });
});
