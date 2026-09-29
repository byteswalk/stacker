import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { Modal, useToast } from "../../../ui";
import { formatSpaceBytes } from "./SpaceOverview";
import type { MonitorFileChange, MonitorSnapshot } from "../types";

const POLL_MS = 2000;

function signedBytes(bytes: number) {
  if (bytes === 0) return "0 B";
  return `${bytes > 0 ? "+" : "-"}${formatSpaceBytes(Math.abs(bytes))}`;
}

function formatUpdatedAt(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleTimeString();
}

export function SpaceMonitorModal({ roots, onClose }: { roots: string[]; onClose: () => void }) {
  const { tr } = useI18n();
  const toast = useToast();
  const [snapshot, setSnapshot] = useState<MonitorSnapshot | null>(null);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [starting, setStarting] = useState(true);

  useEffect(() => {
    let disposed = false;
    let timer: number | undefined;
    let activeId: string | null = null;
    async function start() {
      try {
        const id = await invoke<string>("space_monitor_start", { roots });
        activeId = id;
        if (disposed) {
          void invoke("space_monitor_stop", { taskId: id });
          return;
        }
        setTaskId(id);
        const read = async () => {
          try {
            const current = await invoke<MonitorSnapshot>("space_monitor_status", { taskId: id });
            if (!disposed) setSnapshot(current);
          } catch (error) {
            if (!disposed) toast(`${tr("无法读取空间追踪状态：")}${String(error)}`, "err");
          }
        };
        await read();
        timer = window.setInterval(() => void read(), POLL_MS);
      } catch (error) {
        if (!disposed) toast(`${tr("无法开始空间追踪：")}${String(error)}`, "err");
      } finally {
        if (!disposed) setStarting(false);
      }
    }
    void start();
    return () => {
      disposed = true;
      if (timer) window.clearInterval(timer);
      if (activeId) void invoke("space_monitor_stop", { taskId: activeId });
    };
    // The modal represents one fixed tracking session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const events = useMemo(() => snapshot?.events ?? [], [snapshot]);
  const directories = useMemo(() => snapshot?.directories ?? [], [snapshot]);
  async function stop() {
    if (taskId) await invoke("space_monitor_stop", { taskId });
    onClose();
  }

  async function openDirectory(path: string) {
    try {
      await invoke("space_open_directory", { path });
    } catch {
      toast(tr("无法打开文件夹，请确认路径仍然存在且当前账户有访问权限。"), "err");
    }
  }

  function changeLabel(change: MonitorFileChange) {
    if (change.kind === "added") return tr("新增");
    if (change.kind === "removed") return tr("删除");
    return tr("修改");
  }

  return (
    <Modal wide title={tr("空间实时追踪")} icon="ti-activity" sub={tr("只记录文件路径、大小和变更时间，不读取文件内容。")} onClose={() => void stop()}
      footer={<button className="gh sm" onClick={() => void stop()}><i className="ti ti-player-stop" /> {tr("停止追踪")}</button>}>
      <div className="space-monitor-roots">
        <strong>{tr("追踪范围")}</strong>
        {roots.map((root) => <span key={root} title={root}>{root}</span>)}
      </div>
      {snapshot && <div className="space-monitor-status">
        <span><i className={`ti ${snapshot.state === "running" ? "ti-player-play" : "ti-info-circle"}`} /> {snapshot.state === "running" ? tr("追踪中") : tr("追踪已停止")}</span>
        <span>{tr("文件变化实时更新")}</span>
        <span>{tr("最近检查")} {formatUpdatedAt(snapshot.updatedAt)}</span>
      </div>}
      {starting && <div className="space-monitor-state trace-card"><span className="border-runner" /><i className="ti ti-loader spin" /> {tr("正在建立初始快照…")}</div>}
      {snapshot?.error && <div className="banner danger"><i className="ti ti-alert-circle" /> {snapshot.error}</div>}
      {snapshot && <>
        <div className="space-monitor-metrics">
          <div><span>{tr("追踪期间空间变化")}</span><strong className={snapshot.deltaBytes > 0 ? "negative" : "positive"}>{signedBytes(snapshot.deltaBytes)}</strong></div>
          <div><span>{tr("变更文件")}</span><strong>{snapshot.filesChanged.toLocaleString()}</strong></div>
          <div><span>{tr("当前占用")}</span><strong>{formatSpaceBytes(snapshot.currentBytes)}</strong></div>
        </div>
        <div className="space-monitor-events-head"><strong>{tr("空间变化集中目录")}</strong><span>{directories.length.toLocaleString()}</span></div>
        {directories.length > 0 ? <div className="space-monitor-directories">
          {directories.map((directory) => <div className="space-monitor-directory" key={directory.path}>
            <i className="ti ti-folder" />
            <div><strong title={directory.path}>{directory.path}</strong><span>{directory.filesChanged.toLocaleString()} {tr("个文件发生变化")}</span></div>
            <b className={directory.deltaBytes > 0 ? "negative" : "positive"}>{signedBytes(directory.deltaBytes)}</b>
            <button className="space-icon-button" title={tr("打开目录")} onClick={() => void openDirectory(directory.path)}><i className="ti ti-folder-open" /></button>
          </div>)}
        </div> : <div className="space-analysis-empty compact"><i className="ti ti-clock" /> {tr("等待目录空间变化…")}</div>}
        <div className="space-monitor-events-head"><strong>{tr("最近文件变化")}</strong><span>{events.length.toLocaleString()}</span></div>
        {events.length > 0 ? <div className="space-monitor-events">
          {events.map((event) => <div className="space-monitor-event" key={`${event.kind}:${event.path}:${event.modifiedAt}`}>
            <i className={`ti ${event.kind === "added" ? "ti-file-plus" : event.kind === "removed" ? "ti-file-minus" : "ti-file-pencil"}`} />
            <div><strong title={event.path}>{event.path}</strong><span>{changeLabel(event)} · {signedBytes(event.deltaBytes)}</span></div>
            <button className="space-icon-button" title={tr("打开所在目录")} onClick={() => void openDirectory(event.path)}><i className="ti ti-folder-open" /></button>
          </div>)}
        </div> : <div className="space-analysis-empty"><i className="ti ti-clock" /> {tr("等待文件变化…")}</div>}
      </>}
    </Modal>
  );
}
