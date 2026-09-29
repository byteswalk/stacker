// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { GatewayAgents, agentState, effortNote, sinceText, snippet, type AgentCard } from "./GatewayAgents";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const cards: AgentCard[] = [
  { id: "codex", name: "Codex CLI", vendor: "OpenAI", installed: true, version: "0.155.1", supported: true, reason: "", enabled: true, defaultIsChosen: false,
    login: { state: "logged_in", method: "ChatGPT" }, defaultModel: null, defaultEffort: "low", efforts: ["low", "high"],
    models: [{ call: "codex/gpt-5.6-sol", label: "GPT-5.6-Sol", efforts: ["low", "high"], defaultEffort: "low" }] },
  // Signed out: one sign-in away from working, so the page still names it.
  { id: "mimo", name: "MiMo Code CLI", vendor: "小米", installed: true, version: "0.1.15", supported: false, reason: "未登录：请在终端运行该智能体并完成登录", enabled: false, defaultIsChosen: false,
    login: { state: "logged_out", method: "" }, defaultModel: null, defaultEffort: null, efforts: [], models: [] },
  // No API backend at all: nothing the user can do about it, so it stays off the page.
  { id: "hermes", name: "Hermes CLI", vendor: "", installed: true, version: "1.0", supported: false, reason: "需要自备第三方 API key", enabled: false, defaultIsChosen: false,
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
    await act(async () => { root.render(<GatewayAgents base="http://127.0.0.1:8765" token="sk-stacker-test" />); });
    expect(host.textContent).toContain("Codex CLI");
    // The line under the name says who makes it and how the account signed in.
    expect(host.textContent).toContain("OpenAI · ChatGPT · 1 个模型");
    // Folded: the default is always visible, the code and models open from the header.
    expect(host.textContent).not.toContain("codex/gpt-5.6-sol");
    expect(host.textContent).toContain("请求里写");
    await act(async () => { host.querySelector<HTMLButtonElement>(".gw-fold")!.click(); });
    expect(host.textContent).toContain("codex/gpt-5.6-sol");
    // The examples are reference material: they open from a fold of their own.
    expect(host.textContent).not.toContain("sk-stacker-test");
    await act(async () => { host.querySelector<HTMLButtonElement>(".gw-use-toggle")!.click(); });
    expect(host.textContent).toContain("sk-stacker-test");
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

  it("lets a passing test answer whether the agent works", () => {
    // CodeBuddy has no way to report a sign-in, and saying "unknown" next to a test that
    // just succeeded told the user two different things.
    const unknown = { ...card, login: { state: "unknown" as const, method: "" } };
    expect(agentState(unknown).label).toBe("未测试");
    const passed = agentState(unknown, { at: Date.now() - 4 * 60_000, ok: true, elapsedMs: 4100 });
    expect(passed.label).toBe("可用 · 4 分钟前测试通过");
    expect(agentState({ ...card, login: { state: "logged_out", method: "" } }).label).toBe("未登录");
  });

  it("counts the wait in words", () => {
    const now = Date.parse("2026-09-26T12:00:00Z");
    expect(sinceText(now - 30_000, now)).toBe("刚刚");
    expect(sinceText(now - 90 * 60_000, now)).toBe("1 小时前");
    expect(sinceText(now - 50 * 60 * 60_000, now)).toBe("2 天前");
  });

  it("fills the port, key and model into code that runs as it is", () => {
    const code = snippet("openai", "http://127.0.0.1:8765", "sk-stacker-abc", "codex");
    expect(code).toContain('base_url="http://127.0.0.1:8765/v1"');
    expect(code).toContain('api_key="sk-stacker-abc"');
    expect(code).toContain('model="codex"');
    expect(snippet("client", "http://127.0.0.1:8765", "sk-stacker-abc", "qoder")).toContain("模型名称：qoder");
  });
});

describe("reasoning levels in the model list", () => {
  const all = ["low", "medium", "high"];

  it("says nothing when a model takes the same levels as the rest", () => {
    expect(effortNote(["high", "low", "medium"], all)).toBe("");
    expect(effortNote([], all)).toBe("");
  });

  it("names them only where the model differs", () => {
    expect(effortNote(["low", "medium"], all)).toBe("仅 low / medium");
    expect(effortNote(["low", "xhigh"], all)).toBe("支持 low / xhigh");
  });
});
