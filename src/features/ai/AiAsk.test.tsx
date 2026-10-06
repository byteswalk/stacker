// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AiAskModal, saveAnswer, savedAnswer } from "./AiAsk";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn(), reportFrontendError: vi.fn() }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text));

describe("kept AI answers", () => {
  it("keeps the newest few dozen", () => {
    for (let i = 0; i < 45; i += 1) saveAnswer(`k${i}`, `a${i}`, i);
    expect(savedAnswer("k44")?.answer).toBe("a44");
    expect(savedAnswer("k0")).toBeNull();
  });

  it("shows the last answer without asking, and asks again only when told", async () => {
    saveAnswer("task:1", "kept answer", 1000);
    const run = vi.fn(async () => "fresh answer");
    await act(async () => root.render(<AiAskModal title="t" note="n" run={run} saveAs="task:1" onClose={() => undefined} />));
    expect(host.textContent).toContain("kept answer");
    expect(run).not.toHaveBeenCalled();
    await act(async () => button("重新诊断")!.click());
    expect(run).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("fresh answer");
    expect(savedAnswer("task:1")?.answer).toBe("fresh answer");
  });

  it("asks at once when nothing was kept", async () => {
    const run = vi.fn(async () => "first");
    await act(async () => root.render(<AiAskModal title="t" note="n" run={run} saveAs="task:2" onClose={() => undefined} />));
    expect(run).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("first");
  });
});
