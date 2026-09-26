// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { GatewayAgents, type AgentCard } from "./GatewayAgents";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const cards: AgentCard[] = [
  { id: "codex", name: "Codex CLI", installed: true, version: "0.155.1", supported: true, reason: "", enabled: true,
    login: { state: "logged_in", method: "ChatGPT" }, defaultModel: null, defaultEffort: "low", efforts: ["low", "high"],
    models: [{ call: "codex/gpt-5.6-sol", label: "GPT-5.6-Sol", efforts: ["low", "high"], defaultEffort: "low" }] },
  // Signed out: one sign-in away from working, so the page still names it.
  { id: "mimo", name: "MiMo Code CLI", installed: true, version: "0.1.15", supported: false, reason: "未登录：请在终端运行该智能体并完成登录", enabled: false,
    login: { state: "logged_out", method: "" }, defaultModel: null, defaultEffort: null, efforts: [], models: [] },
  // No API backend at all: nothing the user can do about it, so it stays off the page.
  { id: "hermes", name: "Hermes CLI", installed: true, version: "1.0", supported: false, reason: "需要自备第三方 API key", enabled: false,
    login: null, defaultModel: null, defaultEffort: null, efforts: [], models: [] },
];

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  localStorage.clear();
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => command === "gateway_agents" ? cards : null);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

describe("gateway agents", () => {
  it("shows usable agents, invites a signed-out one to log in, and hides what it cannot drive", async () => {
    await act(async () => { root.render(<GatewayAgents />); });
    expect(host.textContent).toContain("Codex CLI");
    expect(host.textContent).toContain("ChatGPT");
    // Folded: a summary line; the model table opens from the header.
    expect(host.textContent).not.toContain("codex/gpt-5.6-sol");
    expect(host.textContent).toContain("1 个可指定的模型");
    await act(async () => { host.querySelector<HTMLButtonElement>(".gw-fold")!.click(); });
    expect(host.textContent).toContain("codex/gpt-5.6-sol");
    expect(host.textContent).toContain("high");
    expect(host.textContent).toContain("MiMo Code CLI");
    expect(host.textContent).toContain("未登录：请在终端运行该智能体并完成登录");
    expect(host.textContent).not.toContain("Hermes CLI");
    expect(host.textContent).not.toContain("需要自备第三方 API key");
    const toggle = host.querySelector<HTMLInputElement>(".gw-switch input")!;
    await act(async () => { toggle.click(); });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("gateway_set_agent", { agent: "codex", enabled: false });
  });

  it("shows the last check at once, without waiting for a new one", async () => {
    localStorage.setItem("stacker.gateway.agents.v1", JSON.stringify(cards));
    vi.mocked(invoke).mockImplementation(() => new Promise(() => {}));
    await act(async () => { root.render(<GatewayAgents />); });
    expect(host.textContent).toContain("Codex CLI");
    expect(host.textContent).toContain("正在更新登录状态和模型列表");
  });
});
