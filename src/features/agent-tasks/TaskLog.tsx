import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Modal } from "../../ui";
import { agentTaskLog, isOpenTask, type AgentTask } from "./taskStore";
import { AiAskModal, AiButton, askAi } from "../ai/AiAsk";
import { ACTION_TEXT } from "./useTaskToasts";

const POLL_MS = 1000;

/**
 * A task's log that keeps up while the task runs: it re-reads the log every second, and
 * once more when the task finishes, following the newest line unless the reader has
 * scrolled up to look at something earlier.
 */
export function TaskLogModal({ task, onClose }: { task: AgentTask; onClose: () => void }) {
  const [lines, setLines] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const pre = useRef<HTMLPreElement>(null);
  const follow = useRef(true);
  const running = isOpenTask(task);

  useEffect(() => {
    let alive = true;
    const read = () =>
      agentTaskLog(task.id).then(
        (next) => { if (alive) { setLines(next); setError(null); } },
        (err: unknown) => { if (alive) setError(String(err)); },
      );
    void read();
    if (!running) return () => { alive = false; };
    const timer = setInterval(() => void read(), POLL_MS);
    return () => { alive = false; clearInterval(timer); };
  }, [task.id, running]);

  useLayoutEffect(() => {
    const el = pre.current;
    if (el && follow.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const onScroll = () => {
    const el = pre.current;
    if (el) follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  };

  const text = error
    ? `读取任务日志失败：${error}`
    : lines === null ? "正在读取…" : lines.length ? lines.join("\n") : "暂无日志";
  // A failure is the moment a second opinion helps: the log goes to the AI, nothing else.
  const failed = task.state === "failed";
  return (
    <Modal title={`${task.surfaceLabel} · 任务日志${running ? "（实时）" : ""}`} icon="ti-file-text" wide onClose={onClose}
      footer={failed ? <AiButton label="问问 AI 为什么失败" disabled={!lines?.length} onClick={() => setAsking(true)} /> : undefined}>
      <pre ref={pre} className="task-log mono" onScroll={onScroll}>{text}</pre>
      {asking && <AiAskModal title="为什么失败" sub={`${task.surfaceLabel} · ${ACTION_TEXT[task.action]}`}
        note="只把这次任务的日志（最多最后 8000 字）和产品名发给 AI；AI 只给建议，不会替你执行任何操作。"
        run={() => askAi("install_failure", {
          product: task.productName, surface: task.surfaceLabel, action: ACTION_TEXT[task.action],
          message: task.message ?? task.lastLine ?? "", log: (lines ?? []).join("\n"),
        })}
        onClose={() => setAsking(false)} />}
    </Modal>
  );
}
