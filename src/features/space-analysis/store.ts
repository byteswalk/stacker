import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import { invoke } from "../../invoke";
import type {
  QuickScanResult,
  ScanProgress,
  ScanRequest,
  ScanTaskState,
} from "./types";

export interface SpaceScanAdapter {
  start(request: ScanRequest, elevated: boolean): Promise<string>;
  status(taskId: string): Promise<ScanProgress>;
  cancel(taskId: string): Promise<void>;
  quickResult(taskId: string): Promise<QuickScanResult>;
  listenProgress(listener: (progress: ScanProgress) => void): Promise<UnlistenFn>;
}

export interface SpaceScanSnapshot {
  taskId: string | null;
  request: ScanRequest | null;
  pendingRequest: ScanRequest | null;
  progress: ScanProgress | null;
  result: QuickScanResult | null;
  error: string | null;
}

export interface ScanStartResult {
  taskId: string;
  request: ScanRequest;
  persistenceOwner: boolean;
}

export interface SpaceScanStore {
  getSnapshot(): SpaceScanSnapshot;
  subscribe(listener: () => void): () => void;
  startScan(request: ScanRequest, options?: ScanStartOptions): Promise<ScanStartResult>;
  startQuickScan(): Promise<ScanStartResult>;
  cancelScan(): Promise<void>;
  refreshTask(): Promise<void>;
  dispose(): void;
}

type PendingStart = {
  request: ScanRequest;
  elevated: boolean;
  promise: Promise<{ taskId: string; request: ScanRequest }>;
};

export interface ScanStartOptions {
  elevated?: boolean;
}

const INITIAL_SNAPSHOT: SpaceScanSnapshot = {
  taskId: null,
  request: null,
  pendingRequest: null,
  progress: null,
  result: null,
  error: null,
};

// Non-terminal states only move forward; terminal states are sticky.
const STATE_ORDER: Record<ScanTaskState, number> = {
  queued: 0,
  running: 1,
  cancelling: 2,
  completed: 3,
  cancelled: 3,
  failed: 3,
};

const TERMINAL_STATES = new Set<ScanTaskState>([
  "completed",
  "cancelled",
  "failed",
]);

function canAdvanceState(current: ScanTaskState, next: ScanTaskState): boolean {
  if (current === next) return true;
  if (TERMINAL_STATES.has(current)) return false;
  return STATE_ORDER[next] >= STATE_ORDER[current];
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function cloneRequest(request: ScanRequest): ScanRequest {
  return { mode: request.mode, targets: [...request.targets] };
}

function sameRequest(left: ScanRequest, right: ScanRequest): boolean {
  return left.mode === right.mode
    && left.targets.length === right.targets.length
    && left.targets.every((target, index) => target === right.targets[index]);
}

export function scanSnapshotIsActive(snapshot: SpaceScanSnapshot): boolean {
  if (snapshot.pendingRequest) return true;
  if (!snapshot.taskId) return false;
  if (!snapshot.progress) return true;
  return !TERMINAL_STATES.has(snapshot.progress.state);
}

export function createSpaceScanStore(adapter: SpaceScanAdapter): SpaceScanStore {
  let snapshot = INITIAL_SNAPSHOT;
  let disposed = false;
  let unlisten: UnlistenFn | undefined;
  let pendingStart: PendingStart | null = null;
  let resultTaskId: string | null = null;
  let resultPromise: Promise<void> | null = null;
  let progressGeneration = 0;
  let statusRequestGeneration = 0;
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  const listeners = new Set<() => void>();

  function stopPolling() {
    if (pollTimer === null) return;
    clearTimeout(pollTimer);
    pollTimer = null;
  }

  function schedulePolling() {
    if (disposed || listeners.size === 0 || !scanSnapshotIsActive(snapshot)) {
      stopPolling();
      return;
    }
    if (pollTimer !== null) return;
    pollTimer = setTimeout(() => {
      pollTimer = null;
      const taskId = snapshot.taskId;
      if (!taskId || !scanSnapshotIsActive(snapshot)) return;
      void refreshTaskById(taskId)
        .catch(() => undefined)
        .finally(schedulePolling);
    }, 500);
  }

  function publish(next: SpaceScanSnapshot) {
    if (disposed) return;
    snapshot = next;
    listeners.forEach((listener) => listener());
    schedulePolling();
  }

  function setError(cause: unknown, taskId?: string) {
    if (taskId !== undefined && snapshot.taskId !== taskId) return;
    publish({ ...snapshot, error: errorMessage(cause) });
  }

  async function fetchCompletedResult(taskId: string) {
    if (snapshot.result?.taskId === taskId) return;
    if (resultTaskId === taskId && resultPromise) {
      await resultPromise;
      return;
    }

    const pending = (async () => {
      const result = await adapter.quickResult(taskId);
      if (
        snapshot.taskId === taskId
        && snapshot.progress?.state === "completed"
      ) {
        publish({ ...snapshot, result, error: null });
      }
    })();
    resultTaskId = taskId;
    resultPromise = pending;

    try {
      await pending;
    } finally {
      if (resultPromise === pending) {
        resultTaskId = null;
        resultPromise = null;
      }
    }
  }

  async function acceptProgress(progress: ScanProgress) {
    if (progress.taskId !== snapshot.taskId) return;
    if (
      snapshot.progress
      && !canAdvanceState(snapshot.progress.state, progress.state)
    ) return;

    progressGeneration += 1;
    publish({
      ...snapshot,
      progress,
      result: progress.state === "completed" ? snapshot.result : null,
      error: progress.state === "failed" ? snapshot.error : null,
    });

    if (progress.state === "completed" && snapshot.request?.mode === "quick") {
      await fetchCompletedResult(progress.taskId);
    }
  }

  function onProgress(progress: ScanProgress) {
    void acceptProgress(progress).catch((cause: unknown) => {
      setError(cause, progress.taskId);
    });
  }

  try {
    void adapter.listenProgress(onProgress).then((stopListening) => {
      if (disposed) {
        stopListening();
      } else {
        unlisten = stopListening;
      }
    }).catch(() => {
      // A failed event channel must not become an unhandled rejection.
    });
  } catch {
    // Tauri can throw synchronously when imported outside its runtime.
  }

  async function refreshTaskById(taskId: string) {
    const requestGeneration = ++statusRequestGeneration;
    const generationAtRequest = progressGeneration;
    const isCurrentRequest = () => (
      snapshot.taskId === taskId
      && statusRequestGeneration === requestGeneration
      && progressGeneration === generationAtRequest
    );

    try {
      const progress = await adapter.status(taskId);
      if (!isCurrentRequest()) return;
      await acceptProgress(progress);
    } catch (cause) {
      if (!isCurrentRequest()) return;
      setError(cause, taskId);
      throw cause;
    }
  }

  const store: SpaceScanStore = {
    getSnapshot() {
      return snapshot;
    },

    subscribe(listener) {
      listeners.add(listener);
      const taskId = snapshot.taskId;
      if (taskId && scanSnapshotIsActive(snapshot)) {
        void refreshTaskById(taskId).catch(() => undefined);
      }
      schedulePolling();
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) stopPolling();
      };
    },

    startScan(request, options = {}) {
      const requested = cloneRequest(request);
      const elevated = options.elevated === true;
      if (pendingStart) {
        if (!sameRequest(pendingStart.request, requested) || pendingStart.elevated !== elevated) {
          return Promise.reject(new Error("A different scan request is already starting."));
        }
        return pendingStart.promise.then((accepted) => ({
          taskId: accepted.taskId,
          request: cloneRequest(accepted.request),
          persistenceOwner: false,
        }));
      }
      if (scanSnapshotIsActive(snapshot)) {
        return Promise.reject(new Error("A scan is already active."));
      }

      publish({ ...snapshot, pendingRequest: cloneRequest(requested), error: null });
      let ownedStart: PendingStart | null = null;
      const pending = (async (): Promise<{ taskId: string; request: ScanRequest }> => {
        let taskId: string;
        try {
          taskId = await adapter.start(requested, elevated);
        } catch (cause) {
          if (!ownedStart || pendingStart === ownedStart) {
            publish({ ...snapshot, pendingRequest: null, error: errorMessage(cause) });
          }
          throw cause;
        }

        const acceptedRequest = cloneRequest(requested);
        progressGeneration += 1;
        publish({
          taskId,
          request: cloneRequest(acceptedRequest),
          pendingRequest: null,
          progress: null,
          result: null,
          error: null,
        });
        try {
          await refreshTaskById(taskId);
        } catch {
          // The backend has already accepted the task; progress events or a
          // later refresh can recover from a transient initial status failure.
        }
        return { taskId, request: acceptedRequest };
      })().finally(() => {
        if (ownedStart && pendingStart === ownedStart) pendingStart = null;
      });
      ownedStart = { request: requested, elevated, promise: pending };
      pendingStart = ownedStart;
      return pending.then((accepted) => ({
        taskId: accepted.taskId,
        request: cloneRequest(accepted.request),
        persistenceOwner: true,
      }));
    },

    startQuickScan() {
      return store.startScan({ mode: "quick", targets: [] });
    },

    async cancelScan() {
      if (snapshot.pendingRequest) return;
      const taskId = snapshot.taskId;
      if (!taskId) return;

      try {
        await adapter.cancel(taskId);
      } catch (cause) {
        setError(cause, taskId);
        throw cause;
      }
      await refreshTaskById(taskId);
    },

    async refreshTask() {
      const taskId = snapshot.taskId;
      if (!taskId) return;
      await refreshTaskById(taskId);
    },

    dispose() {
      if (disposed) return;
      disposed = true;
      stopPolling();
      listeners.clear();
      unlisten?.();
      unlisten = undefined;
    },
  };

  return store;
}

const tauriAdapter: SpaceScanAdapter = {
  start: (request, elevated) => invoke<string>(
    elevated ? "space_scan_start_elevated" : "space_scan_start",
    { request },
  ),
  status: (taskId) => invoke<ScanProgress>("space_scan_status", { taskId }),
  cancel: (taskId) => invoke<void>("space_scan_cancel", { taskId }),
  quickResult: (taskId) => invoke<QuickScanResult>("space_scan_quick_result", { taskId }),
  listenProgress: (listener) => listen<ScanProgress>(
    "space-scan-progress",
    (event) => listener(event.payload),
  ),
};

export const spaceScanStore = createSpaceScanStore(tauriAdapter);

export function useSpaceScan(): SpaceScanSnapshot {
  return useSyncExternalStore(
    spaceScanStore.subscribe,
    spaceScanStore.getSnapshot,
    spaceScanStore.getSnapshot,
  );
}

export function startQuickScan(): Promise<ScanStartResult> {
  return spaceScanStore.startQuickScan();
}

export function startScan(request: ScanRequest, options?: ScanStartOptions): Promise<ScanStartResult> {
  return spaceScanStore.startScan(request, options);
}

export function cancelScan(): Promise<void> {
  return spaceScanStore.cancelScan();
}

export function refreshTask(): Promise<void> {
  return spaceScanStore.refreshTask();
}
