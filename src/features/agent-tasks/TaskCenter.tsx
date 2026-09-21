import { useEffect, useMemo, useRef, useState } from "react";
import { Modal, useToast } from "../../ui";
import { formatAge } from "../sessions/sessionsView";
import { ACTION_TEXT } from "./useTaskToasts";
import {
  agentTaskLog, cancelAgentTask, clearFinishedTasks, dismissTask, isOpenTask, retryAgentTask, subscribeTasks,
  taskSnapshot, type AgentTask,
} from "./taskStore";

const STATE_TEXT: Record<AgentTask["state"], string> = {
  queued: "排队中", running: "进行中", succeeded: "已完成", failed: "失败", cancelled: "已取消",
};

const STATE_ICON: Record<AgentTask["state"], string> = {
  queued: "ti-clock-hour-4", running: "ti-loader-2 spin", succeeded: "ti-circle-check", failed: "ti-alert-circle", cancelled: "ti-circle-minus",
};

const secondsOf = (iso: string | null) => (iso ? Date.parse(iso) / 1000 : null);

/** "42 秒" under a minute, then whole minutes, then whole hours. */
export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} 秒`;
  if (s < 3600) return `${Math.floor(s / 60)} 分钟`;
  return `${Math.floor(s / 3600)} 小时`;
}

/** Running first, then queued in the order they will start; finished tasks newest first. */
export function splitTasks(tasks: AgentTask[]) {
  const open = tasks
    .filter(isOpenTask)
    .sort((a, b) => (a.state === b.state ? a.createdAt.localeCompare(b.createdAt) : a.state === "running" ? -1 : 1));
  const done = tasks
    .filter((task) => !isOpenTask(task))
    .sort((a, b) => (b.finishedAt ?? b.createdAt).localeCompare(a.finishedAt ?? a.createdAt));
  return { open, done };
}

function useNow(active: boolean) {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    if (!active) return;
    const timer = setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}

/** Header button with the running count; opens the task list with cancel, retry, logs and clearing. */
export function TaskCenter() {
  const toast = useToast();
  const [tasks, setTasks] = useState(taskSnapshot());
  const [open, setOpen] = useState(false);
  const [log, setLog] = useState<{ task: AgentTask; lines: string[] } | null>(null);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => subscribeTasks(setTasks), []);
  const { open: active, done } = useMemo(() => splitTasks(Object.values(tasks)), [tasks]);
  const now = useNow(open);

  // Close on a click outside the panel or on Escape, like any other popover.
  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      if (root.current && !root.current.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  if (active.length === 0 && done.length === 0) return null;

  const fail = (error: unknown) => toast(String(error), "err");

  async function showLog(task: AgentTask) {
    try {
      setLog({ task, lines: await agentTaskLog(task.id) });
    } catch (error) {
      toast(`读取任务日志失败：${error}`, "err");
    }
  }

  const title = (task: AgentTask) => `${task.surfaceLabel} · ${ACTION_TEXT[task.action]}`;

  function openRow(task: AgentTask) {
    const started = secondsOf(task.startedAt) ?? secondsOf(task.createdAt) ?? now;
    const detail = task.state === "queued" ? task.waiting ?? "排队中" : task.lastLine ?? "";
    return (
      <div className={"task-row " + task.state} key={task.id}>
        <i className={"ti task-icon " + STATE_ICON[task.state]} />
        <div className="task-main">
          <div className="task-title">
            <span className="task-name">{title(task)}</span>
            <span className="task-time">{formatDuration(now - started)}</span>
          </div>
          <div className="task-line mono" title={detail}>{detail}</div>
          {task.state === "running" && <div className="task-progress"><span /></div>}
        </div>
        <div className="task-actions">
          <button className="task-icon-btn" title="日志" aria-label="日志" onClick={() => void showLog(task)}>
            <i className="ti ti-file-text" />
          </button>
          <button className="gh sm" onClick={() => void cancelAgentTask(task.id).catch(fail)}>取消</button>
        </div>
      </div>
    );
  }

  function doneRow(task: AgentTask) {
    const started = secondsOf(task.startedAt);
    const finished = secondsOf(task.finishedAt);
    return (
      <div className={"task-row " + task.state} key={task.id}>
        <i className={"ti task-icon " + STATE_ICON[task.state]} />
        <div className="task-main">
          <div className="task-title">
            <span className="task-name">{title(task)}</span>
            <span className="task-state">{STATE_TEXT[task.state]}</span>
            {finished !== null && <span className="task-time">{formatAge(finished, now)}</span>}
          </div>
          <div className="task-line" title={task.message ?? ""}>
            {task.message ?? ""}
            {started !== null && finished !== null && <span className="task-took"> · 用时 {formatDuration(finished - started)}</span>}
          </div>
        </div>
        <div className="task-actions">
          {(task.state === "failed" || task.state === "cancelled") && (
            <button className="gh sm" onClick={() => void retryAgentTask(task.id).catch(fail)}>重试</button>
          )}
          <button className="task-icon-btn" title="日志" aria-label="日志" onClick={() => void showLog(task)}>
            <i className="ti ti-file-text" />
          </button>
          <button className="task-icon-btn task-dismiss" title="移除这条记录" aria-label="移除这条记录"
            onClick={() => void dismissTask(task.id).catch(fail)}>
            <i className="ti ti-x" />
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="task-center" ref={root}>
      <button className={"gh sm task-center-btn" + (active.length ? " busy" : "")} aria-expanded={open} onClick={() => setOpen(!open)}>
        <i className={"ti " + (active.length ? "ti-loader-2 spin" : "ti-list-check")} />
        任务
        {active.length > 0 && <span className="task-count">{active.length}</span>}
      </button>
      {open && (
        <div className="task-panel" role="dialog" aria-label="任务">
          {active.length > 0 && <>
            <div className="task-section">
              <span>进行中</span><span className="task-section-count">{active.length}</span>
            </div>
            {active.map(openRow)}
          </>}
          {done.length > 0 && <>
            <div className="task-section">
              <span>已结束</span><span className="task-section-count">{done.length}</span>
              <button className="task-clear" onClick={() => void clearFinishedTasks().catch(fail)}>
                <i className="ti ti-clear-all" />清除已结束
              </button>
            </div>
            {done.map(doneRow)}
          </>}
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
