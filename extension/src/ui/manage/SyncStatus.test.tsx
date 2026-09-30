// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SyncStatus } from "./SyncStatus";

// Every root a test mounts is unmounted inside act afterwards, so nothing React has queued runs
// after the test environment is gone.
const mounted: Root[] = [];
const track = (root: Root) => { mounted.push(root); return root; };
afterEach(() => { while (mounted.length) { const root = mounted.pop()!; act(() => root.unmount()); } });

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function render(node: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  act(() => track(createRoot(host)).render(node));
  return host;
}

describe("SyncStatus", () => {
  it("shows pending changes when connected and offers restore", () => {
    const onRestore = vi.fn();
    const host = render(<SyncStatus status={{ connected: true, pending: 3, lastSyncAt: null, error: "", theme: null }} busy={false} onReconnect={() => {}} onRestore={onRestore} />);
    expect(host.textContent).toContain("已连接 Stacker · 待同步 3 项");
    act(() => [...host.querySelectorAll("button")].find((b) => b.textContent === "从 Stacker 恢复")!.click());
    expect(onRestore).toHaveBeenCalled();
  });

  it("offers to reconnect when Stacker is not available", () => {
    const onReconnect = vi.fn();
    const host = render(<SyncStatus status={{ connected: false, pending: 2, lastSyncAt: null, error: "host not found", theme: null }} busy={false} onReconnect={onReconnect} onRestore={() => {}} />);
    expect(host.textContent).toContain("未连接 Stacker");
    expect(host.textContent).not.toContain("从 Stacker 恢复");
    act(() => host.querySelector("button")!.click());
    expect(onReconnect).toHaveBeenCalled();
  });
});
