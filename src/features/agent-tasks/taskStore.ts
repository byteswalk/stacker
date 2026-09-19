import { listen } from "@tauri-apps/api/event";
import { invoke } from "../../invoke";

export type AgentTaskState = "queued" | "running" | "succeeded" | "failed" | "cancelled";
export type AgentTaskAction = "install" | "update" | "uninstall" | "repair";
export type AgentTask = {
  id: string;
  productId: string;
  productName: string;
  surface: "cli" | "desktop";
  surfaceLabel: string;
  cliId: string | null;
  action: AgentTaskAction;
  state: AgentTaskState;
  message: string | null;
  lastLine: string | null;
  createdAt: string;
  startedAt: string | null;
  finishedAt: string | null;
};
export type TaskMap = Record<string, AgentTask>;

export function isOpenTask(task: AgentTask) {
  return task.state === "queued" || task.state === "running";
}

/** Merges a task snapshot; `finished` is set only on the transition into a final state. */
export function applyTaskEvent(tasks: TaskMap, next: AgentTask): { tasks: TaskMap; finished: AgentTask | null } {
  const previous = tasks[next.id];
  const finished = !isOpenTask(next) && (!previous || isOpenTask(previous)) ? next : null;
  return { tasks: { ...tasks, [next.id]: next }, finished };
}

/** The open task for a card surface. A shared CLI is matched by its CLI id from any card. */
export function openTaskFor(tasks: TaskMap, productId: string, cliId: string | null | undefined, surface: "cli" | "desktop") {
  return Object.values(tasks).find((task) => isOpenTask(task) && task.surface === surface
    && (surface === "cli" && cliId ? task.cliId === cliId : task.productId === productId)) ?? null;
}

let taskMap: TaskMap = {};
const listeners = new Set<(tasks: TaskMap) => void>();

function publish(next: TaskMap) {
  taskMap = next;
  listeners.forEach((fn) => fn(taskMap));
}

export function taskSnapshot() {
  return taskMap;
}

export function subscribeTasks(fn: (tasks: TaskMap) => void) {
  listeners.add(fn);
  return () => { listeners.delete(fn); };
}

/** Subscribes to backend task events and loads tasks that already exist. */
export async function initAgentTasks(onFinished: (task: AgentTask) => void) {
  const unlisten = await listen<AgentTask>("agent-task", (event) => {
    const result = applyTaskEvent(taskMap, event.payload);
    publish(result.tasks);
    if (result.finished) onFinished(result.finished);
  });
  const existing = await invoke<AgentTask[]>("agent_tasks");
  publish({ ...Object.fromEntries(existing.map((task) => [task.id, task])), ...taskMap });
  return unlisten;
}

export function startAgentTask(productId: string, surface: "cli" | "desktop", action: AgentTaskAction) {
  return invoke<AgentTask>("agent_task_start", { request: { productId, surface, action } });
}

export function cancelAgentTask(id: string) {
  return invoke<void>("agent_task_cancel", { id });
}

export function retryAgentTask(id: string) {
  return invoke<AgentTask>("agent_task_retry", { id });
}

export function agentTaskLog(id: string) {
  return invoke<string[]>("agent_task_log", { id });
}
