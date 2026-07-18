import { describe, expect, it, vi } from "vitest";
import {
  createSpaceScanStore,
  scanSnapshotIsActive,
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
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

  it("polls the backend so a missed terminal event cannot leave controls locked", async () => {
    vi.useFakeTimers();
    try {
      let backendProgress = progress("scan-deep", "running", { scannedFiles: 4 });
      const harness = fakeAdapter({ startId: "scan-deep" });
      vi.mocked(harness.adapter.status).mockImplementation(async () => backendProgress);
      const store = createSpaceScanStore(harness.adapter);
      const unsubscribe = store.subscribe(() => undefined);

      await store.startScan({ mode: "directories", targets: ["C:\\work"] });
      backendProgress = progress("scan-deep", "completed", { scannedFiles: 12 });
      await vi.advanceTimersByTimeAsync(500);

      expect(store.getSnapshot().progress).toMatchObject({
        state: "completed",
        scannedFiles: 12,
      });
      expect(scanSnapshotIsActive(store.getSnapshot())).toBe(false);
      unsubscribe();
      store.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it("starts and retains an explicit manual scan request", async () => {
    const harness = fakeAdapter({ startId: "scan-deep" });
    const store = createSpaceScanStore(harness.adapter);
    const request = { mode: "directories" as const, targets: ["C:\\work"] };

    await store.startScan(request);

    expect(harness.adapter.start).toHaveBeenCalledWith(request, false);
    expect(store.getSnapshot()).toMatchObject({
      taskId: "scan-deep",
      request,
      pendingRequest: null,
    });
  });

  it("dispatches an elevated deep scan without changing the stored request", async () => {
    const harness = fakeAdapter({ startId: "scan-elevated" });
    const store = createSpaceScanStore(harness.adapter);
    const request = { mode: "directories" as const, targets: ["C:\\protected"] };

    await store.startScan(request, { elevated: true });

    expect(harness.adapter.start).toHaveBeenCalledWith(request, true);
    expect(store.getSnapshot()).toMatchObject({ taskId: "scan-elevated", request });
  });

  it("owns a pending request across remounts and assigns one persistence owner", async () => {
    const accepted = deferred<string>();
    const harness = fakeAdapter();
    vi.mocked(harness.adapter.start).mockReturnValueOnce(accepted.promise);
    const store = createSpaceScanStore(harness.adapter);
    const original = { mode: "directories" as const, targets: ["C:\\work"] };
    const expected = { mode: "directories" as const, targets: ["C:\\work"] };
    const unsubscribe = store.subscribe(() => undefined);

    const first = store.startScan(original);
    original.targets[0] = "C:\\mutated-after-dispatch";
    unsubscribe();
    const remountedUnsubscribe = store.subscribe(() => undefined);
    const coalesced = store.startScan({ mode: "directories", targets: ["C:\\work"] });
    const different = store.startScan({ mode: "drives", targets: ["D:\\"] });

    expect(store.getSnapshot()).toMatchObject({
      pendingRequest: expected,
      taskId: null,
    });
    expect(scanSnapshotIsActive(store.getSnapshot())).toBe(true);
    await expect(different).rejects.toThrow("different scan request");
    expect(harness.adapter.start).toHaveBeenCalledOnce();

    await store.cancelScan();
    expect(harness.adapter.cancel).not.toHaveBeenCalled();

    accepted.resolve("scan-owned");
    await expect(first).resolves.toEqual({
      taskId: "scan-owned",
      request: expected,
      persistenceOwner: true,
    });
    await expect(coalesced).resolves.toEqual({
      taskId: "scan-owned",
      request: expected,
      persistenceOwner: false,
    });
    expect(store.getSnapshot()).toMatchObject({
      pendingRequest: null,
      taskId: "scan-owned",
      request: expected,
    });
    remountedUnsubscribe();
  });

  it("does not request a quick result when a deep scan completes", async () => {
    const harness = fakeAdapter({ startId: "scan-deep" });
    const store = createSpaceScanStore(harness.adapter);
    await store.startScan({ mode: "drives", targets: ["D:\\"] });

    harness.emit(progress("scan-deep", "completed"));
    await flushPromises();

    expect(harness.adapter.quickResult).not.toHaveBeenCalled();
    expect(store.getSnapshot().progress?.state).toBe("completed");
  });

  it("keeps an accepted scan when the initial status read fails", async () => {
    const harness = fakeAdapter({ startId: "scan-accepted" });
    vi.mocked(harness.adapter.status).mockRejectedValueOnce(new Error("status unavailable"));
    const store = createSpaceScanStore(harness.adapter);

    await expect(store.startScan({ mode: "directories", targets: ["C:\\work"] }))
      .resolves.toMatchObject({
        taskId: "scan-accepted",
        request: { mode: "directories", targets: ["C:\\work"] },
        persistenceOwner: true,
      });
    expect(store.getSnapshot()).toMatchObject({
      taskId: "scan-accepted",
      error: "status unavailable",
    });
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

  it("ignores an old status response that arrives after completion", async () => {
    const harness = fakeAdapter({ startId: "scan-1" });
    const store = createSpaceScanStore(harness.adapter);
    await store.startQuickScan();
    const oldStatus = deferred<ScanProgress>();
    vi.mocked(harness.adapter.status).mockReturnValueOnce(oldStatus.promise);

    const refresh = store.refreshTask();
    harness.emit(progress("scan-1", "completed", { scannedFiles: 12 }));
    await flushPromises();
    oldStatus.resolve(progress("scan-1", "running", { scannedFiles: 2 }));
    await refresh;

    expect(store.getSnapshot().progress).toMatchObject({
      state: "completed",
      scannedFiles: 12,
    });
    expect(store.getSnapshot().result).toEqual(quickResult("scan-1"));
  });

  it("ignores an old status response after newer running progress", async () => {
    const harness = fakeAdapter({ startId: "scan-1" });
    const store = createSpaceScanStore(harness.adapter);
    await store.startQuickScan();
    const oldStatus = deferred<ScanProgress>();
    vi.mocked(harness.adapter.status).mockReturnValueOnce(oldStatus.promise);

    const refresh = store.refreshTask();
    harness.emit(progress("scan-1", "running", { scannedFiles: 10 }));
    oldStatus.resolve(progress("scan-1", "running", { scannedFiles: 3 }));
    await refresh;

    expect(store.getSnapshot().progress).toMatchObject({
      state: "running",
      scannedFiles: 10,
    });
  });

  it("does not regress a completed task or clear its result", async () => {
    const result = quickResult("scan-1");
    const harness = fakeAdapter({ startId: "scan-1", result });
    const store = createSpaceScanStore(harness.adapter);
    await store.startQuickScan();
    harness.emit(progress("scan-1", "completed", { scannedFiles: 15 }));
    await flushPromises();

    harness.emit(progress("scan-1", "running", { scannedFiles: 4 }));
    await flushPromises();

    expect(store.getSnapshot().progress).toMatchObject({
      state: "completed",
      scannedFiles: 15,
    });
    expect(store.getSnapshot().result).toEqual(result);
  });

  it("drops a completed result that arrives after a new task starts", async () => {
    const oldResult = deferred<QuickScanResult>();
    const harness = fakeAdapter({ startId: "scan-1" });
    vi.mocked(harness.adapter.quickResult).mockReturnValueOnce(oldResult.promise);
    const store = createSpaceScanStore(harness.adapter);
    await store.startQuickScan();
    harness.emit(progress("scan-1", "completed"));
    await flushPromises();

    vi.mocked(harness.adapter.start).mockResolvedValueOnce("scan-2");
    await store.startQuickScan();
    oldResult.resolve(quickResult("scan-1"));
    await flushPromises();

    expect(store.getSnapshot()).toMatchObject({
      taskId: "scan-2",
      progress: { taskId: "scan-2", state: "running" },
      result: null,
    });
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

  it("does not regress a cancelled task when old running progress arrives", async () => {
    const harness = fakeAdapter({ startId: "scan-1" });
    const store = createSpaceScanStore(harness.adapter);
    await store.startQuickScan();
    harness.emit(progress("scan-1", "cancelled", {
      scannedFiles: 18,
      accountedBytes: 8192,
    }));

    harness.emit(progress("scan-1", "running", {
      scannedFiles: 5,
      accountedBytes: 1024,
    }));

    expect(store.getSnapshot().progress).toMatchObject({
      state: "cancelled",
      scannedFiles: 18,
      accountedBytes: 8192,
    });
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
