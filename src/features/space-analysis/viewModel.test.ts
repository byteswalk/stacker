import { describe, expect, it } from "vitest";
import type { SpaceScanSnapshot } from "./store";
import type { ScanProgress, ScanTaskState } from "./types";
import { quickScanView } from "./viewModel";

function progress(state: ScanTaskState): ScanProgress {
  return {
    taskId: "scan-1",
    state,
    scannedFiles: 42,
    scannedDirectories: 7,
    accountedBytes: 8192,
    skippedPaths: 2,
    elapsedMs: 1350,
    currentPath: "C:\\Users\\developer\\AppData\\Local\\Temp\\long-path",
  };
}

function snapshot(
  state?: ScanTaskState,
  overrides: Partial<SpaceScanSnapshot> = {},
): SpaceScanSnapshot {
  return {
    taskId: state ? "scan-1" : null,
    request: null,
    progress: state ? progress(state) : null,
    result: null,
    error: null,
    ...overrides,
  };
}

describe("quickScanView", () => {
  it("keeps the idle page manual and ready to start", () => {
    expect(quickScanView(snapshot())).toMatchObject({
      phase: "idle",
      primaryLabel: "开始扫描",
      autoStart: false,
      canStart: true,
      canCancel: false,
      showProgress: false,
      snapshotComparable: false,
    });
  });

  it("shows live progress and cancellation while running", () => {
    expect(quickScanView(snapshot("running"))).toMatchObject({
      phase: "running",
      primaryLabel: "取消扫描",
      canStart: false,
      canCancel: true,
      showProgress: true,
      snapshotComparable: false,
    });
  });

  it("keeps progress visible while cancellation converges", () => {
    expect(quickScanView(snapshot("cancelling"))).toMatchObject({
      phase: "cancelling",
      primaryLabel: "正在取消…",
      canStart: false,
      canCancel: false,
      showProgress: true,
      snapshotComparable: false,
    });
  });

  it("exposes complete results for snapshot comparison", () => {
    expect(quickScanView(snapshot("completed", {
      result: {
        taskId: "scan-1",
        completed: true,
        totalBytes: 8192,
        safelyReleasableBytes: 4096,
        items: [],
        errors: { accessDenied: 0, vanished: 0, invalidTarget: 0, other: 0 },
      },
    }))).toMatchObject({
      phase: "completed",
      primaryLabel: "重新扫描",
      canStart: true,
      canCancel: false,
      showProgress: true,
      snapshotComparable: true,
    });
  });

  it("preserves incomplete progress after cancellation", () => {
    expect(quickScanView(snapshot("cancelled"))).toMatchObject({
      phase: "cancelled",
      title: "扫描已取消，结果不完整",
      primaryLabel: "重新扫描",
      canStart: true,
      canCancel: false,
      showProgress: true,
      snapshotComparable: false,
    });
  });

  it("keeps the page retryable after failure", () => {
    expect(quickScanView(snapshot("failed", { error: "access denied" }))).toMatchObject({
      phase: "failed",
      title: "扫描失败",
      primaryLabel: "重试",
      canStart: true,
      canCancel: false,
      showProgress: true,
      snapshotComparable: false,
      errorSummary: "扫描任务未完成，详细错误已记录。",
    });
  });
});
