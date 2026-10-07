// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { BrowserExport } from "./BrowserExport";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn(async () => "C:\\out\\chrome.csv") }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation((async (command: string, args?: { browser: string }) => {
    if (command === "vault_browser_exportable") return args!.browser === "firefox" ? 2 : 3;
    if (command === "vault_export_browser") return 3;
    return null;
  }) as typeof invoke);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const text = () => document.body.textContent ?? "";
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((item) => item.textContent?.includes(label))!;

describe("exporting logins for a browser", () => {
  it("counts per browser, asks for the master password and writes the chosen ones", async () => {
    const ids = ["a", "b"];
    await act(async () => { root.render(<BrowserExport ids={ids} onClose={() => {}} />); });
    expect(text()).toContain("将导出 3 条登录。");
    expect(text()).toContain("chrome://password-manager/settings");
    await act(async () => { button("Firefox").click(); });
    expect(text()).toContain("将导出 2 条登录。");
    expect(text()).toContain("about:logins");
    await act(async () => { button("Chrome").click(); });
    expect(button("选择位置并导出").disabled).toBe(true);

    const input = document.querySelector<HTMLInputElement>("input[type=password]")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "master password");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => { button("选择位置并导出").click(); });
    expect(invoke).toHaveBeenCalledWith("vault_export_browser", { password: "master password", ids, browser: "chrome", dest: "C:\\out\\chrome.csv" });
    expect(text()).toContain("已导出 3 条登录，可以导入 Chrome。");
  });
});
