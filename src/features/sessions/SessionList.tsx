import { useState } from "react";
import { useI18n } from "../../i18n";
import { Select } from "../../Select";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { AGENT_LABEL, CLIENT_LABEL, STATUS_LABEL, formatAge, toggleSelection } from "./sessionsView";
import { PAGE_SIZE, type AgentName, type ClientTag, type ProjectRow, type Session, type SessionPage, type SessionQuery } from "./types";

/** Client tags each agent can produce (CodeBuddy runs from a terminal or an IDE). */
function clientsFor(agent: string): ClientTag[] {
  if (agent === "codex") return ["desktop", "terminal", "ide", "automation"];
  if (agent === "claude") return ["desktop", "terminal", "ide", "sdk"];
  if (agent === "codebuddy") return ["terminal", "ide"];
  if (agent === "mimo" || agent === "kimi") return ["terminal"];
  return ["desktop", "terminal", "ide", "automation", "sdk"];
}

type Props = {
  page: SessionPage;
  query: SessionQuery;
  projects: ProjectRow[];
  loading: boolean;
  selected: string[];
  onSelect: (ids: string[]) => void;
  onFilter: (patch: Partial<SessionQuery>) => void;
  onPage: (offset: number) => void;
  onOpen: (session: Session) => void;
  onFavorite: (ids: string[], favorite: boolean) => void;
  onDelete: () => void;
  onSummarize: () => void;
  onDistill: () => void;
};

export function SessionList({ page, query, projects, loading, selected, onSelect, onFilter, onPage, onOpen, onFavorite, onDelete, onSummarize, onDistill }: Props) {
  const { tr: t, locale } = useI18n();
  const [expanded, setExpanded] = useState<string | null>(null);
  const [now] = useState(() => Math.floor(Date.now() / 1000));
  const allPage = page.items.length > 0 && page.items.every((s) => selected.includes(s.id));
  const selectedBytes = page.items.filter((s) => selected.includes(s.id)).reduce((sum, s) => sum + s.bytes, 0);
  const selectedFavorite = page.items.filter((s) => selected.includes(s.id)).every((s) => s.favorite);
  // Cascade: only projects that have sessions of the chosen agent.
  const agentProjects = query.agent ? projects.filter((p) => p.agents.includes(query.agent as AgentName)) : projects;
  const clients = clientsFor(query.agent);
  const agents = page.agents ?? [];
  const allSessions = agents.reduce((sum, a) => sum + a.sessions, 0);
  const pickAgent = (agent: string) => onFilter({
    agent,
    project: !agent || !query.project || projects.some((p) => p.project.key === query.project && p.agents.includes(agent as AgentName)) ? query.project : "",
    client: clientsFor(agent).includes(query.client as ClientTag) ? query.client : "",
  });
  const togglePage = () => onSelect(allPage
    ? selected.filter((id) => !page.items.some((s) => s.id === id))
    : Array.from(new Set([...selected, ...page.items.map((s) => s.id)])));

  return <>
    <div className="session-filters">
      <label className="session-search"><i className="ti ti-search" /><input value={query.search} aria-label={t("搜索会话")} placeholder={t("搜索标题、项目或摘要")} onChange={(e) => onFilter({ search: e.target.value })} /></label>
      <label className="session-check"><input type="checkbox" checked={query.fullText} onChange={(e) => onFilter({ fullText: e.target.checked })} />{t("搜索原文")}</label>
      <Select value={query.agent} onChange={pickAgent} width={170} options={[
        { value: "", label: `${t("全部智能体")}${allSessions ? ` (${allSessions})` : ""}` },
        ...agents.map((a) => ({ value: a.agent, label: `${AGENT_LABEL[a.agent]} (${a.sessions})`, title: bytes(a.bytes) })),
      ]} />
      <Select value={query.project} onChange={(project) => onFilter({ project })} options={[{ value: "", label: t("全部项目") }, ...agentProjects.map((p) => ({ value: p.project.key, label: p.project.name, title: p.project.path }))]} />
      <Select value={query.status} onChange={(status) => onFilter({ status })} options={[{ value: "", label: t("全部状态") }, ...(["active", "archived", "orphaned", "discarded"] as const).map((value) => ({ value, label: t(STATUS_LABEL[value]) }))]} />
      <Select value={query.client} onChange={(client) => onFilter({ client })} options={[{ value: "", label: t("全部来源") }, ...clients.map((value) => ({ value, label: t(CLIENT_LABEL[value]) }))]} />
      <Select value={query.updatedAfter ? String(Math.round((now - query.updatedAfter) / 86400)) : ""} onChange={(days) => onFilter({ updatedAfter: days ? now - Number(days) * 86400 : 0 })} options={[{ value: "", label: t("全部时间") }, ...[7, 30, 90].map((days) => ({ value: String(days), label: `${t("最近")} ${days} ${t("天")}` }))]} />
      <Select value={query.sort} onChange={(sort) => onFilter({ sort: sort as SessionQuery["sort"] })} options={[{ value: "", label: t("最近活动优先") }, { value: "bytes", label: t("占用最大优先") }]} />
      <label className="session-check"><input type="checkbox" checked={query.favoritesOnly} onChange={(e) => onFilter({ favoritesOnly: e.target.checked })} />{t("仅收藏")}</label>
      <label className="session-check"><input type="checkbox" checked={query.includeAutomation} onChange={(e) => onFilter({ includeAutomation: e.target.checked })} />{t("包含自动化运行")}</label>
    </div>
    <div className="session-bulk">
      <label className="session-check"><input type="checkbox" checked={allPage} onChange={togglePage} />{t("本页")}</label>
      <button className="gh sm" disabled={loading || !page.total} onClick={() => onSelect([...page.ids])}>{t("选择全部结果")} ({page.total})</button>
      <span className="session-total">{page.total} {t("个会话")} · {bytes(page.totalBytes)}</span>
    </div>
    <div className="session-list" aria-busy={loading}>
      {!page.items.length ? <div className="session-empty"><i className="ti ti-messages-off" /><b>{t(loading ? "正在读取会话" : "没有符合筛选的会话")}</b></div>
        : page.items.map((s) => <div key={s.id} className="session-item">
          <div className={`session-row ${selected.includes(s.id) ? "selected" : ""}`}>
            <input type="checkbox" checked={selected.includes(s.id)} aria-label={s.title} onChange={() => onSelect(toggleSelection(selected, s.id))} />
            <button className={`session-favorite ${s.favorite ? "on" : ""}`} title={t(s.favorite ? "取消收藏" : "收藏")} aria-label={t(s.favorite ? "取消收藏" : "收藏")} onClick={() => onFavorite([s.id], !s.favorite)}><i className="ti ti-star" /></button>
            <button className="session-title" onClick={() => onOpen(s)}>
              <b title={s.title}>{s.pinned && <i className="ti ti-pin" />}{s.title}{s.summary && <i className={`ti ti-notes summary-mark ${s.summaryStale ? "stale" : ""}`} title={t(s.summaryStale ? "摘要已过期" : "已有摘要")} />}</b>
              <span title={s.project.path}>{s.project.name}{!s.project.exists && s.project.path ? ` · ${t("项目已删除")}` : ""}</span>
            </button>
            <span className="session-tags">
              <span className={`session-tag agent-${s.agent}`}>{AGENT_LABEL[s.agent]}</span>
              <span className="session-tag">{t(CLIENT_LABEL[s.client])}</span>
              {!!s.copies.length && <span className="session-tag" title={t("这个会话切换过工作目录，Claude 在其他 worktree 目录里另存了记录；占用已合并计算，删除时一起删除。")}>{t("副本")} {s.copies.length}</span>}
              {s.importedBy?.map((by) => <span key={by} className="session-tag" title={t("这条对话被导入过另一个智能体的库；只算一份，删除本条不会动那份副本。")}>{t("副本 · ")}{AGENT_LABEL[by]}{t(" 导入")}</span>)}
              {s.importedFrom && <span className="session-tag" title={t("这条对话是从别的智能体导入的，原件已不在磁盘上。")}>{t("导入自 ")}{AGENT_LABEL[s.importedFrom]}</span>}
              {!!s.children.length && <button className="session-tag link" aria-expanded={expanded === s.id} onClick={() => setExpanded(expanded === s.id ? null : s.id)}>{t("子任务")} {s.children.length}</button>}
            </span>
            <span className={`session-status ${s.status}`}>{t(STATUS_LABEL[s.status])}{s.parentMissing && <small>{t("父会话已不存在")}</small>}</span>
            <span className="session-time" title={new Date(s.updatedAt * 1000).toLocaleString(locale)}>{t(formatAge(s.updatedAt, now))}<small>{bytes(s.bytes)}</small></span>
          </div>
          {expanded === s.id && <div className="session-children">{s.children.map((c) => <div key={c.id}><i className="ti ti-corner-down-right" /><span title={c.title}>{c.title || c.id}</span><small>{bytes(c.bytes)}</small></div>)}</div>}
        </div>)}
    </div>
    <div className="session-pagination"><span>{loading ? t("读取中") : ""}</span>
      <button className="gh sm" disabled={!query.offset || loading} title={t("上一页")} aria-label={t("上一页")} onClick={() => onPage(Math.max(0, query.offset - PAGE_SIZE))}><i className="ti ti-chevron-left" /></button>
      <span>{Math.floor(query.offset / PAGE_SIZE) + 1} / {Math.max(1, Math.ceil(page.total / PAGE_SIZE))}</span>
      <button className="gh sm" disabled={query.offset + PAGE_SIZE >= page.total || loading} title={t("下一页")} aria-label={t("下一页")} onClick={() => onPage(query.offset + PAGE_SIZE)}><i className="ti ti-chevron-right" /></button>
    </div>
    {!!selected.length && <div className="session-actionbar" role="toolbar">
      <b>{t("已选")} {selected.length} {t("项")}</b>{selectedBytes > 0 && <span>{t("本页约")} {bytes(selectedBytes)}</span>}
      <button className="gh sm" onClick={() => onFavorite(selected, !selectedFavorite)}><i className="ti ti-star" />{t(selectedFavorite ? "取消收藏" : "收藏")}</button>
      <button className="gh sm" onClick={onSummarize}><i className="ti ti-sparkles" />{t("生成摘要")}</button>
      <button className="gh sm" onClick={onDistill}><i className="ti ti-bulb" />{t("提炼…")}</button>
      <button className="pr sm danger" onClick={onDelete}><i className="ti ti-trash" />{t("删除…")}</button>
      <button className="ic" title={t("清除选择")} aria-label={t("清除选择")} onClick={() => onSelect([])}><i className="ti ti-x" /></button>
    </div>}
  </>;
}
