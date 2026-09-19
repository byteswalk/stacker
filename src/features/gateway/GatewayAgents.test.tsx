// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { GatewayAgents, type AgentCard } from "./GatewayAgents";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const cards: AgentCard[] = [
  { id: "codex", name: "Codex CLI", installed: true, version: "0.155.1", supported: true, reason: "", enabled: true,
    login: { state: "logged_in", method: "ChatGPT" }, defaultModel: null, defaultEffort: "low",
    models: [{ call: "codex/gpt-5.6-sol", label: "GPT-5.6-Sol", efforts: ["low", "high"], defaultEffort: "low" }] },
  { id: "kimi", name: "Kimi Code CLI", installed: true, version: "1.0", supported: false, reason: "not verified", enabled: false,
    login: null, defaultModel: null, defaultEffort: null, models: [] },
];

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => command === "gateway_agents" ? cards : null);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

describe("gateway agents", () => {
  it("shows each usable agent with login, models and efforts, and lists the rest with a reason", async () => {
    await act(async () => { root.render(<GatewayAgents />); });
    expect(host.textContent).toContain("Codex CLI");
    expect(host.textContent).toContain("ChatGPT");
    expect(host.textContent).toContain("codex/gpt-5.6-sol");
    expect(host.textContent).toContain("high");
    expect(host.textContent).toContain("Kimi Code CLI");
    expect(host.textContent).toContain("not verified");
    const toggle = host.querySelector<HTMLInputElement>(".gw-switch input")!;
    await act(async () => { toggle.click(); });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("gateway_set_agent", { agent: "codex", enabled: false });
  });
});
