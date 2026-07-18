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
  start(request: ScanRequest): Promise<string>;
  status(taskId: string): Promise<ScanProgress>;
  cancel(taskId: string): Promise<void>;
  quickResult(taskId: string): Promise<QuickScanResult>;
  listenProgress(listener: (progress: ScanProgress) => void): Promise<UnlistenFn>;
}

export interface SpaceScanSnapshot {
  taskId: string | null;
  progress: ScanProgress | null;
  result: QuickScanResult | null;
  error: string | null;
}

export interface SpaceScanStore {
  getSnapshot(): SpaceScanSnapshot;
  subscribe(listener: () => void): () => void;
  startQuickScan(): Promise<string>;
  cancelScan(): Promise<void>;
  refreshTask(): Promise<void>;
  dispose(): void;
}

const INITIAL_SNAPSHOT: SpaceScanSnapshot = {
  taskId: null,
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

export function createSpaceScanStore(adapter: SpaceScanAdapter): SpaceScanStore {
  let snapshot = INITIAL_SNAPSHOT;
  let disposed = false;
  let unlisten: UnlistenFn | undefined;
  let startPromise: Promise<string> | null = null;
  let resultTaskId: string | null = null;
  let resultPromise: Promise<void> | null = null;
  let progressGeneration = 0;
  let statusRequestGeneration = 0;
  const listeners = new Set<() => void>();

  function publish(next: SpaceScanSnapshot) {
    if (disposed) return;
    snapshot = next;
    listeners.forEach((listener) => listener());
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

    if (progress.state === "completed") {
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
      return () => {
        listeners.delete(listener);
      };
    },

    startQuickScan() {
      if (startPromise) return startPromise;

      const pending = (async () => {
        let taskId: string;
        try {
          taskId = await adapter.start({ mode: "quick", targets: [] });
        } catch (cause) {
          setError(cause);
          throw cause;
        }

        if (snapshot.taskId !== taskId) {
          progressGeneration += 1;
          publish({ taskId, progress: null, result: null, error: null });
        }
        await refreshTaskById(taskId);
        return taskId;
      })().finally(() => {
        startPromise = null;
      });
      startPromise = pending;
      return pending;
    },

    async cancelScan() {
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
      listeners.clear();
      unlisten?.();
      unlisten = undefined;
    },
  };

  return store;
}

const tauriAdapter: SpaceScanAdapter = {
  start: (request) => invoke<string>("space_scan_start", { request }),
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

export function startQuickScan(): Promise<string> {
  return spaceScanStore.startQuickScan();
}

export function cancelScan(): Promise<void> {
  return spaceScanStore.cancelScan();
}

export function refreshTask(): Promise<void> {
  return spaceScanStore.refreshTask();
}
