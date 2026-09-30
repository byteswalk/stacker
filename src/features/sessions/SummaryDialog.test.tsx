// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { SummaryDialog } from "./SummaryDialog";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const preview = {
  items: [{ id: "codex:a", title: "A session", agent: "codex", chars: 23456, needed: true, runner: { backend: "codex", model: null, effort: null } }],
  totalChars: 23456, projectName: "", handoffRunner: null,
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "summary_preview") return preview;
    if (command === "summary_start") return { id: "j", kind: "summary", state: "running", done: 0, total: 1, items: [{ id: "codex:a", title: "A session", status: "queued", detail: "", elapsedMs: 0, by: "" }], error: "", resultPath: "", resultText: "" };
    return null;
  });
});

afterEach(() => { act(() => root.unmount()); host.remove(); });

describe("summary dialog", () => {
  it("shows what will be sent, names the AI source, and starts without settings of its own", async () => {
    await act(async () => { root.render(<SummaryDialog target={{ kind: "summary", ids: ["codex:a"] }} onClose={() => {}} />); });
    await act(async () => { await Promise.resolve(); });
    expect(host.textContent).toContain("2.3");
    expect(host.textContent).toContain("本机 codex");
    expect(host.textContent).toContain("会话正文会发送给上面这个 AI");
    const start = [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("开始生成"));
    await act(async () => { start!.click(); });
    const call = vi.mocked(invoke).mock.calls.find(([c]) => c === "summary_start");
    expect(call?.[1]).toEqual({ ids: ["codex:a"], regenerate: false, locale: expect.any(String) });
    expect(host.textContent).toContain("A session");
  });
});
