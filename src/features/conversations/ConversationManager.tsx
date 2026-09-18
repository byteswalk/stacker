import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Select } from "../../Select";
import { Modal, useToast } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { SourceSettings } from "./SourceSettings";
import { errorMessage } from "./errors";
import { EMPTY_LIST, EMPTY_QUERY, currentSelection, isSummaryStale, toggleSelection, type Conversation, type Detail, type Job, type Listing, type Preview, type Query, type Settings, type SummaryApproval } from "./types";
import "./conversations.css";

let cached = EMPTY_LIST;
let lastQuery = EMPTY_QUERY;
let lastTab = "sessions";

export function ConversationManager({ onCleanup, advanced }: { onCleanup: () => void; advanced: React.ReactNode }) {
  const { tr: t, locale } = useI18n();
  const toast = useToast();
  const [tab, setTab] = useState(lastTab);
  const [today] = useState(() => Math.floor(Date.now() / 86400000) * 86400);
  const [query, setQuery] = useState<Query>(lastQuery);
  const [list, setList] = useState<Listing>(cached);
  const [selected, setSelected] = useState<string[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [job, setJob] = useState<Job | null>(null);
  const [jobOpen, setJobOpen] = useState(false);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [detailOffset, setDetailOffset] = useState(0);
  const [detailTab, setDetailTab] = useState("original");
  const [loading, setLoading] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [summary, setSummary] = useState<SummaryApproval | null>(null);
  const [confirm, setConfirm] = useState("");
  const [group, setGroup] = useState("");
  const [groupOpen, setGroupOpen] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [warningsOpen, setWarningsOpen] = useState(false);
  const mounted = useRef(true);
  const request = useRef(0);
  const detailRequest = useRef(0);
  const busy = loading || pending || job?.state === "running";
  const queryRef = useRef(query);
  const seenJob = useRef("");
  const autoScan = useRef(false);
  useEffect(() => { lastTab = tab; }, [tab]);

  const load = useCallback(async () => {
    const generation = ++request.current;
    setLoading(true);
    try {
      const result = await invoke<Listing>("conversations_list", { query: queryRef.current });
      if (!mounted.current || generation !== request.current) return;
      cached = result; setList(result); setSelected((old) => currentSelection(old, result.ids)); setError("");
      if (!result.synced_at && !autoScan.current) {
        autoScan.current = true;
        const current = await invoke<Job>("conversations_job");
        if (current.state !== "running") await invoke("conversations_start", { action: "scan", ids: [], destination: null, locale: "zh-CN", approval: null });
      }
    } catch (e) { if (mounted.current && generation === request.current) setError(errorMessage(e)); }
    finally { if (mounted.current && generation === request.current) setLoading(false); }
  }, []);
  const loadSettings = useCallback(async () => { setSettings(await invoke<Settings>("conversations_sources")); }, []);

  useEffect(() => {
    queryRef.current = query; lastQuery = query;
    const timer = window.setTimeout(() => void load(), 300);
    return () => clearTimeout(timer);
  }, [query, load]);
  useEffect(() => {
    mounted.current = true;
    void loadSettings().catch((e) => setError(errorMessage(e)));
    let polling = false;
    const poll = async () => {
      if (polling) return; polling = true;
      try {
        const next = await invoke<Job>("conversations_job");
        if (!mounted.current) return;
        setJob(next);
        const key = `${next.id}:${next.state}`;
        if (seenJob.current !== key) {
          seenJob.current = key;
          if (next.id && next.state !== "running") void load();
        }
      } catch (e) { if (mounted.current) setError(errorMessage(e)); }
      finally { polling = false; }
    };
    void poll(); const timer = window.setInterval(() => void poll(), 1500);
    return () => { mounted.current = false; clearInterval(timer); };
  }, [load, loadSettings]);

  function filter(patch: Partial<Query>) { ++request.current; setLoading(true); setQuery((old) => ({ ...old, ...patch, offset: 0 })); setSelected([]); }
  function navigate(value: string) { setTab(value); setDetail(null); }
  async function run(task: () => Promise<unknown>) {
    setPending(true); setError("");
    try { await task(); } catch (e) { setError(errorMessage(e)); }
    finally { if (mounted.current) setPending(false); }
  }
  async function start(action: string, ids = selected, approval: string | null = null) {
    let destination: string | null = null;
    if (["export", "handoff"].includes(action)) {
      const result = await open({ directory: true, multiple: false });
      if (typeof result !== "string") return; destination = result;
    }
    await invoke("conversations_start", { action, ids, destination, locale, approval });
    setJob(await invoke<Job>("conversations_job"));
  }
  async function annotate(field: string, value: string, ids = selected) { await invoke("conversations_annotate", { ids, field, value }); await load(); }
  async function show(c: Conversation, offset = 0) {
    const generation = ++detailRequest.current;
    const next = await invoke<Detail>("conversations_read", { id: c.id, offset });
    if (generation === detailRequest.current && mounted.current) { setDetail(next); setDetailOffset(offset); }
  }
  async function prepare(action: string) {
    setConfirm("");
    if (action === "summarize") setSummary(await invoke<SummaryApproval>("conversations_prepare_summary", { ids: selected, locale }));
    else setPreview(await invoke<Preview>("conversations_preview", { ids: selected, action }));
  }
  const status = (value: string) => t(({ running: "执行中", completed: "已完成", failed: "失败", partial: "部分完成", interrupted: "已中断", cancelled: "已取消", blocked: "已阻止" } as Record<string, string>)[value] ?? value);
  const actionName = (value: string) => t(({ scan: "刷新索引", export: "导出原文", summarize: "总结", handoff: "导出交接资料", archive: "归档原会话", unarchive: "恢复原会话", delete: "删除原会话" } as Record<string, string>)[value] ?? value);
  const date = (value: number) => new Date(value * 1000).toLocaleString(locale);
  const sourceName = (c: Conversation) => settings?.sources.find((s) => s.id === c.source_id)?.name ?? c.client;
  const allPage = list.items.length > 0 && list.items.every((c) => selected.includes(c.id));

  return <div className="conversation-manager">
    <div className="conversation-heading">
      <div className="space-analysis-tabs" role="tablist">
        {[["sessions", "会话管理", "ti-messages"], ["knowledge", "项目资料", "ti-notebook"], ["space", "关联空间", "ti-database"]].map(([value, label, icon]) => <button key={value} role="tab" aria-selected={tab === value} className={tab === value ? "active" : ""} onClick={() => navigate(value)}><i className={`ti ${icon}`} />{t(label)}</button>)}
      </div>
      <div className="conversation-actions">
        <button className="gh sm" disabled={busy} onClick={() => setSettingsOpen(true)}><i className="ti ti-settings" />{t("数据来源")}</button>
        <button className="pr sm" disabled={busy} onClick={() => void run(() => start("scan", []))}><i className={`ti ${job?.action === "scan" && busy ? "ti-loader spin" : "ti-refresh"}`} />{t("刷新索引")}</button>
      </div>
    </div>
    <div className="conversation-sync"><span>{t("本机已索引")} <b>{list.indexed}</b> · {list.synced_at ? new Date(list.synced_at).toLocaleString(locale) : t("尚未扫描")}</span>
      {!!list.warnings.length && <button className="gh sm" onClick={() => setWarningsOpen(true)}><i className="ti ti-alert-triangle" />{t("未完整读取")} {list.warnings.length}</button>}
    </div>
    {error && <div role="alert" className="conversation-error">{t(error)}<button className="gh sm" onClick={() => void load()}><i className="ti ti-refresh" />{t("重试")}</button></div>}
    {!!job?.id && <div className={`conversation-job ${job.state === "running" ? "running" : ""}`} role="status">
      <i className={`ti ${job.state === "running" ? "ti-loader spin" : "ti-list-check"}`} /><b>{actionName(job.action)}</b><span>{status(job.state)} · {job.done} / {job.total}</span>
      {job.state === "running" && <progress max={Math.max(1, job.total)} value={job.done} />}
      <button className="gh sm" onClick={() => setJobOpen(true)}>{t("任务结果")}</button>
      {job.state === "running" && <button className="gh sm" onClick={() => void run(() => invoke("conversations_cancel"))}>{t("取消后续项目")}</button>}
    </div>}
    {tab !== "space" ? <>
      <div className="conversation-filters">
        <label className="conversation-search"><i className="ti ti-search" /><input value={query.search} aria-label={t("搜索会话")} placeholder={t("搜索标题、项目或摘要")} onChange={(e) => filter({ search: e.target.value })} /></label>
        <Select value={query.source} onChange={(source) => filter({ source })} options={[{ value: "", label: t("全部智能体") }, ...(settings?.sources ?? []).map((s) => ({ value: s.id, label: s.name }))]} />
        <Select value={query.project} onChange={(project) => filter({ project })} options={[{ value: "", label: t("全部项目") }, ...list.projects.map((p) => ({ value: p, label: p.split(/[\\/]/).pop() || p, title: p }))]} />
        <Select value={query.state} onChange={(state) => filter({ state })} options={[["", "全部状态"], ["active", "未归档"], ["archived", "已归档"], ["favorite", "收藏"], ["unsummarized", "待总结"], ["hidden", "在 Stacker 隐藏"]].map(([value, label]) => ({ value, label: t(label) }))} />
        <Select value={String(query.before)} onChange={(value) => filter({ before: Number(value) })} options={[{ value: "0", label: t("全部时间") }, ...[7, 30, 90].map((days) => ({ value: String(today - days * 86400), label: `${days} ${t("天以前")}` }))]} />
        <label className="conversation-check"><input type="checkbox" checked={query.full_text} onChange={(e) => filter({ full_text: e.target.checked })} />{t("搜索原文")}</label>
      </div>
      <div className="conversation-bulk">
        <label className="conversation-check"><input type="checkbox" checked={allPage} onChange={() => setSelected(allPage ? selected.filter((id) => !list.items.some((c) => c.id === id)) : Array.from(new Set([...selected, ...list.items.map((c) => c.id)])))} />{t("本页")}</label>
        <button className="gh sm" onClick={() => setSelected([...list.ids])} disabled={busy || !list.total}>{t("选择全部结果")} ({list.total})</button>
        <span>{t("已选")} {selected.length}</span>
        {!!selected.length && <button className="ic" title={t("清除选择")} aria-label={t("清除选择")} onClick={() => setSelected([])}><i className="ti ti-x" /></button>}
        <div className="conversation-actions">
          <button className="gh sm" disabled={busy || !selected.length} onClick={() => void run(() => prepare("summarize"))}><i className="ti ti-sparkles" />{t("总结")}</button>
          <button className="gh sm" disabled={busy || !selected.length} onClick={() => setConfirm(tab === "knowledge" ? "handoff" : "export")}><i className="ti ti-download" />{t(tab === "knowledge" ? "导出交接资料" : "导出原文")}</button>
          <button className="gh sm" disabled={busy || !selected.length} onClick={() => { setGroup(""); setGroupOpen(true); }}><i className="ti ti-folders" />{t("项目归属")}</button>
          <Select value="" disabled={busy || !selected.length} placeholder={t("更多操作")} options={[
            { value: "archive", label: t("归档原会话") }, { value: "unarchive", label: t("恢复原会话") }, { value: "delete", label: t("删除原会话") },
            { value: "hide", label: t(query.state === "hidden" ? "取消隐藏" : "在 Stacker 隐藏") },
          ]} onChange={(action) => { if (action === "hide") void run(() => annotate("hidden", query.state === "hidden" ? "0" : "1")); else void run(() => prepare(action)); }} />
        </div>
      </div>
      <div className="conversation-list" aria-busy={loading}>
        {!list.items.length ? <div className="conversation-empty"><i className="ti ti-messages-off" /><b>{t(loading ? "正在读取会话" : list.indexed ? "没有符合筛选的会话" : "尚未建立本机会话索引")}</b><button className="gh sm" disabled={busy} onClick={() => list.indexed ? filter(EMPTY_QUERY) : void run(() => start("scan", []))}>{t(list.indexed ? "清除筛选" : "扫描本机会话")}</button></div> : list.items.map((c) => <div className={`conversation-row ${selected.includes(c.id) ? "selected" : ""}`} key={c.id}>
          <input type="checkbox" checked={selected.includes(c.id)} aria-label={c.title} onChange={() => setSelected((old) => toggleSelection(old, c.id))} />
          <button className={`conversation-favorite ${c.favorite ? "active" : ""}`} title={t(c.favorite ? "取消收藏" : "收藏")} aria-label={t(c.favorite ? "取消收藏" : "收藏")} onClick={() => void run(() => annotate("favorite", c.favorite ? "0" : "1", [c.id]))}><i className={`ti ${c.favorite ? "ti-star-filled" : "ti-star"}`} /></button>
          <button className="conversation-title" onClick={() => { setDetailTab(tab === "knowledge" ? "summary" : "original"); void run(() => show(c)); }}>
            <b title={c.title}>{c.title}</b><span title={c.project}>{c.group_name || c.project || t("未归属项目")}</span>
            {tab === "knowledge" && <small>{c.summary ? c.summary.slice(0, 160) : t("尚未总结")}</small>}
          </button>
          <span className="conversation-source-label" title={c.client}>{sourceName(c)}</span>
          <span className="conversation-row-status">{t(c.archived ? "已归档" : "未归档")}<small>{t(!c.complete ? "部分内容" : isSummaryStale(c) ? "摘要待更新" : c.summary ? "已总结" : "待总结")}</small></span>
          <span className="conversation-time" title={date(c.modified)}>{new Date(c.modified * 1000).toLocaleDateString(locale)}<small>{bytes(c.bytes)}</small></span>
        </div>)}
      </div>
      <div className="conversation-pagination"><span>{loading ? t("读取中") : `${list.total} ${t("条会话")}`}</span>
        <button className="gh sm" disabled={!query.offset || loading} title={t("上一页")} aria-label={t("上一页")} onClick={() => setQuery((q) => ({ ...q, offset: Math.max(0, q.offset - 40) }))}><i className="ti ti-chevron-left" /></button>
        <span>{Math.floor(query.offset / 40) + 1} / {Math.max(1, Math.ceil(list.total / 40))}</span>
        <button className="gh sm" disabled={query.offset + 40 >= list.total || loading} title={t("下一页")} aria-label={t("下一页")} onClick={() => setQuery((q) => ({ ...q, offset: q.offset + 40 }))}><i className="ti ti-chevron-right" /></button>
      </div>
    </> : <div className="conversation-space">
      <div className="conversation-heading"><h3>{t("会话与项目空间")}</h3><button className="pr sm" onClick={onCleanup}><i className="ti ti-device-desktop-analytics" />{t("检查磁盘空间")}</button></div>
      <p className="conversation-note">{t("会话记录不等于构建缓存。删除聊天不会删除项目、工作树或 target；可回收空间需另行扫描核对。")}</p>
      <div className="conversation-space-row"><i className="ti ti-messages" /><b>{t("本机会话索引")}</b><span>{list.indexed} {t("条会话")}</span></div>
      <div className="conversation-space-row"><i className="ti ti-folders" /><b>{t("关联项目")}</b><span>{list.projects.length}</span></div>
      {list.projects.map((project) => <button className="conversation-project" key={project} title={project} onClick={() => { filter({ project }); navigate("sessions"); }}><i className="ti ti-folder" /><span>{project}</span><i className="ti ti-chevron-right" /></button>)}
      <button className="gh sm" aria-expanded={advancedOpen} onClick={() => setAdvancedOpen(!advancedOpen)}><i className={`ti ${advancedOpen ? "ti-chevron-up" : "ti-chevron-down"}`} />{t("高级：环境约束与空间跟踪记录")}</button>
      {advancedOpen && advanced}
    </div>}
    {settingsOpen && settings && <SourceSettings settings={settings} onClose={() => setSettingsOpen(false)} onSaved={() => void run(async () => { await loadSettings(); await start("scan", []); })} />}
    {detail && <Modal wide title={detail.conversation.title} onClose={() => { ++detailRequest.current; setDetail(null); }} footer={<>
      <button className="gh sm" onClick={() => void run(() => invoke("conversations_open", { id: detail.conversation.id, target: "project" }))}><i className="ti ti-folder" />{t("打开项目")}</button>
      <button className="gh sm" onClick={() => void run(() => invoke("conversations_open", { id: detail.conversation.id, target: "file" }))}><i className="ti ti-file" />{t("打开原文目录")}</button>
      {settings?.sources.find((s) => s.id === detail.conversation.source_id)?.kind === "codex" && <button className="pr sm" onClick={() => void run(() => invoke("conversations_open", { id: detail.conversation.id, target: "native" }))}>{t("在 Codex 打开")}</button>}
    </>}>
      <div className="conversation-detail-meta"><span>{detail.conversation.client}</span><span>{bytes(detail.conversation.bytes)}</span><code title={detail.conversation.path}>{detail.conversation.path}</code></div>
      {!detail.conversation.complete && <p className="conversation-error">{t(errorMessage(detail.conversation.warning || "E_PARTIAL"))}</p>}
      <div className="space-analysis-tabs"><button className={detailTab === "original" ? "active" : ""} onClick={() => setDetailTab("original")}>{t("原文")}</button><button className={detailTab === "summary" ? "active" : ""} onClick={() => setDetailTab("summary")}>{t("摘要与交接")}</button></div>
      {detailTab === "original" ? <><div className="conversation-transcript" translate="no">{detail.messages.map((m) => <article key={m.line} id={`message-${m.line}`}><header><b>{m.role}</b><code>L{m.line}</code></header><pre>{m.text}</pre></article>)}</div><div className="conversation-pagination">
        <button className="gh sm" disabled={!detailOffset || pending} onClick={() => void run(() => show(detail.conversation, Math.max(0, detailOffset - 60)))}>{t("上一页")}</button><span>{detailOffset + 1} / {detail.total}</span><button className="gh sm" disabled={detailOffset + 60 >= detail.total || pending} onClick={() => void run(() => show(detail.conversation, detailOffset + 60))}>{t("下一页")}</button>
      </div></> : <><p className="conversation-note">{t("摘要是模型生成的资料，不代表代码已验证；请根据行号核对原文。")}</p>{isSummaryStale(detail.conversation) && <p className="conversation-error">{t("原会话已变化，摘要待更新。")}</p>}<pre className="conversation-summary" translate="no">{detail.conversation.summary || t("尚未总结")}</pre><button className="gh sm" disabled={!detail.conversation.summary} onClick={() => void run(async () => { await navigator.clipboard.writeText(detail.conversation.summary); toast(t("已复制")); })}><i className="ti ti-copy" />{t("复制摘要")}</button></>}
    </Modal>}
    {preview && <Modal title={actionName(preview.action)} wide onClose={pending ? undefined : () => setPreview(null)} footer={<><button className="gh sm" onClick={() => setPreview(null)}>{t("取消")}</button><button className="pr sm" disabled={busy || !preview.selected.length || confirm !== "confirm"} onClick={() => void run(async () => { await invoke("conversations_execute", { token: preview.token }); setPreview(null); setJob(await invoke<Job>("conversations_job")); })}>{t("执行已核对的项目")}</button></>}>
      <p>{t("包含派生会话的实际影响范围")} <b>{preview.affected.length}</b> · {bytes(preview.bytes)}</p>
      <p className="conversation-note">{t("原始记录会先备份到 Stacker。备份占用磁盘空间，能恢复阅读内容，不保证恢复官方客户端状态。不会删除源码、工作树或构建缓存。")}</p>
      <div className="conversation-preview-list">{preview.affected.map((c) => <div key={c.id}><b>{c.title}</b><code title={c.path}>{c.path}</code></div>)}{preview.blocked.map((item) => <div key={item.id} className="conversation-error"><b>{item.title}</b><span>{t(errorMessage(item.detail))}</span></div>)}</div>
      <label className="conversation-check"><input type="checkbox" checked={confirm === "confirm"} onChange={(e) => setConfirm(e.target.checked ? "confirm" : "")} />{t("我已核对影响范围并同意操作原会话")}</label>
    </Modal>}
    {summary && <Modal title={t("确认总结发送范围")} wide onClose={pending ? undefined : () => setSummary(null)} footer={<><button className="gh sm" onClick={() => setSummary(null)}>{t("取消")}</button><button className="pr sm" disabled={busy || confirm !== "send"} onClick={() => void run(async () => { await start("summarize", summary.items.map((i) => i.id), summary.token); setSummary(null); })}>{t("发送并总结")}</button></>}>
      <p><b>{summary.model}</b> · {summary.endpoint}</p><p className="conversation-note">{t("以下是实际发送内容。自动脱敏不能覆盖所有秘密；不同意发送时可取消，改用本机模型。每条会话独立总结，不续写原聊天。")}</p>
      <div className="conversation-preview-list">{summary.items.map((item) => <details key={item.id}><summary>{item.title} · {item.chars} {t("字符")}</summary><pre translate="no">{item.text}</pre></details>)}</div>
      <label className="conversation-check"><input type="checkbox" checked={confirm === "send"} onChange={(e) => setConfirm(e.target.checked ? "send" : "")} />{t("允许将以上内容发送至该模型服务")}</label>
    </Modal>}
    {["export", "handoff"].includes(confirm) && <Modal title={actionName(confirm)} onClose={() => setConfirm("")} footer={<><button className="gh sm" onClick={() => setConfirm("")}>{t("取消")}</button><button className="pr sm" disabled={busy} onClick={() => void run(async () => { const action = confirm; setConfirm(""); await start(action); })}>{t("选择保存目录")}</button></>}><p>{t("导出文件可能包含原始代码、路径和敏感内容，请选择可信的本地目录。导出不删除原会话。")}</p><p>{selected.length} {t("条会话")}</p></Modal>}
    {groupOpen && <Modal title={t("项目归属")} onClose={() => setGroupOpen(false)} footer={<button className="pr sm" disabled={busy} onClick={() => void run(async () => { await annotate("group_name", group.trim()); setGroupOpen(false); })}>{t("保存")}</button>}><input className="ip full" aria-label={t("项目分组名称")} value={group} onChange={(e) => setGroup(e.target.value)} /><p className="conversation-note">{t("仅修改 Stacker 中的分组。留空恢复按原工作目录分组，不修改原客户端或源码目录。")}</p></Modal>}
    {jobOpen && job && <Modal wide title={t("任务结果")} onClose={() => setJobOpen(false)} footer={<button className="gh sm" disabled={busy || !job.items.some((i) => i.status === "failed")} onClick={() => { const failed = job.items.filter((i) => i.status === "failed").map((i) => i.id); setSelected(failed); setJobOpen(false); toast(t("失败项目已选中，请核对后重新执行。"), "info"); }}>{t("选择失败项目")}</button>}>
      <p>{actionName(job.action)} · {status(job.state)} · {job.done} / {job.total}</p>{job.error && <p className="conversation-error">{t(errorMessage(job.error))}</p>}{job.output && <p><b>{t(job.action === "scan" ? "已索引" : "输出目录")}</b> <code>{job.output}</code></p>}
      <div className="conversation-preview-list">{job.items.map((item, i) => <div key={`${item.id}:${i}`}><b>{item.title}</b><span>{status(item.status)} {t(errorMessage(item.detail))}</span></div>)}</div>
    </Modal>}
    {warningsOpen && <Modal wide title={t("未完整读取")} onClose={() => setWarningsOpen(false)}><div className="conversation-preview-list">{list.warnings.map((warning, i) => { const split = warning.lastIndexOf(": E_"); return <div key={i}><code>{split >= 0 ? warning.slice(0, split) : warning}</code><span>{split >= 0 ? t(errorMessage(warning.slice(split + 2))) : ""}</span></div>; })}</div></Modal>}
  </div>;
}
