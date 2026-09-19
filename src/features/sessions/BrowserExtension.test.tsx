// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { BrowserExtension } from "./BrowserExtension";
import type { WebchatStatus, WebHostState } from "./types";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const status = (chrome: WebHostState): WebchatStatus => ({
  extensionId: "a".repeat(32), extensionDir: "C:\\Stacker\\extension", extensionFound: true,
  dataDir: "C:\\data", exportDir: "C:\\data\\exports\\web",
  browsers: [{ browser: "chrome", state: chrome, registered: "" }, { browser: "edge", state: "off", registered: "" }],
  lastHelloAt: null, lastSyncAt: 1_700_000_000_000,
  counts: { accounts: 1, conversations: 12, bodies: 3, folders: 2, excerpts: 4 },
});

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === text);

describe("browser extension settings", () => {
  it("shows the folder and counts, and asks before writing the registry", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => command === "webchat_connect" ? status("connected") : status("off"));
    await act(async () => { root.render(<BrowserExtension />); });
    expect(host.textContent).toContain("C:\\Stacker\\extension");
    expect(host.textContent).toContain("对话 12");
    expect(host.textContent).toContain("从未");
    await act(async () => { button("连接")!.click(); });
    expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).not.toContain("webchat_connect");
    expect(host.textContent).toContain("HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.stacker.webchat");
    const confirm = [...host.querySelectorAll(".modal button")].find((b) => b.textContent === "连接") as HTMLElement;
    await act(async () => { confirm.click(); });
    expect(invoke).toHaveBeenCalledWith("webchat_connect", { browser: "chrome" });
    expect(host.querySelector(".modal")).toBeNull();
    expect(host.textContent).toContain("已连接");
  });

  it("disconnects without asking", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => command === "webchat_disconnect" ? status("off") : status("connected"));
    await act(async () => { root.render(<BrowserExtension />); });
    await act(async () => { button("断开")!.click(); });
    expect(invoke).toHaveBeenCalledWith("webchat_disconnect", { browser: "chrome" });
  });

  it("explains a stale registration", async () => {
    vi.mocked(invoke).mockResolvedValue(status("stale"));
    await act(async () => { root.render(<BrowserExtension />); });
    expect(host.textContent).toContain("登记指向其他位置，请重新连接");
  });
});
