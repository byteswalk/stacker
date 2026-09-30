import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { cancelSummary, previewHandoff, previewSummary, startHandoff, startSummary, summaryJob } from "./api";
import { errorMessage, type RunnerChoice, type SummaryJob, type SummaryPreview } from "./types";

const STATUS: Record<string, string> = { queued: "排队中", running: "生成中", completed: "已完成", failed: "失败", skipped: "已跳过" };
const JOB_STATE: Record<string, string> = { running: "进行中", completed: "已完成", failed: "部分失败", cancelled: "已取消" };

/** Which AI source will answer, as the confirmation names it. */
export function runnerText(c: RunnerChoice | null | undefined, t: (s: string) => string) {
  if (!c) return t("尚未配置的 AI");
  if (!c.backend) return `${t("外部 API")} · ${c.model ?? t("默认模型")}`;
  return `${t("本机")} ${c.backend}${c.model ? ` / ${c.model}` : ""}`;
}

function formatChars(n: number, t: (s: string) => string) {
  return n >= 10000 ? `${(n / 10000).toFixed(1)} ${t("万字")}` : `${n} ${t("字")}`;
}

type Target = { kind: "summary"; ids: string[] } | { kind: "handoff"; project: string };

/** Confirm → run → results, for session summaries and project handoffs. */
export function SummaryDialog({ target, onClose }: { target: Target; onClose: (changed: boolean) => void }) {
  const { tr: t, locale } = useI18n();
  const [regenerate, setRegenerate] = useState(false);
  const [limit, setLimit] = useState(10);
  const [preview, setPreview] = useState<SummaryPreview | null>(null);
  const [job, setJob] = useState<SummaryJob | null>(null);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const request = useRef(0);

  useEffect(() => {
    const current = ++request.current;
    const load = target.kind === "summary" ? previewSummary(target.ids, regenerate) : previewHandoff(target.project, limit);
    load.then((p) => { if (current === request.current) { setPreview(p); setError(""); } })
      .catch((e) => { if (current === request.current) setError(errorMessage(e)); });
  }, [target, regenerate, limit]);

  const running = job?.state === "running";
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => {
      void summaryJob().then((next) => { if (next) setJob(next); }).catch((e) => setError(errorMessage(e)));
    }, 1000);
    return () => clearInterval(timer);
  }, [running]);

  async function start() {
    setError("");
    try {
      setJob(target.kind === "summary"
        ? await startSummary(target.ids, regenerate, locale)
        : await startHandoff(target.project, limit, locale));
    } catch (e) { setError(errorMessage(e)); }
  }

  const needed = preview?.items.filter((i) => i.needed) ?? [];
  const title = target.kind === "summary" ? t("生成会话摘要") : `${t("生成交接资料")} · ${preview?.projectName ?? ""}`;
  const canStart = !!preview && (needed.length > 0 || target.kind === "handoff");

  const footer = job
    ? <>
      {running && <button className="gh sm" onClick={() => void cancelSummary()}>{t("取消后续项目")}</button>}
      {job.resultText && <button className="gh sm" onClick={() => void navigator.clipboard.writeText(job.resultText).then(() => setCopied(true))}><i className="ti ti-copy" />{t(copied ? "已复制" : "复制全文")}</button>}
      <button className="pr sm" disabled={running} onClick={() => onClose(true)}>{t("完成")}</button>
    </>
    : <>
      <button className="gh sm" onClick={() => onClose(false)}>{t("取消")}</button>
      <button className="pr sm" disabled={!canStart} onClick={() => void start()}><i className="ti ti-sparkles" />{t("开始生成")}</button>
    </>;

  return <Modal wide title={title} icon="ti-sparkles" onClose={running ? undefined : () => onClose(!!job)} footer={footer}>
    <div className="session-delete">
      {!job ? <>
        {target.kind === "handoff" && <label className="runner-field inline">
          <span>{t("纳入最近")}</span>
          <select className="ip" value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
            {[5, 10, 20, 0].map((n) => <option key={n} value={n}>{n ? `${n} ${t("个会话")}` : t("全部会话")}</option>)}
          </select>
        </label>}
        {preview && <p className="session-impact">
          {target.kind === "summary"
            ? <>{t("将为")} <b>{needed.length}</b> {t("个会话生成摘要")}</>
            : <>{t("纳入")} <b>{preview.items.length}</b> {t("个会话，其中")} <b>{needed.length}</b> {t("个需要先生成摘要")}</>}
          {" · "}{t("将发送约")} <b>{formatChars(preview.totalChars, t)}</b>
          {preview.items.length > needed.length && target.kind === "summary" && <> · {preview.items.length - needed.length} {t("个已有摘要，跳过")}</>}
        </p>}
        {target.kind === "summary" && <label className="session-check"><input type="checkbox" checked={regenerate} onChange={(e) => setRegenerate(e.target.checked)} />{t("重新生成已有摘要")}</label>}
        {preview && <p className="session-note"><i className="ti ti-sparkles" /> {t("执行者")}：<b>{runnerText(preview.handoffRunner ?? preview.items[0]?.runner, t)}</b>{" · "}<span className="dim">{t("在「偏好设置 → AI 能力」更换")}</span></p>}
        <p className="session-note"><i className="ti ti-shield-lock" /> {t("会话正文会发送给上面这个 AI，用的是它的账号额度。运行时不开放任何工具，也不会在智能体里留下新会话。")}</p>
      </> : <>
        <p className="session-impact"><b>{t(JOB_STATE[job.state] ?? job.state)}</b> · {job.done} / {job.total}</p>
        {running && <progress max={Math.max(1, job.total)} value={job.done} />}
        <div className="session-results">{job.items.map((i) => <div key={i.id} className={i.status}>
          <i className={`ti ${i.status === "completed" ? "ti-circle-check" : i.status === "failed" ? "ti-alert-circle" : i.status === "running" ? "ti-loader spin" : "ti-clock"}`} />
          <span title={i.title}>{i.title}</span>
          <small>{i.status === "failed" ? t(errorMessage(i.detail)) : `${t(STATUS[i.status] ?? i.status)}${i.elapsedMs ? ` · ${Math.round(i.elapsedMs / 1000)}s` : ""}`}</small>
        </div>)}</div>
        {job.error && <p role="alert" className="session-error">{t(errorMessage(job.error))}</p>}
        {job.resultText && <>
          <p className="session-note">{t("已保存到")} <code>{job.resultPath}</code></p>
          <pre className="handoff-text" translate="no">{job.resultText}</pre>
        </>}
      </>}
      {error && <p role="alert" className="session-error">{t(error)}</p>}
    </div>
  </Modal>;
}
