import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Modal, useToast } from "../../ui";
import { Select } from "../../Select";
import { agentRepairers, agentTaskLog, isOpenTask, startAgentRepair, type AgentTask, type Repairer } from "./taskStore";
import { AiAskModal, AiButton, askAi } from "../ai/AiAsk";
import { ACTION_TEXT } from "./useTaskToasts";

const POLL_MS = 1000;

/** Why a task failed, asked of the AI with the task's log; `lines` when the log is already on screen. */
export function TaskFailureAsk({ task, lines, onClose, onSetup }: {
  task: AgentTask; lines?: string[]; onClose: () => void; onSetup?: () => void;
}) {
  return <AiAskModal title="为什么失败" sub={`${task.surfaceLabel} · ${ACTION_TEXT[task.action]}`}
    note="只把这次任务的日志（最多最后 8000 字）和产品名发给 AI；AI 只给建议，不会替你执行任何操作。"
    run={async () => askAi("install_failure", {
      product: task.productName, surface: task.surfaceLabel, action: ACTION_TEXT[task.action],
      message: task.message ?? task.lastLine ?? "", log: (lines ?? await agentTaskLog(task.id)).join("\n"),
    })}
    saveAs={`task:${task.id}`} onClose={onClose} onSetup={onSetup} />;
}

/** The button beside 「AI 诊断」 on a failed task. */
export function RepairButton({ onClick }: { onClick: () => void }) {
  return <button className="gh sm" title="在终端里打开你已登录的智能体，让它查明原因并修好" onClick={onClick}>
    <i className="ti ti-tool" /> 交给智能体修复
  </button>;
}

/** Hands a failed task to a signed-in agent CLI, which works in a terminal the user watches. */
export function TaskRepairDialog({ task, onClose }: { task: AgentTask; onClose: () => void }) {
  const toast = useToast();
  const [agents, setAgents] = useState<Repairer[] | null>(null);
  const [picked, setPicked] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    agentRepairers(task.id).then(
      (found) => { if (alive) { setAgents(found); setPicked(found[0]?.id ?? ""); } },
      () => { if (alive) setAgents([]); },
    );
    return () => { alive = false; };
  }, [task.id]);

  async function start() {
    setBusy(true);
    try {
      await startAgentRepair(task.id, picked);
      toast("已在终端打开智能体。修好后回到这里点「重试」确认。", "ok");
      onClose();
    } catch (error) {
      toast("打开智能体失败：" + error, "err");
      setBusy(false);
    }
  }

  const name = agents?.find((agent) => agent.id === picked)?.name;
  return <Modal title="交给智能体修复" icon="ti-tool" sub={`${task.surfaceLabel} · ${ACTION_TEXT[task.action]}`} onClose={onClose}
    footer={<>
      <button className="gh sm" onClick={onClose}>取消</button>
      {!!agents?.length && <button className="pr sm" disabled={busy} onClick={() => void start()}><i className="ti ti-terminal-2" /> 打开 {name}</button>}
    </>}>
    {agents === null && <div className="ai-explain wait"><i className="ti ti-loader spin" /> 正在查找可用的智能体…</div>}
    {agents?.length === 0 && <div className="banner amber" style={{ margin: 0 }}><i className="ti ti-info-circle lead" />
      <div className="bt">没有找到可用的智能体。先装好并登录 Codex 或 Claude Code；现在也可以用「AI 诊断」看原因。</div></div>}
    {!!agents?.length && <>
      {agents.length > 1 && <div className="field"><label>用哪个智能体</label>
        <Select value={picked} width={220} onChange={setPicked} options={agents.map((agent) => ({ value: agent.id, label: agent.name }))} /></div>}
      <div className="s dim" style={{ whiteSpace: "normal" }}>{name} 会在终端里读取这次的日志，查明原因后动手修。要执行的命令由它按自己的规则问你确认，密钥会先遮掉。修好后回到这里点「重试」确认。</div>
    </>}
  </Modal>;
}

/**
 * A task's log that keeps up while the task runs: it re-reads the log every second, and
 * once more when the task finishes, following the newest line unless the reader has
 * scrolled up to look at something earlier.
 */
export function TaskLogModal({ task, onClose }: { task: AgentTask; onClose: () => void }) {
  const [lines, setLines] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const [repairing, setRepairing] = useState(false);
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
      footer={failed ? <>
        <AiButton label="AI 诊断" title="把这次任务的日志交给 AI，看是哪里出的错、怎么处理" disabled={!lines?.length} onClick={() => setAsking(true)} />
        <RepairButton onClick={() => setRepairing(true)} />
      </> : undefined}>
      <pre ref={pre} className="task-log mono" onScroll={onScroll}>{text}</pre>
      {asking && <TaskFailureAsk task={task} lines={lines ?? []} onClose={() => setAsking(false)}
        onSetup={() => { setAsking(false); onClose(); }} />}
      {repairing && <TaskRepairDialog task={task} onClose={() => setRepairing(false)} />}
    </Modal>
  );
}
