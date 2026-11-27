import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "../../../i18n";
import { Modal, useToast } from "../../../ui";
import {
  STEP_LABEL, checkLowSpaceNow, clampThreshold, driveName, freePercent, itemLabel, lastRunOf, lowSpaceStatus, reasonLabel, saveLowSpace, stillLow,
  type LowSpaceProgress, type LowSpaceRun, type LowSpaceStatus,
} from "../lowSpace";
import { formatSpaceBytes as bytes } from "./DevelopmentArtifacts";

const RUNNING_TEXT = "正在自动清理 {drive}";

/** The switch for cleaning a disk by itself when it runs low, and what the last runs did. */
export function LowSpaceAuto() {
  const { tr, locale } = useI18n();
  const toast = useToast();
  const [status, setStatus] = useState<LowSpaceStatus | null>(null);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [checking, setChecking] = useState(false);
  const [recordsOpen, setRecordsOpen] = useState(false);

  const load = useCallback(() => {
    lowSpaceStatus().then((next) => { setStatus(next); setDraft(String(next.thresholdPercent)); }).catch(() => undefined);
  }, []);

  useEffect(() => {
    load();
    let alive = true;
    const stops: (() => void)[] = [];
    const keep = (stop: () => void) => { if (alive) stops.push(stop); else stop(); };
    void listen<LowSpaceProgress | null>("low-space-progress", (event) => {
      setStatus((old) => old && { ...old, running: event.payload });
    }).then(keep).catch(() => undefined);
    void listen<LowSpaceRun>("low-space-cleaned", () => load()).then(keep).catch(() => undefined);
    return () => { alive = false; stops.forEach((stop) => stop()); };
  }, [load]);

  async function save(enabled: boolean, threshold: number) {
    setSaving(true);
    try {
      const next = await saveLowSpace(enabled, threshold);
      setStatus(next);
      setDraft(String(next.thresholdPercent));
    } catch {
      toast(tr("无法保存自动清理设置，请重试。"), "err");
      if (status) setDraft(String(status.thresholdPercent));
    } finally {
      setSaving(false);
    }
  }

  function commitThreshold() {
    if (!status) return;
    const value = clampThreshold(Number(draft));
    setDraft(String(value));
    if (value !== status.thresholdPercent) void save(status.enabled, value);
  }

  async function checkNow() {
    setChecking(true);
    try {
      const outcome = await checkLowSpaceNow();
      if (outcome.cleaning.length === 0) {
        toast(tr("所有磁盘剩余都高于 {percent}%，不需要清理。").replace("{percent}", String(status?.thresholdPercent ?? 5)), "ok");
      } else {
        toast(tr("{drives} 空间不足，正在后台清理。").replace("{drives}", outcome.cleaning.map(driveName).join("、")), "info");
      }
      load();
    } catch (error) {
      toast(tr(String(error) === "E_BUSY" ? "自动清理正在进行，请等它完成。" : "无法开始检查，请重试。"), "err");
    } finally {
      setChecking(false);
    }
  }

  if (!status) return null;
  const last = status.runs[0];
  const running = status.running;
  const lowDisks = status.disks.filter((disk) => stillLow({ freeAfter: disk.free, total: disk.total }, status.thresholdPercent));
  return <section className="low-space" aria-label={tr("空间不足自动清理")}>
    <div className="low-space-row">
      <label className="low-space-toggle">
        <input className="ck2" type="checkbox" checked={status.enabled} disabled={saving}
          onChange={(event) => void save(event.target.checked, status.thresholdPercent)} />
        <b>{tr("空间不足时自动清理")}</b>
      </label>
      <label className="low-space-threshold">
        {tr("剩余空间不超过")}
        <input className="ip" type="number" min={1} max={50} value={draft} disabled={saving} aria-label={tr("自动清理的剩余空间比例")}
          onChange={(event) => setDraft(event.target.value)} onBlur={commitThreshold}
          onKeyDown={(event) => { if (event.key === "Enter") commitThreshold(); }} />
        %
      </label>
      <span className="low-space-actions">
        {running && <span className="low-space-running"><i className="ti ti-loader spin" />
          {tr(RUNNING_TEXT).replace("{drive}", driveName(running.drive))}{running.step ? ` · ${tr(STEP_LABEL[running.step])}` : ""}</span>}
        <button className="gh sm" disabled={checking || !!running} onClick={() => void checkNow()} title={tr("现在就看一遍各个磁盘，空间不足的立即清理")}>
          <i className={`ti ${checking ? "ti-loader spin" : "ti-radar"}`} />{tr("立即检查")}
        </button>
        {status.runs.length > 0 && <button className="gh sm" onClick={() => setRecordsOpen(true)}><i className="ti ti-history" />{tr("自动清理记录")}</button>}
      </span>
    </div>
    <p className="low-space-hint">{tr("依次清理：闲置项目的开发产物 → 在用项目的开发产物 → 缓存与下载 → 系统盘的临时文件与系统缓存，清到剩余达到设定值的两倍为止。源码、文档、虚拟环境、模型和回收站从不自动删除。")}</p>
    {lowDisks.length > 0 ? <div className="low-space-disks">
      {lowDisks.map((disk) => {
        const run = lastRunOf(status.runs, disk.root);
        return <p className="low-space-last" key={disk.root}>
          <b>{driveName(disk.root)}</b>
          <span>{tr("剩余")} {bytes(disk.free)}（{freePercent(disk)}%）</span>
          {!run ? <span>{tr("等待下一次检查")}</span>
            : <span>{new Date(run.finishedAt * 1000).toLocaleString(locale)} · {run.freed > 0
              ? <>{tr("自动清理释放")} <b>{bytes(run.freed)}</b></>
              : tr("已检查，没有能自动清理的内容")}</span>}
        </p>;
      })}
      {lowDisks.some((disk) => lastRunOf(status.runs, disk.root)) &&
        <p className="low-space-hint warn">{tr("剩下的是你自己的文件：备份、安装包、虚拟机、下载和视频等，自动清理不会删。点上方的磁盘卡片做深度分析，再决定删什么。")}</p>}
    </div> : last && <p className="low-space-last">
      <span>{tr("上次")} {driveName(last.drive)} · {new Date(last.finishedAt * 1000).toLocaleString(locale)}</span>
      <span>{tr("释放")} <b>{bytes(last.freed)}</b></span>
      <span>{tr("剩余")} {bytes(last.freeBefore)} → {bytes(last.freeAfter)}</span>
    </p>}
    {recordsOpen && <LowSpaceRecords runs={status.runs} onClose={() => setRecordsOpen(false)} />}
  </section>;
}

function LowSpaceRecords({ runs, onClose }: { runs: LowSpaceRun[]; onClose: () => void }) {
  const { tr, locale } = useI18n();
  const [open, setOpen] = useState<number | null>(0);
  return <Modal wide title={tr("自动清理记录")} icon="ti-history"
    sub={tr("每次因空间不足自动清理都记在这里：哪一步、删了什么、释放了多少。")}
    onClose={onClose} footer={<button className="pr sm" onClick={onClose}>{tr("关闭")}</button>}>
    <div className="clean-history">
      {runs.map((run, index) => <div className={"clean-history-row" + (open === index ? " open" : "")} key={`${run.drive}-${run.at}`}>
        <button className="clean-history-head" onClick={() => setOpen(open === index ? null : index)}>
          <i className={"ti " + (open === index ? "ti-chevron-down" : "ti-chevron-right")} />
          <span className="when">{new Date(run.finishedAt * 1000).toLocaleString(locale)}</span>
          <span className="bd n">{driveName(run.drive)}</span>
          <span className="s dim">{tr(run.manual ? "手动检查" : "自动")} · {tr("剩余")} {bytes(run.freeBefore)} → {bytes(run.freeAfter)}</span>
          <b>{bytes(run.freed)}</b>
        </button>
        {open === index && <div className="clean-history-items">
          {run.steps.length === 0 && <div><i className="ti ti-info-circle" /><span>{tr("没有找到可以自动清理的内容。")}</span></div>}
          {run.steps.map((step) => <div className="low-space-step" key={step.step}>
            <div className="low-space-step-head"><b>{tr(STEP_LABEL[step.step])}</b><b>{bytes(step.freed)}</b></div>
            {step.items.map((item) => <div key={item.path}>
              <i className={"ti " + (item.error && !item.freed ? "ti-alert-circle" : "ti-circle-check")} />
              <span className="mono" title={item.path}>{tr(itemLabel(item.label))} · {item.path}</span>
              <small>{item.error ? tr(reasonLabel(item.error)) : ""}</small>
              <b>{bytes(item.freed)}</b>
            </div>)}
            {step.more > 0 && <div><i className="ti ti-dots" /><span>{tr("另有 {count} 项").replace("{count}", String(step.more))}</span></div>}
          </div>)}
          {run.notes.map((note) => <div key={note}><i className="ti ti-info-circle" /><span>{tr(note)}</span></div>)}
        </div>}
      </div>)}
    </div>
  </Modal>;
}
