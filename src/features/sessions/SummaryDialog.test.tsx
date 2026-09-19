// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { SummaryDialog } from "./SummaryDialog";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const settings = { runner: "same", codexModel: "", codexEffort: "low", claudeModel: "sonnet", claudeEffort: "low" };
const options = [
  { agent: "codex", installed: true, models: [{ id: "gpt-5.6-sol", label: "GPT-5.6-Sol", efforts: ["low", "high"], defaultEffort: "low" }], efforts: ["low", "high"] },
  { agent: "claude", installed: true, models: [{ id: "sonnet", label: "sonnet", efforts: ["low", "max"], defaultEffort: null }], efforts: ["low", "max"] },
];
const preview = {
  items: [{ id: "codex:a", title: "A session", agent: "codex", chars: 23456, needed: true, runner: { agent: "codex", model: null, effort: "low" } }],
  totalChars: 23456, projectName: "", handoffRunner: null,
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "summary_settings") return settings;
    if (command === "runner_options") return options;
    if (command === "summary_preview") return preview;
    if (command === "summary_start") return { id: "j", kind: "summary", state: "running", done: 0, total: 1, items: [{ id: "codex:a", title: "A session", status: "queued", detail: "", elapsedMs: 0, by: "" }], error: "", resultPath: "", resultText: "" };
    return null;
  });
});

afterEach(() => { act(() => root.unmount()); host.remove(); });

describe("summary dialog", () => {
  it("shows what will be sent and starts with the chosen runner settings", async () => {
    await act(async () => { root.render(<SummaryDialog target={{ kind: "summary", ids: ["codex:a"] }} onClose={() => {}} />); });
    await act(async () => { await Promise.resolve(); });
    expect(host.textContent).toContain("2.3");
    expect(host.textContent).toContain("会话正文会发送给所选智能体的模型服务");
    const start = [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("开始生成"));
    await act(async () => { start!.click(); });
    const call = vi.mocked(invoke).mock.calls.find(([c]) => c === "summary_start");
    expect(call?.[1]).toMatchObject({ ids: ["codex:a"], regenerate: false, settings: { codexEffort: "low", claudeModel: "sonnet" } });
    expect(host.textContent).toContain("A session");
  });
});
