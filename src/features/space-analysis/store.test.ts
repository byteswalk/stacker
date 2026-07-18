import { describe, expect, it, vi } from "vitest";
import {
  createSpaceScanStore,
  type SpaceScanAdapter,
} from "./store";
import type {
  QuickScanResult,
  ScanProgress,
} from "./types";

function progress(
  taskId: string,
  state: ScanProgress["state"] = "running",
  overrides: Partial<ScanProgress> = {},
): ScanProgress {
  return {
    taskId,
    state,
    scannedFiles: 0,
    scannedDirectories: 0,
    accountedBytes: 0,
    skippedPaths: 0,
    elapsedMs: 0,
    currentPath: null,
    ...overrides,
  };
}

function quickResult(taskId: string): QuickScanResult {
  return {
    taskId,
    completed: true,
    totalBytes: 2048,
    safelyReleasableBytes: 1024,
    items: [],
    errors: {
      accessDenied: 0,
      vanished: 0,
      invalidTarget: 0,
      other: 0,
    },
  };
}

function fakeAdapter(options: {
  startId?: string;
  status?: ScanProgress;
  result?: QuickScanResult;
  listenPromise?: Promise<() => void>;
} = {}) {
  let listener: ((value: ScanProgress) => void) | undefined;
  const calls: string[] = [];
  const adapter: SpaceScanAdapter = {
    start: vi.fn(async () => {
      calls.push("start");
      return options.startId ?? "scan-1";
    }),
    status: vi.fn(async (taskId: string) => {
      calls.push("status");
      return options.status ?? progress(taskId);
    }),
    cancel: vi.fn(async () => {
      calls.push("cancel");
    }),
    quickResult: vi.fn(async (taskId: string) => {
      calls.push("quickResult");
      return options.result ?? quickResult(taskId);
    }),
    listenProgress: vi.fn((next) => {
      listener = next;
      return options.listenPromise ?? Promise.resolve(() => undefined);
    }),
  };

  return {
    adapter,
    calls,
    emit(value: ScanProgress) {
      listener?.(value);
    },
  };
}

async function flushPromises() {
  await Promise.resolve();
  await Promise.resolve();
}

describe("space scan store", () => {
  it("does not start a scan during construction", () => {
    const harness = fakeAdapter();

    createSpaceScanStore(harness.adapter);

    expect(harness.calls).toEqual([]);
  });

  it("keeps a running task when the page unsubscribes", async () => {
    const harness = fakeAdapter({ startId: "scan-1" });
    const store = createSpaceScanStore(harness.adapter);
    const unsubscribe = store.subscribe(() => undefined);
    await store.startQuickScan();

    unsubscribe();
    harness.emit(progress("scan-1", "running", { scannedFiles: 9 }));

    expect(store.getSnapshot().taskId).toBe("scan-1");
    expect(store.getSnapshot().progress?.scannedFiles).toBe(9);
  });

  it("fetches the result only after the current task completes", async () => {
    const result = quickResult("scan-1");
    const harness = fakeAdapter({ startId: "scan-1", result });
    const store = createSpaceScanStore(harness.adapter);
    await store.startQuickScan();

    harness.emit(progress("scan-1", "running", { scannedFiles: 3 }));
    expect(harness.adapter.quickResult).not.toHaveBeenCalled();

    harness.emit(progress("scan-1", "completed", { scannedFiles: 8 }));
    await flushPromises();

    expect(harness.adapter.quickResult).toHaveBeenCalledOnce();
    expect(harness.adapter.quickResult).toHaveBeenCalledWith("scan-1");
    expect(store.getSnapshot().result).toEqual(result);
  });

  it("ignores progress events for a different task", async () => {
    const harness = fakeAdapter({ startId: "scan-1" });
    const store = createSpaceScanStore(harness.adapter);
    await store.startQuickScan();

    harness.emit(progress("scan-2", "completed", { scannedFiles: 99 }));
    await flushPromises();

    expect(store.getSnapshot().taskId).toBe("scan-1");
    expect(store.getSnapshot().progress?.scannedFiles).not.toBe(99);
    expect(harness.adapter.quickResult).not.toHaveBeenCalled();
  });

  it("keeps the last progress when a task is cancelled", async () => {
    const harness = fakeAdapter({ startId: "scan-1" });
    const store = createSpaceScanStore(harness.adapter);
    await store.startQuickScan();
    harness.emit(progress("scan-1", "running", {
      scannedFiles: 14,
      accountedBytes: 4096,
    }));

    harness.emit(progress("scan-1", "cancelled", {
      scannedFiles: 14,
      accountedBytes: 4096,
    }));

    expect(store.getSnapshot().progress).toMatchObject({
      state: "cancelled",
      scannedFiles: 14,
      accountedBytes: 4096,
    });
    expect(store.getSnapshot().result).toBeNull();
  });

  it("starts one progress listener regardless of store subscribers", () => {
    const harness = fakeAdapter();
    const store = createSpaceScanStore(harness.adapter);

    const unsubscribeFirst = store.subscribe(() => undefined);
    const unsubscribeSecond = store.subscribe(() => undefined);
    unsubscribeFirst();
    unsubscribeSecond();

    expect(harness.adapter.listenProgress).toHaveBeenCalledOnce();
  });

  it("unlistens when disposal happens before listen resolves", async () => {
    let resolveListen!: (unlisten: () => void) => void;
    const unlisten = vi.fn();
    const listenPromise = new Promise<() => void>((resolve) => {
      resolveListen = resolve;
    });
    const harness = fakeAdapter({ listenPromise });
    const store = createSpaceScanStore(harness.adapter);

    store.dispose();
    resolveListen(unlisten);
    await flushPromises();

    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("refreshes and cancels the current task through the adapter", async () => {
    const refreshed = progress("scan-1", "running", { scannedDirectories: 7 });
    const harness = fakeAdapter({ startId: "scan-1", status: refreshed });
    const store = createSpaceScanStore(harness.adapter);
    await store.startQuickScan();

    await store.refreshTask();
    await store.cancelScan();

    expect(harness.adapter.status).toHaveBeenCalledWith("scan-1");
    expect(harness.adapter.cancel).toHaveBeenCalledWith("scan-1");
    expect(store.getSnapshot().progress).toEqual(refreshed);
  });
});
