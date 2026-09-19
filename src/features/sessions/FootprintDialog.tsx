import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { cleanupJob, executeCleanup, previewCleanup } from "./api";
import { errorMessage, type CleanupJob, type CleanupPreview } from "./types";

const STATE: Record<string, string> = { verifying: "正在复核", running: "正在清理", completed: "已完成", failed: "部分失败" };

export function FootprintDialog({ ids, onClose }: { ids: string[]; onClose: (changed: boolean) => void }) {
  const { tr: t } = useI18n();
  const [preview, setPreview] = useState<CleanupPreview | null>(null);
  const [job, setJob] = useState<CleanupJob | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    previewCleanup(ids).then(setPreview).catch((e) => setError(errorMessage(e)));
  }, [ids]);

  const active = job?.state === "verifying" || job?.state === "running";
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => {
      void cleanupJob().then((next) => { if (next) setJob(next); }).catch((e) => setError(errorMessage(e)));
    }, 800);
    return () => clearInterval(timer);
  }, [active]);

  async function confirm() {
    if (!preview) return;
    try { setJob(await executeCleanup(preview.token)); } catch (e) { setError(errorMessage(e)); }
  }

  const footer = job
    ? <button className="pr sm" disabled={active} onClick={() => onClose(true)}>{t("完成")}</button>
    : <>
      <button className="gh sm" onClick={() => onClose(false)}>{t("取消")}</button>
      <button className="pr sm danger" disabled={!preview?.items.length} onClick={() => void confirm()}><i className="ti ti-trash" />{t("清理")} {preview ? bytes(preview.bytes) : ""}</button>
    </>;

  return <Modal wide title={t("清理智能体数据")} icon="ti-trash" onClose={active ? undefined : () => onClose(!!job)} footer={footer}>
    <div className="session-delete">
      {!job ? <>
        {!preview && !error && <p className="session-note"><i className="ti ti-loader spin" /> {t("正在重新统计并核对…")}</p>}
        {preview && <>
          <p className="session-impact">{t("将删除")} <b>{preview.items.length}</b> {t("项")} · {t("可释放")} <b>{bytes(preview.bytes)}</b></p>
          <div className="session-results">{preview.items.map((i) => <div key={i.id}><span title={i.paths.join("\n")}>{t(i.label)}</span><small>{bytes(i.bytes)}</small></div>)}</div>
          {!!preview.blocked.length && <div className="session-blocked">
            <b>{t("以下项目不会清理")} ({preview.blocked.length})</b>
            {preview.blocked.map((i) => <div key={i.id}><span>{t(i.label)}</span><small>{t(errorMessage(i.blocked))}</small></div>)}
          </div>}
          <p className="session-note">{t("清理前会重新统计，大小有变化时自动停止。不会删除会话记录、配置或登录信息。")}</p>
        </>}
      </> : <>
        <p className="session-impact"><b>{t(STATE[job.state] ?? job.state)}</b> · {job.done} / {job.total} · {t("已释放")} {bytes(job.freed)}</p>
        {active && <progress max={Math.max(1, job.total)} value={job.done} />}
        {job.error && <p role="alert" className="session-error">{t(errorMessage(job.error))}</p>}
        <div className="session-results">{job.items.map((item) => <div key={item.id} className={item.status}>
          <i className={`ti ${item.status === "completed" ? "ti-circle-check" : "ti-alert-circle"}`} /><span>{t(item.label)}</span>
          <small>{item.status === "completed" ? bytes(item.freed) : t(errorMessage(item.detail))}</small>
        </div>)}</div>
      </>}
      {error && <p role="alert" className="session-error">{t(error)}</p>}
    </div>
  </Modal>;
}
