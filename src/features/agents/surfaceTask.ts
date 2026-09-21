import { parseProgress } from "../agent-tasks/progress";
import type { AgentTask } from "../agent-tasks/taskStore";

/** Inline status for a card surface while its task is queued or running. */
export function surfaceTaskText(task: AgentTask | null): string | null {
  if (!task) return null;
  // Say what it waits for, so a queue that looks stuck reads as the deliberate one-at-a-time it is.
  if (task.state === "queued") return task.waiting ? `排队中 · ${task.waiting}` : "排队中";
  if (task.state === "running") {
    const line = task.lastLine?.trim();
    if (!line) return "正在执行…";
    const progress = parseProgress(line);
    // Stacker's own downloader already says its percentage; a bare "done / total" gets one in front.
    return progress && !line.includes(`${progress.percent}%`) ? `${progress.percent}% · ${line}` : line;
  }
  return null;
}
