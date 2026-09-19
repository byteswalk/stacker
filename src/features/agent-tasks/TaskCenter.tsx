import { useEffect, useMemo, useState } from "react";
import { Modal, useToast } from "../../ui";
import { ACTION_TEXT } from "./useTaskToasts";
import { agentTaskLog, cancelAgentTask, isOpenTask, retryAgentTask, subscribeTasks, taskSnapshot, type AgentTask } from "./taskStore";

const STATE_TEXT: Record<AgentTask["state"], string> = {
  queued: "排队中", running: "进行中", succeeded: "已完成", failed: "失败", cancelled: "已取消",
};

/** Header button with the running count; opens the task list with cancel, retry and logs. */
export function TaskCenter() {
  const toast = useToast();
  const [tasks, setTasks] = useState(taskSnapshot());
  const [open, setOpen] = useState(false);
  const [log, setLog] = useState<{ task: AgentTask; lines: string[] } | null>(null);
  useEffect(() => subscribeTasks(setTasks), []);
  const list = useMemo(() => Object.values(tasks).sort((a, b) => b.createdAt.localeCompare(a.createdAt)), [tasks]);
  const running = list.filter(isOpenTask).length;
  if (list.length === 0) return null;

  async function showLog(task: AgentTask) {
    try {
      setLog({ task, lines: await agentTaskLog(task.id) });
    } catch (error) {
      toast(`读取任务日志失败：${error}`, "err");
    }
  }

  return (
    <div className="task-center">
      <button className={"gh sm task-center-btn" + (running ? " busy" : "")} aria-expanded={open} onClick={() => setOpen(!open)}>
        <i className={"ti " + (running ? "ti-loader spin" : "ti-list-check")} />
        任务
        {running > 0 && <span className="task-count">{running}</span>}
      </button>
      {open && (
        <div className="task-panel" role="dialog" aria-label="任务">
          {list.map((task) => (
            <div className={"task-row " + task.state} key={task.id}>
              <div className="task-main">
                <div className="task-title">
                  <span>{task.surfaceLabel} · {ACTION_TEXT[task.action]}</span>
                  <span className="task-state">{STATE_TEXT[task.state]}</span>
                </div>
                <div className="task-line mono" title={task.message ?? task.lastLine ?? ""}>{task.message ?? task.lastLine ?? ""}</div>
              </div>
              <div className="task-actions">
                {isOpenTask(task) && <button className="gh sm" onClick={() => void cancelAgentTask(task.id).catch((error) => toast(String(error), "err"))}>取消</button>}
                {(task.state === "failed" || task.state === "cancelled") && (
                  <button className="gh sm" onClick={() => void retryAgentTask(task.id).catch((error) => toast(String(error), "err"))}>重试</button>
                )}
                <button className="gh sm" onClick={() => void showLog(task)}>日志</button>
              </div>
            </div>
          ))}
        </div>
      )}
      {log && (
        <Modal title={`${log.task.surfaceLabel} · 任务日志`} icon="ti-file-text" wide onClose={() => setLog(null)}>
          <pre className="task-log mono">{log.lines.length ? log.lines.join("\n") : "暂无日志"}</pre>
        </Modal>
      )}
    </div>
  );
}
