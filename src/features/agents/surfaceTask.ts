import type { AgentTask } from "../agent-tasks/taskStore";

/** Inline status for a card surface while its task is queued or running. */
export function surfaceTaskText(task: AgentTask | null): string | null {
  if (!task) return null;
  if (task.state === "queued") return "排队中";
  if (task.state === "running") return task.lastLine ?? "正在执行…";
  return null;
}
