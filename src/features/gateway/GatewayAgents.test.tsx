// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { GatewayAgents, agentState, snippet, type AgentCard } from "./GatewayAgents";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const cards: AgentCard[] = [
  { id: "codex", name: "Codex CLI", vendor: "OpenAI", installed: true, version: "0.155.1", supported: true, reason: "", enabled: true,
    login: { state: "logged_in", method: "ChatGPT" }, efforts: ["low", "high"],
    models: [{ call: "codex/gpt-5.6-sol", label: "GPT-5.6-Sol", efforts: ["low", "high"], defaultEffort: "low" }] },
  // Signed out: one sign-in away from working, so the page still names it.
  { id: "mimo", name: "MiMo Code CLI", vendor: "小米", installed: true, version: "0.1.15", supported: false, reason: "未登录：请在终端运行该智能体并完成登录", enabled: false,
    login: { state: "logged_out", method: "" }, efforts: [], models: [] },
  // No API backend at all: nothing the user can do about it, so it stays off the page.
  { id: "hermes", name: "Hermes CLI", vendor: "", installed: true, version: "1.0", supported: false, reason: "需要自备第三方 API key", enabled: false,
    login: null, efforts: [], models: [] },
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
    await act(async () => { root.render(<GatewayAgents base="http://127.0.0.1:8765" token="sk-stacker-test" />); });
    expect(host.textContent).toContain("Codex CLI");
    // The line under the name says who makes it and how the account signed in.
    expect(host.textContent).toContain("OpenAI · ChatGPT · 1 个模型");
    // Folded until asked for; then one row per name, the bare one first, levels to pick.
    expect(host.textContent).not.toContain("codex/gpt-5.6-sol");
    expect(host.textContent).not.toContain("请求里写");
    await act(async () => { host.querySelector<HTMLButtonElement>(".gw-fold")!.click(); });
    expect(host.textContent).toContain("跟随 CLI 默认");
    expect(host.textContent).toContain("codex/gpt-5.6-sol");
    // Picking a level, then the examples: the code carries that level in OpenAI's own field.
    const level = [...host.querySelectorAll<HTMLButtonElement>(".gw-model:nth-child(2) .gw-level")].find((b) => b.textContent === "high")!;
    await act(async () => { level.click(); });
    const examples = [...host.querySelectorAll<HTMLButtonElement>(".gw-model:nth-child(2) button")].find((b) => b.textContent?.includes("调用示例"))!;
    await act(async () => { examples.click(); });
    expect(document.body.textContent).toContain('reasoning_effort="high"');
    expect(document.body.textContent).toContain("sk-stacker-test");
    expect(host.textContent).toContain("MiMo Code CLI");
    expect(host.textContent).toContain("mimo auth login");
    expect(host.textContent).not.toContain("Hermes CLI");
    const toggle = host.querySelector<HTMLInputElement>(".gw-switch input")!;
    await act(async () => { toggle.click(); });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("gateway_set_agent", { agent: "codex", enabled: false });
  });

  it("shows the last check at once, without waiting for a new one", async () => {
    localStorage.setItem("stacker.gateway.agents.v1", JSON.stringify(cards));
    vi.mocked(invoke).mockImplementation(() => new Promise(() => {}));
    await act(async () => { root.render(<GatewayAgents base="http://127.0.0.1:8765" token="sk-stacker-test" />); });
    expect(host.textContent).toContain("Codex CLI");
    expect(host.textContent).toContain("正在更新登录状态和模型列表");
  });
});

describe("what the header says", () => {
  const card = cards[0];

  it("says signed in the same way for every agent", () => {
    // CodeBuddy cannot report a sign-in; a model list or a passing test shows one all the same.
    const unknown = { ...card, login: { state: "unknown" as const, method: "" } };
    expect(agentState({ ...unknown, models: [] }).label).toBe("未测试");
    expect(agentState(unknown).label).toBe("已登录");
    expect(agentState({ ...unknown, models: [] }, { at: Date.now(), ok: true, elapsedMs: 4100 }).label).toBe("已登录");
    expect(agentState({ ...card, login: { state: "logged_out", method: "" } }).label).toBe("未登录");
  });

  it("fills the port, key and model into code that runs as it is", () => {
    const code = snippet("openai", "http://127.0.0.1:8765", "sk-stacker-abc", "codex");
    expect(code).toContain('base_url="http://127.0.0.1:8765/v1"');
    expect(code).toContain('api_key="sk-stacker-abc"');
    expect(code).toContain('model="codex"');
    expect(snippet("client", "http://127.0.0.1:8765", "sk-stacker-abc", "qoder")).toContain("模型名称：qoder");
  });
});

describe("reasoning levels in the examples", () => {
  const base = "http://127.0.0.1:8765";

  it("puts each level in its own API's field, and none when nothing is picked", () => {
    expect(snippet("openai", base, "k", "claude/opus", "xhigh")).toContain('reasoning_effort="xhigh"');
    expect(snippet("anthropic", base, "k", "claude/opus", "max")).toContain('output_config={"effort": "max"}');
    expect(snippet("anthropic", base, "k", "claude/opus", "max")).not.toContain("reasoning_effort");
    expect(snippet("curl", base, "k", "claude/opus", "low")).toContain('"reasoning_effort":"low"');
    expect(snippet("openai", base, "k", "claude")).not.toContain("reasoning_effort");
    expect(snippet("client", base, "k", "claude")).toContain("由智能体自己决定");
  });
});
