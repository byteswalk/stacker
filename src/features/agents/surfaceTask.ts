import type { AgentTask } from "../agent-tasks/taskStore";

/** Inline status for a card surface while its task is queued or running. */
export function surfaceTaskText(task: AgentTask | null): string | null {
  if (!task) return null;
  // Say what it waits for, so a queue that looks stuck reads as the deliberate one-at-a-time it is.
  if (task.state === "queued") return task.waiting ? `排队中 · ${task.waiting}` : "排队中";
  if (task.state === "running") return task.lastLine ?? "正在执行…";
  return null;
}
