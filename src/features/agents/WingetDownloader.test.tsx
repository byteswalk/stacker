// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { WingetDownloader } from "./WingetDownloader";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

let host: HTMLDivElement;
let root: Root;
let value = "default";
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  value = "default";
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation((async (command: string, args?: Record<string, unknown>) => {
    if (command === "winget_downloader_set") value = String(args?.value);
    return { value, path: "C:\\settings.json", available: true, backedUp: command === "winget_downloader_set" };
  }) as typeof invoke);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const click = async (element: Element | null | undefined) => { await act(async () => { (element as HTMLElement).click(); }); };

describe("WinGet downloads", () => {
  it("leaves WinGet as it is by default and switches to downloading by itself when picked", async () => {
    await act(async () => { root.render(<WingetDownloader />); });
    expect(host.textContent).toContain("跟随系统（传递优化）");
    expect(host.textContent).toContain("这是 WinGet 的默认做法");

    await click(host.querySelector("button"));
    await click([...host.querySelectorAll("[role=option]")].find((option) => option.textContent === "WinGet 自己下载"));
    expect(invoke).toHaveBeenCalledWith("winget_downloader_set", { value: "wininet" });
    expect(host.textContent).toContain("走系统代理，有进度");
  });

  it("is greyed out without WinGet", async () => {
    vi.mocked(invoke).mockResolvedValue({ value: "default", path: "", available: false, backedUp: false });
    await act(async () => { root.render(<WingetDownloader />); });
    expect(host.textContent).toContain("本机没有找到 WinGet。");
    expect(host.querySelector("button")?.disabled).toBe(true);
  });
});
