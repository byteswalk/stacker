// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { FootprintPanel, defaultSelection, slices } from "./FootprintPanel";
import type { FootprintItem, FootprintReport } from "./types";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const item = (id: string, kind: FootprintItem["kind"], label: string, blocked: string | null = null): FootprintItem => ({
  id, product: "codex", owner: "shared", kind, label, explain: `${label} explain`, paths: [`C:\\x\\${id}`], bytes: 1024 * 1024, files: 1, blocked, note: null,
});

const report: FootprintReport = {
  agents: [{ product: { id: "codex", name: "Codex", icon: "codex.png", sessionsAgent: "codex" as const }, total: 5 * 1024 * 1024, reclaimable: 1024 * 1024, items: [
    item("codex-sessions:1", "sessions", "Session records"),
    item("codex-tmp:1", "reclaimable", "Temp files"),
    item("codex-cache:1", "reclaimable", "Cache", "E_APP_RUNNING"),
    item("codex-target:1", "review", "Build output"),
    item("other:1", "keep", "Other data"),
  ] }],
  total: 5 * 1024 * 1024, reclaimable: 1024 * 1024, scannedAt: 1, warnings: [],
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => command === "footprint_scan" ? report : null);
});

afterEach(() => { act(() => root.unmount()); host.remove(); });

describe("footprint panel", () => {
  it("selects only unblocked reclaimable items by default", () => {
    expect(defaultSelection(report)).toEqual(["codex-tmp:1"]);
  });

  it("splits the total into what the space is", () => {
    expect(slices(report.agents[0].items).map((s) => [s.kind, s.bytes])).toEqual([
      ["sessions", 1024 * 1024],
      ["review", 1024 * 1024],
      ["reclaimable", 2 * 1024 * 1024],
      ["keep", 1024 * 1024],
    ]);
  });

  it("asks for a tick only where the user has to judge, and cleans the safe ones in one go", async () => {
    const onShow = vi.fn();
    await act(async () => { root.render(<FootprintPanel onShowSessions={onShow} />); });
    // The page opens on tiles; one click on an agent opens its detail.
    const tile = host.querySelector<HTMLButtonElement>(".fp-tile")!;
    expect(tile.textContent).toContain("Codex");
    expect(host.querySelectorAll("input[type=checkbox]").length).toBe(0);
    await act(async () => { tile.click(); });
    // Reclaimable items are handled by the one-click button, so only the review item is ticked.
    const boxes = [...host.querySelectorAll<HTMLInputElement>("input[type=checkbox]")];
    expect(boxes.map((b) => b.getAttribute("aria-label"))).toEqual(["Build output"]);
    expect(boxes.map((b) => b.checked)).toEqual([false]);
    expect(host.textContent).toContain("一键清理");
    // Deleting stays unavailable until something is ticked.
    const remove = [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("删除所选"))!;
    expect(remove.hasAttribute("disabled")).toBe(true);
    await act(async () => { boxes[0].click(); });
    expect([...host.querySelectorAll("button")].find((b) => b.textContent?.includes("删除所选"))!.hasAttribute("disabled")).toBe(false);
    const show = [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("查看最大的会话"));
    await act(async () => { show!.click(); });
    expect(onShow).toHaveBeenCalledWith("codex");
  });
});
