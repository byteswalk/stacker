import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { executeLeftovers, leftoversJob, previewLeftovers } from "./api";
import { errorMessage, type CleanupJob, type LeftoverPreview } from "./types";

const STATE: Record<string, string> = { verifying: "正在复核", running: "正在清理", completed: "已完成", failed: "部分失败" };

export function LeftoversDialog({ ids, onClose }: { ids: string[]; onClose: (changed: boolean) => void }) {
  const { tr: t } = useI18n();
  const [preview, setPreview] = useState<LeftoverPreview | null>(null);
  const [job, setJob] = useState<CleanupJob | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    previewLeftovers(ids).then(setPreview).catch((e) => setError(errorMessage(e)));
  }, [ids]);

  const active = job?.state === "verifying" || job?.state === "running";
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => {
      void leftoversJob().then((next) => { if (next) setJob(next); }).catch((e) => setError(errorMessage(e)));
    }, 800);
    return () => clearInterval(timer);
  }, [active]);

  async function confirm() {
    if (!preview) return;
    try { setJob(await executeLeftovers(preview.token)); } catch (e) { setError(errorMessage(e)); }
  }

  const slimming = !!preview?.items.some((i) => i.kind === "images");
  const footer = job
    ? <button className="pr sm" disabled={active} onClick={() => onClose(true)}>{t("完成")}</button>
    : <>
      <button className="gh sm" onClick={() => onClose(false)}>{t("取消")}</button>
      <button className="pr sm danger" disabled={!preview?.items.length} onClick={() => void confirm()}><i className="ti ti-trash" />{t("清理")} {preview ? bytes(preview.bytes) : ""}</button>
    </>;

  return <Modal wide title={t("清理会话残留")} icon="ti-trash" onClose={active ? undefined : () => onClose(!!job)} footer={footer}>
    <div className="session-delete">
      {!job ? <>
        {!preview && !error && <p className="session-note"><i className="ti ti-loader spin" /> {t("正在核对所选项…")}</p>}
        {preview && <>
          <p className="session-impact">{t("将清理")} <b>{preview.items.length}</b> {t("项")} · {t("可释放")} <b>{bytes(preview.bytes)}</b></p>
          <div className="session-results">{preview.items.map((i) => <div key={i.id}><span title={i.path}>{t(i.label)}</span><small>{bytes(i.bytes)}</small></div>)}</div>
          {!!preview.blocked.length && <div className="session-blocked">
            <b>{t("以下项目不会清理")} ({preview.blocked.length})</b>
            {preview.blocked.map((i) => <div key={i.id}><span>{t(i.label)}</span><small>{t(i.blocked ?? "")}</small></div>)}
          </div>}
          <p className="session-note">{t("编译产物、依赖和临时目录会永久删除，需要时重新编译或安装即可恢复。清理前会重新查找一遍，大小变了或有程序在用就停下。")}</p>
          {slimming && <p className="session-note">{t("截图瘦身会改写会话记录：截图换成 1 像素小图，文字全部保留；客户端正在写的记录会跳过。")}</p>}
        </>}
      </> : <>
        <p className="session-impact"><b>{t(STATE[job.state] ?? job.state)}</b> · {job.done} / {job.total} · {t("已释放")} {bytes(job.freed)}</p>
        {active && <progress max={Math.max(1, job.total)} value={job.done} />}
        {job.error && <p role="alert" className="session-error">{t(errorMessage(job.error))}</p>}
        <div className="session-results">{job.items.map((item) => <div key={item.id} className={item.status}>
          <i className={`ti ${item.status === "completed" ? "ti-circle-check" : "ti-alert-circle"}`} /><span>{t(item.label)}</span>
          <small>{item.detail ? t(errorMessage(item.detail)) : bytes(item.freed)}</small>
        </div>)}</div>
      </>}
      {error && <p role="alert" className="session-error">{t(error)}</p>}
    </div>
  </Modal>;
}
