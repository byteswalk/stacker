import { describe, expect, it } from "vitest";
import { parseProgress } from "./progress";

describe("task progress from log lines", () => {
  it("reads Stacker's own downloader", () => {
    expect(parseProgress("正在下载 45% · 12.3/27.0 MB · 已 5s")).toEqual({ ratio: 0.45, percent: 45 });
    expect(parseProgress("正在下载 100% · 27.0/27.0 MB · 已 9s")).toEqual({ ratio: 1, percent: 100 });
    // A Store install through WinGet shows a percent with no sizes.
    expect(parseProgress("正在处理 45% · 已 12s")).toEqual({ ratio: 0.45, percent: 45 });
    // The heartbeat has no number worth a bar.
    expect(parseProgress("WinGet 正在处理 · 已 12 秒")).toBeNull();
    // Size unknown: nothing measurable, so no made-up number.
    expect(parseProgress("正在下载 12.3 MB · 已 5s")).toBeNull();
  });

  it("reads a tool that prints done / total, in any unit", () => {
    expect(parseProgress("24.0 MB / 48.0 MB")).toEqual({ ratio: 0.5, percent: 50 });
    expect(parseProgress("512 KB / 2.00 MB")).toEqual({ ratio: 0.25, percent: 25 });
  });

  it("has nothing to say about npm, or about WinGet running with captured output", () => {
    expect(parseProgress("added 3 packages, changed 12 packages in 31s")).toBeNull();
    // What WinGet 1.29 actually prints when its output is piped: no progress frames at all.
    expect(parseProgress("Found 7-Zip [7zip.7zip] Version 26.03")).toBeNull();
    expect(parseProgress("Successfully verified installer hash")).toBeNull();
    expect(parseProgress("Improved startup time by 30%")).toBeNull();
    expect(parseProgress(null)).toBeNull();
  });
});
