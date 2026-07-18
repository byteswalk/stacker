import { useSyncExternalStore } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "../../invoke";
import type { CleanupPlan, CleanupProgress, CleanupResult, DirectoryNode, SafetyClass } from "./types";

export type CleanupStoreState = {
  scanTaskId: string | null;
  candidates: DirectoryNode[];
  selected: Set<string>;
  loading: boolean;
  error: string | null;
  plan: CleanupPlan | null;
  progress: CleanupProgress | null;
  result: CleanupResult | null;
};

const initialState: CleanupStoreState = {
  scanTaskId: null,
  candidates: [],
  selected: new Set(),
  loading: false,
  error: null,
  plan: null,
  progress: null,
  result: null,
};

let state = initialState;
let unlisten: UnlistenFn | null = null;
const subscribers = new Set<() => void>();

function publish(next: CleanupStoreState) {
  state = next;
  subscribers.forEach((subscriber) => subscriber());
}

export function defaultSelectedNodeIds(nodes: readonly DirectoryNode[]) {
  return new Set(nodes.filter((node) => node.safety === "safe").map((node) => node.nodeId));
}

export function canSelectSafety(safety: string): safety is Exclude<SafetyClass, "viewOnly"> {
  return safety === "safe" || safety === "rebuildable" || safety === "needsConfirmation";
}

export function useCleanupStore() {
  return useSyncExternalStore(
    (subscriber) => {
      subscribers.add(subscriber);
      return () => subscribers.delete(subscriber);
    },
    () => state,
  );
}

export async function loadCleanupCandidates(scanTaskId: string) {
  if (state.scanTaskId === scanTaskId && (state.loading || state.candidates.length > 0)) return;
  publish({ ...initialState, scanTaskId, loading: true });
  try {
    const candidates = await invoke<DirectoryNode[]>("space_cleanup_candidates", { taskId: scanTaskId });
    publish({ ...initialState, scanTaskId, candidates, selected: defaultSelectedNodeIds(candidates) });
  } catch (error) {
    publish({ ...initialState, scanTaskId, error: String(error) });
  }
}

export function toggleCleanupNode(node: DirectoryNode) {
  if (!canSelectSafety(node.safety) || state.progress?.state === "running") return;
  const selected = new Set(state.selected);
  if (selected.has(node.nodeId)) selected.delete(node.nodeId);
  else selected.add(node.nodeId);
  publish({ ...state, selected, plan: null, result: null });
}

export function selectionWithNodes(
  current: ReadonlySet<string>,
  nodes: readonly DirectoryNode[],
  selected: boolean,
) {
  const next = new Set(current);
  nodes.forEach((node) => {
    if (!canSelectSafety(node.safety)) return;
    if (selected) next.add(node.nodeId);
    else next.delete(node.nodeId);
  });
  return next;
}

export function setCleanupNodesSelected(nodes: readonly DirectoryNode[], selected: boolean) {
  if (state.progress?.state === "running") return;
  publish({
    ...state,
    selected: selectionWithNodes(state.selected, nodes, selected),
    plan: null,
    result: null,
  });
}

export async function prepareCleanupPlan() {
  if (!state.scanTaskId || state.selected.size === 0) return null;
  const plan = await invoke<CleanupPlan>("space_cleanup_plan", {
    scanTaskId: state.scanTaskId,
    nodeIds: [...state.selected],
  });
  publish({ ...state, plan, error: null });
  return plan;
}

function terminal(value: CleanupProgress["state"]) {
  return value === "completed" || value === "cancelled" || value === "failed";
}

export async function startCleanup() {
  const plan = state.plan;
  if (!plan) throw new Error("Cleanup plan is unavailable.");
  if (!unlisten) {
    unlisten = await listen<CleanupProgress>("space-cleanup-progress", (event) => {
      if (state.plan?.planId !== event.payload.planId) return;
      publish({ ...state, progress: event.payload });
    });
  }
  const taskId = await invoke<string>("space_cleanup_start", {
    planId: plan.planId,
    nodeIds: plan.items.map((item) => item.nodeId),
  });
  let progress = await invoke<CleanupProgress>("space_cleanup_status", { taskId });
  publish({ ...state, progress });
  while (!terminal(progress.state)) {
    await new Promise((resolve) => window.setTimeout(resolve, 220));
    progress = await invoke<CleanupProgress>("space_cleanup_status", { taskId });
    publish({ ...state, progress });
  }
  const result = await invoke<CleanupResult>("space_cleanup_result", { taskId });
  publish({ ...state, progress, result });
  return result;
}

export async function cancelCleanup() {
  const taskId = state.progress?.taskId;
  if (taskId && !terminal(state.progress!.state)) {
    await invoke("space_cleanup_cancel", { taskId });
  }
}

export function dismissCleanupPlan() {
  publish({ ...state, plan: null });
}

export function dismissCleanupResult() {
  publish({ ...state, plan: null, progress: null, result: null });
}
