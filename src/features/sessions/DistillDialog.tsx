import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { ConfirmModal, Modal } from "../../ui";
import { cancelDistill, distillCandidates, distillJob, openDistill, previewDistill, startDistill } from "./api";
import { runnerText } from "./SummaryDialog";
import { DISTILL_KINDS, DISTILL_KIND_LABEL, errorMessage, type DistillCandidate, type DistillJob, type DistillPreview, type DistillSourceRef } from "./types";

const STAGE: Record<string, string> = { reading: "正在读取材料", distilling: "正在提炼", merging: "正在合并去重", saving: "正在保存" };
// `empty`：任务跑完了、没出错，但一条可用条目都没提炼出来，不能和 completed 看起来一样。
const JOB_STATE: Record<string, string> = { running: "进行中", completed: "已完成", empty: "没有提炼出可用内容", failed: "失败", cancelled: "已取消" };
const SOURCE_KIND: Record<string, string> = { web: "网页对话", session: "本机会话", excerpt: "摘录" };

function formatChars(n: number, t: (s: string) => string) {
  return n >= 10000 ? `${(n / 10000).toFixed(1)} ${t("万字")}` : `${n} ${t("字")}`;
}

/** 选来源与产出类型 → 告知并确认 → 运行 → 结果。 */
export function DistillDialog({ initial, onClose }: { initial: DistillSourceRef[]; onClose: (changed: boolean) => void }) {
  const { tr: t, locale } = useI18n();
  const [sources, setSources] = useState<DistillSourceRef[]>(initial);
  const [kinds, setKinds] = useState<string[]>(["qa", "requirement"]);
  const [preview, setPreview] = useState<DistillPreview | null>(null);
  const [picking, setPicking] = useState(false);
  const [search, setSearch] = useState("");
  const [candidates, setCandidates] = useState<DistillCandidate[]>([]);
  const [asking, setAsking] = useState(false);
  const [job, setJob] = useState<DistillJob | null>(null);
  const [error, setError] = useState("");
  const request = useRef(0);

  // 一个任务可能是上次打开这个对话框时启动的、对话框关掉了但任务还在跑：重新打开时
  // 先问一下有没有正在跑的任务，有就直接显示它的进度，而不是让用户点「开始提炼」时
  // 才发现后端已经在跑，只收到一个 E_DISTILL_BUSY。
  useEffect(() => {
    distillJob()
      .then((j) => { if (j?.state === "running") setJob(j); })
      .catch((e) => setError(errorMessage(e)));
  }, []);

  useEffect(() => {
    if (!sources.length) { setPreview(null); return; }
    const current = ++request.current;
    previewDistill(sources)
      .then((p) => { if (current === request.current) { setPreview(p); setError(""); } })
      .catch((e) => { if (current === request.current) { setPreview(null); setError(errorMessage(e)); } });
  }, [sources]);

  useEffect(() => {
    if (!picking) return;
    const timer = window.setTimeout(() => {
      void distillCandidates(search).then(setCandidates).catch((e) => setError(errorMessage(e)));
    }, 300);
    return () => clearTimeout(timer);
  }, [picking, search]);

  const running = job?.state === "running";
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => {
      void distillJob().then((next) => { if (next) setJob(next); }).catch((e) => setError(errorMessage(e)));
    }, 1000);
    return () => clearInterval(timer);
  }, [running]);

  const toggleKind = (kind: string) => setKinds((old) => old.includes(kind) ? old.filter((k) => k !== kind) : [...old, kind]);
  const drop = (ref: DistillSourceRef) => setSources((old) => old.filter((s) => !(s.kind === ref.kind && s.key === ref.key)));
  const add = (c: DistillCandidate) => setSources((old) => old.some((s) => s.kind === c.kind && s.key === c.key) ? old : [...old, { kind: c.kind, key: c.key }]);

  async function start() {
    setAsking(false); setError("");
    try { setJob(await startDistill(sources, kinds, locale)); }
    catch (e) { setError(errorMessage(e)); }
  }

  const canStart = !!preview && preview.items.length > 0 && kinds.length > 0;
  const footer = job ? <>
    {running && <button className="gh sm" onClick={() => void cancelDistill()}>{t("取消")}</button>}
    {!running && !!job.folders.length && <button className="gh sm" onClick={() => void openDistill("skills", "")}><i className="ti ti-folder" />{t("打开 skill 草稿文件夹")}</button>}
    <button className="pr sm" disabled={running} onClick={() => onClose(true)}>{t("完成")}</button>
  </> : <>
    <button className="gh sm" onClick={() => onClose(false)}>{t("取消")}</button>
    <button className="pr sm" disabled={!canStart} onClick={() => setAsking(true)}><i className="ti ti-bulb" />{t("开始提炼")}</button>
  </>;

  return <>
    <Modal wide title={t("提炼")} icon="ti-bulb" onClose={running ? undefined : () => onClose(!!job)} footer={footer}>
    <div className="session-delete">
      {!job ? <>
        <div className="distill-sources">
          <b>{t("材料")}</b>
          {sources.map((s) => <span key={`${s.kind}:${s.key}`} className="session-tag">
            {t(SOURCE_KIND[s.kind] ?? s.kind)} · {s.key}
            <button className="ic" aria-label={t("移除")} onClick={() => drop(s)}><i className="ti ti-x" /></button>
          </span>)}
          <button className="gh sm" onClick={() => setPicking((p) => !p)}><i className="ti ti-plus" />{t("添加来源")}</button>
        </div>
        {picking && <div className="distill-picker">
          <label className="session-search"><i className="ti ti-search" /><input value={search} aria-label={t("搜索来源")} placeholder={t("搜索网页对话、本机会话或摘录")} onChange={(e) => setSearch(e.target.value)} /></label>
          <div className="distill-candidates">
            {candidates.map((c) => <button key={`${c.kind}:${c.key}`} className="distill-candidate" disabled={!c.available} onClick={() => add(c)}>
              <b>{c.title || t("（无标题）")}</b>
              <span>{t(SOURCE_KIND[c.kind] ?? c.kind)} · {c.subtitle}{c.available ? "" : ` · ${t("未存正文")}`}</span>
            </button>)}
          </div>
        </div>}
        <div className="distill-kinds">
          <b>{t("产出类型")}</b>
          {DISTILL_KINDS.map((kind) => <label key={kind} className="session-check">
            <input type="checkbox" checked={kinds.includes(kind)} onChange={() => toggleKind(kind)} />{t(DISTILL_KIND_LABEL[kind])}
          </label>)}
        </div>
        {preview && <div className="distill-preview-items">
          {preview.items.map((item, i) => <div key={i} className="distill-preview-item"><span title={item.title}>{item.title || t("（无标题）")}</span><small>{formatChars(item.chars, t)}</small></div>)}
        </div>}
        {!!preview?.skipped && <p className="session-note">{preview.skipped} {t("个来源无法读取，已跳过。")}</p>}
        {preview && <p className="session-impact">
          {t("将提炼")} <b>{preview.items.length}</b> {t("份材料")} · {t("将发送约")} <b>{formatChars(preview.totalChars, t)}</b>
        </p>}
        {preview && <p className="session-note"><i className="ti ti-sparkles" /> {t("执行者")}：<b>{runnerText(preview.runner, t)}</b>{" · "}<span className="dim">{t("在「偏好设置 → AI 能力」更换")}</span></p>}
        <p className="session-note"><i className="ti ti-shield-lock" /> {t("材料正文会发送给上面这个 AI，用的是它的账号额度。运行时不开放任何工具，也不会在智能体里留下新会话。skill 草稿只写成本机文件夹，不会安装到任何智能体。")}</p>
      </> : <>
        <p className="session-impact"><b>{t(JOB_STATE[job.state] ?? job.state)}</b>{job.state === "running" ? ` · ${t(STAGE[job.stage] ?? job.stage)}` : ""} · {job.done} / {Math.max(1, job.total)}</p>
        {running && <progress max={Math.max(1, job.total)} value={job.done} />}
        {!running && <p className="session-note">{t("已保存")} <b>{job.saved}</b> {t("条")}{job.folders.length ? ` · ${t("skill 草稿")} ${job.folders.join("、")}` : ""}</p>}
        {!running && !!job.dropped && <p className="session-note">{t("另有")} <b>{job.dropped}</b> {t("条未保存（超出上限）")}</p>}
        {job.state === "failed" && job.error && <p role="alert" className="session-error">{t(errorMessage(job.error))}</p>}
      </>}
      {error && <p role="alert" className="session-error">{t(error)}</p>}
    </div>
    </Modal>
    {asking && preview && <ConfirmModal title={t("开始提炼")} icon="ti-bulb"
      message={`${t("将把选中的")} ${preview.items.length} ${t("份材料")}（${formatChars(preview.totalChars, t)}）${t("发送给")} ${runnerText(preview.runner, t)} ${t("提炼，消耗该账号的额度。")}`}
      confirmLabel={t("开始提炼")} onConfirm={() => void start()} onClose={() => setAsking(false)} />}
  </>;
}
