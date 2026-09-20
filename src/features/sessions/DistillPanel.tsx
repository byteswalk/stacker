import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { Select } from "../../Select";
import { ConfirmModal, Modal, useBusyRead, useToast } from "../../ui";
import { deleteDistill, exportDistill, listDistill, openDistill, saveDistill, setDistillState } from "./api";
import { DISTILL_KINDS, DISTILL_KIND_LABEL, distillSourceUrl, EMPTY_DISTILL_QUERY, errorMessage, type DistillPage, type DistillQuery, type DistillResult } from "./types";

const STATE_LABEL: Record<string, string> = { draft: "草稿", adopted: "已采用" };
const SOURCE_KIND: Record<string, string> = { web: "网页对话", session: "本机会话", excerpt: "摘录" };

// 与其他标签一样，页内切换时保留筛选。
let lastQuery = EMPTY_DISTILL_QUERY;

/** 「提炼」结果库：按类型与状态筛选、编辑、删除、标为已采用、导出。 */
export function DistillPanel({ refresh, onNew }: { refresh: number; onNew: () => void }) {
  const { tr: t, locale } = useI18n();
  const toast = useToast();
  const read = useBusyRead();
  const [query, setQuery] = useState<DistillQuery>(lastQuery);
  const [page, setPage] = useState<DistillPage | null>(null);
  const [open, setOpen] = useState<DistillResult | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState("");
  const request = useRef(0);

  const load = useCallback(async (q: DistillQuery) => {
    const generation = ++request.current;
    try {
      const next = await read("正在读取提炼结果", () => listDistill(q));
      if (generation === request.current) { setPage(next); setError(""); }
    } catch (e) {
      if (generation === request.current) setError(errorMessage(e));
    }
  }, [read]);

  useEffect(() => {
    lastQuery = query;
    const timer = window.setTimeout(() => void load(query), 300);
    return () => clearTimeout(timer);
  }, [query, load, refresh]);

  function show(item: DistillResult) {
    setOpen(item); setTitle(item.title); setBody(item.body);
  }

  async function apply<T>(task: () => Promise<T>, after?: (value: T) => void) {
    try { const value = await task(); after?.(value); await load(query); }
    catch (e) { toast(t(errorMessage(e)), "err"); }
  }

  const counts = page?.counts;
  const countOf = (kind: string) => counts ? counts[kind as keyof typeof counts] : 0;

  return <>
    <div className="session-filters distill-filters">
      <label className="session-search"><i className="ti ti-search" /><input value={query.search} aria-label={t("搜索提炼结果")} placeholder={t("搜索标题、正文或来源")} onChange={(e) => setQuery((q) => ({ ...q, search: e.target.value }))} /></label>
      <button className={`gh sm${query.kind === "" ? " active" : ""}`} onClick={() => setQuery((q) => ({ ...q, kind: "" }))}>{t("全部")} {counts?.total ?? 0}</button>
      {DISTILL_KINDS.map((kind) => <button key={kind} className={`gh sm${query.kind === kind ? " active" : ""}`} onClick={() => setQuery((q) => ({ ...q, kind }))}>{t(DISTILL_KIND_LABEL[kind])} {countOf(kind)}</button>)}
      <Select value={query.state} onChange={(state) => setQuery((q) => ({ ...q, state }))} options={[{ value: "", label: t("全部状态") }, { value: "draft", label: t("草稿") }, { value: "adopted", label: t("已采用") }]} />
      <button className="gh sm" onClick={onNew}><i className="ti ti-bulb" />{t("新建提炼…")}</button>
      <button className="gh sm" disabled={!page?.total} onClick={() => void apply(() => exportDistill(query, locale), (path) => toast(`${t("已导出到")} ${path}`, "ok"))}><i className="ti ti-file-export" />{t("导出")}</button>
      <button className="gh sm" onClick={() => void openDistill("skills", "")}><i className="ti ti-folder" />{t("skill 草稿")}</button>
    </div>
    {error && <div role="alert" className="session-error">{t(error)}</div>}
    {page && page.total === 0 && <div className="session-empty">
      <i className="ti ti-bulb" />
      <b>{t("还没有提炼结果")}</b>
      <span>{t("在会话或网页对话里选中材料后点「提炼…」，本机智能体会提炼出经验问答、领域要求、提示词和 skill 草稿。")}</span>
    </div>}
    {page && page.total > 0 && <div className="session-list">
      {page.items.map((item) => <div key={item.id} className="session-item"><div className="distill-row">
        <button className="distill-title session-title" onClick={() => show(item)}>
          <b>{item.title}</b>
          <span>{item.sources.map((s) => s.title || s.key).join("、")}</span>
        </button>
        <div className="session-tags">
          <span className="session-tag">{t(DISTILL_KIND_LABEL[item.kind])}</span>
          <span className="session-tag">{t(STATE_LABEL[item.state] ?? item.state)}</span>
        </div>
        <div className="session-time">{new Date(item.updatedAt).toLocaleDateString(locale)}</div>
      </div></div>)}
    </div>}
    {open && <Modal wide title={open.title} icon="ti-bulb" onClose={() => setOpen(null)} footer={<>
      <button className="gh sm" onClick={() => setDeleting(true)}><i className="ti ti-trash" />{t("删除")}</button>
      {open.kind === "skill" && !!open.folder && <button className="gh sm" onClick={() => void openDistill("skill", open.folder)}><i className="ti ti-folder" />{t("打开 skill 草稿文件夹")}</button>}
      <button className="gh sm" onClick={() => void apply(() => setDistillState(open.id, open.state === "adopted" ? "draft" : "adopted"), (next) => setOpen(next))}>
        {t(open.state === "adopted" ? "退回草稿" : "标为已采用")}
      </button>
      <button className="pr sm" disabled={title === open.title && body === open.body} onClick={() => void apply(() => saveDistill(open.id, title, body), (next) => { setOpen(next); toast(t("已保存"), "ok"); })}>{t("保存")}</button>
    </>}>
      <div className="session-delete">
        <div className="session-detail-meta">
          <span>{t(DISTILL_KIND_LABEL[open.kind])}</span>
          <span>{t(STATE_LABEL[open.state] ?? open.state)}</span>
          <span>{open.by}</span>
          <span>{new Date(open.updatedAt).toLocaleString(locale)}</span>
        </div>
        <label className="runner-field"><span>{t("标题")}</span><input className="ip" value={title} aria-label={t("标题")} onChange={(e) => setTitle(e.target.value)} /></label>
        <textarea className="ip distill-body" rows={14} aria-label={t("正文")} value={body} onChange={(e) => setBody(e.target.value)} />
        <div className="distill-sources">
          <b>{t("来源")}</b>
          {open.sources.map((s) => {
            const url = distillSourceUrl(s);
            return <span key={s.key} className="session-tag" title={s.link || s.key}>
              {t(SOURCE_KIND[s.kind] ?? s.kind)} · {url ? <a href={url} target="_blank" rel="noreferrer">{s.title || s.key}</a> : (s.title || s.key)}
            </span>;
          })}
        </div>
        {open.kind === "skill" && !!open.folder && <p className="session-note">{t("skill 草稿已写成文件夹")} <code>{open.folder}</code>{t("，没有安装到任何智能体。")}</p>}
      </div>
    </Modal>}
    {deleting && open && <ConfirmModal danger title={t("删除提炼结果")} message={`${t("将删除")}「${open.title}」。${t("已写出的 skill 草稿文件夹不会被删除。")}`}
      confirmLabel={t("删除")} onConfirm={() => { const id = open.id; setDeleting(false); setOpen(null); void apply(() => deleteDistill(id)); }} onClose={() => setDeleting(false)} />}
  </>;
}
