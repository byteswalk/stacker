import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { cancelDelete, deleteJob, executeDelete, openSession, previewDelete } from "./api";
import { errorMessage, type DeleteJob, type DeleteMode, type DeletePreview } from "./types";

const MODES: { value: DeleteMode; label: string; hint: string }[] = [
  { value: "slim_export", label: "精简导出后删除", hint: "把用户、助手的正文和工具调用摘要存成 Markdown，不含压缩快照和图片数据，然后删除原记录。" },
  { value: "direct", label: "直接删除", hint: "不留任何副本，立即释放空间。" },
  { value: "full_backup", label: "完整备份后删除", hint: "把原始记录完整复制到 Stacker 数据目录后删除，不释放空间。" },
];

const JOB_STATE: Record<string, string> = { running: "执行中", completed: "已完成", failed: "部分失败", cancelled: "已取消" };

export function DeleteDialog({ ids, onClose }: { ids: string[]; onClose: (changed: boolean) => void }) {
  const { tr: t } = useI18n();
  const [mode, setMode] = useState<DeleteMode>("slim_export");
  const [preview, setPreview] = useState<DeletePreview | null>(null);
  const [job, setJob] = useState<DeleteJob | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const generation = useRef(0);

  useEffect(() => {
    const current = ++generation.current;
    setLoading(true); setError("");
    previewDelete(ids, mode)
      .then((p) => { if (current === generation.current) setPreview(p); })
      .catch((e) => { if (current === generation.current) setError(errorMessage(e)); })
      .finally(() => { if (current === generation.current) setLoading(false); });
  }, [ids, mode]);

  useEffect(() => {
    if (!job || job.state !== "running") return;
    const timer = window.setInterval(() => {
      void deleteJob().then((next) => { if (next) setJob(next); }).catch((e) => setError(errorMessage(e)));
    }, 800);
    return () => clearInterval(timer);
  }, [job]);

  async function confirm() {
    if (!preview) return;
    setError("");
    try { setJob(await executeDelete(preview.token)); }
    catch (e) { setError(errorMessage(e)); }
  }

  const running = job?.state === "running";
  const done = !!job && !running;
  const footer = job
    ? <>
      {running && <button className="gh sm" onClick={() => void cancelDelete()}>{t("取消后续项目")}</button>}
      {done && job.items.some((i) => i.status === "completed") && mode === "slim_export" && <button className="gh sm" onClick={() => void openSession("", "exports")}><i className="ti ti-folder-open" />{t("打开导出目录")}</button>}
      <button className="pr sm" disabled={running} onClick={() => onClose(true)}>{t("完成")}</button>
    </>
    : <>
      <button className="gh sm" onClick={() => onClose(false)}>{t("取消")}</button>
      <button className="pr sm danger" disabled={loading || !preview?.sessions.length} onClick={() => void confirm()}><i className="ti ti-trash" />{t("删除")} {preview?.sessions.length ?? 0} {t("个会话")}</button>
    </>;

  return <Modal wide title={t("删除会话")} icon="ti-trash" onClose={running ? undefined : () => onClose(!!job)} footer={footer}>
    {!job ? <div className="session-delete">
      <div className="session-modes" role="radiogroup">
        {MODES.map((m) => <label key={m.value} className={`session-mode ${mode === m.value ? "on" : ""}`}>
          <input type="radio" name="delete-mode" checked={mode === m.value} onChange={() => setMode(m.value)} />
          <span><b>{t(m.label)}</b><small>{t(m.hint)}</small></span>
        </label>)}
      </div>
      {loading ? <p className="session-note"><i className="ti ti-loader spin" /> {t("正在核对影响范围…")}</p> : preview && <>
        <p className="session-impact">{t("将删除")} <b>{preview.sessions.length}</b> {t("个会话")}{preview.children > 0 && <>（{t("含")} {preview.children} {t("个子任务")}）</>} · {preview.files} {t("个文件或目录")} · {t("可释放")} <b>{bytes(preview.bytes)}</b></p>
        {mode === "slim_export" && <p className="session-note">{t("导出位置")} <code>{preview.exportDir}</code></p>}
        {!!preview.blocked.length && <div className="session-blocked">
          <b>{t("以下会话不会删除")} ({preview.blocked.length})</b>
          {preview.blocked.map((b) => <div key={b.id}><span title={b.title}>{b.title}</span><small>{t(errorMessage(b.reason))}</small></div>)}
        </div>}
        <p className="session-note">{t("只删除智能体的会话记录，不会删除项目源码、工作树或构建缓存。")}</p>
      </>}
    </div> : <div className="session-delete">
      <p className="session-impact"><b>{t(JOB_STATE[job.state] ?? job.state)}</b> · {job.done} / {job.total}</p>
      {running && <progress max={Math.max(1, job.total)} value={job.done} />}
      <div className="session-results">{job.items.map((item) => <div key={item.id} className={item.status}>
        <i className={`ti ${item.status === "completed" ? "ti-circle-check" : "ti-alert-circle"}`} /><span title={item.title}>{item.title}</span>
        {item.status === "failed" && <small>{t(errorMessage(item.detail))}</small>}
      </div>)}</div>
    </div>}
    {error && <p role="alert" className="session-error">{t(error)}</p>}
  </Modal>;
}
