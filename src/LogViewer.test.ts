import { describe, expect, it, vi } from "vitest";

vi.mock("./invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn(), reportFrontendError: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }));

const { recentProblems } = await import("./LogViewer");

describe("recentProblems", () => {
  it("keeps only errors and warnings, newest last, up to the limit", () => {
    const text = [
      "[2026-10-30][10:00:00][INFO][stacker] started",
      "[2026-10-30][10:00:01][WARN][stacker::window] slow",
      "plain line mentioning ERROR",
      "[2026-10-30][10:00:02][ERROR][stacker::update] failed",
    ].join("\n");
    expect(recentProblems(text)).toEqual([
      "[2026-10-30][10:00:01][WARN][stacker::window] slow",
      "[2026-10-30][10:00:02][ERROR][stacker::update] failed",
    ]);
    expect(recentProblems(text, 1)).toEqual(["[2026-10-30][10:00:02][ERROR][stacker::update] failed"]);
    expect(recentProblems("")).toEqual([]);
  });
});
