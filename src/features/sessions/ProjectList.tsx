import { useMemo, useState } from "react";
import { ProjectCleanupDialog } from "./ProjectCleanupDialog";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { AGENT_LABEL, formatAge } from "./sessionsView";
import type { AgentName, ProjectRow } from "./types";

export type SortKey = "bytes" | "sessions" | "orphans" | "updatedAt" | "name";

/** The project view answers "what is taking the space", so size leads until asked otherwise. */
export function sortProjects(rows: ProjectRow[], key: SortKey, ascending: boolean): ProjectRow[] {
  const value = (row: ProjectRow): number | string => {
    switch (key) {
      case "bytes": return row.bytes;
      case "sessions": return row.sessions;
      case "orphans": return row.orphans;
      case "updatedAt": return row.updatedAt;
      default: return row.project.name.toLowerCase();
    }
  };
  const sorted = [...rows].sort((a, b) => {
    const [x, y] = [value(a), value(b)];
    if (typeof x === "string" || typeof y === "string") return String(x).localeCompare(String(y), "zh");
    return x - y;
  });
  return ascending ? sorted : sorted.reverse();
}

/** Text match on the project's name and its folder. */
export function matchProject(row: ProjectRow, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return row.project.name.toLowerCase().includes(needle) || (row.project.path ?? "").toLowerCase().includes(needle);
}

const COLUMNS: { key: SortKey; label: string }[] = [
  { key: "sessions", label: "会话" },
  { key: "orphans", label: "孤儿" },
  { key: "bytes", label: "占用" },
  { key: "updatedAt", label: "最后活动" },
];

export function ProjectList({ rows, loading, onPick, onHandoff }: { rows: ProjectRow[]; loading: boolean; onPick: (key: string) => void; onHandoff: (key: string) => void }) {
  const { tr: t } = useI18n();
  const [now] = useState(() => Math.floor(Date.now() / 1000));
  const [query, setQuery] = useState("");
  const [agent, setAgent] = useState<AgentName | "">("");
  const [missingOnly, setMissingOnly] = useState(false);
  const [sort, setSort] = useState<{ key: SortKey; ascending: boolean }>({ key: "bytes", ascending: false });
  const [error, setError] = useState("");
  const [cleaning, setCleaning] = useState<ProjectRow | null>(null);

  const shown = useMemo(() => sortProjects(
    rows.filter((row) => matchProject(row, query)
      && (!agent || row.agents.includes(agent))
      && (!missingOnly || (!row.project.exists && !!row.project.path))),
    sort.key, sort.ascending,
  ), [rows, query, agent, missingOnly, sort]);

  // Only the agents these projects were actually worked on with.
  const present = useMemo(() => {
    const seen = new Set<AgentName>();
    rows.forEach((row) => row.agents.forEach((a) => seen.add(a)));
    return [...seen];
  }, [rows]);

  const openFolder = (path: string) => {
    invoke("space_open_directory", { path }).catch((e) => setError(String(e)));
  };

  const header = (key: SortKey, label: string) => <button className="session-sort" onClick={() =>
    setSort((old) => old.key === key ? { key, ascending: !old.ascending } : { key, ascending: key === "name" })}>
    {t(label)}{sort.key === key && <i className={"ti " + (sort.ascending ? "ti-chevron-up" : "ti-chevron-down")} />}
  </button>;

  if (!rows.length) return <div className="session-empty"><i className="ti ti-folders" /><b>{t(loading ? "正在读取会话" : "没有项目")}</b></div>;
  return <>
    <div className="session-projects-bar">
      <label className="session-search">
        <i className="ti ti-search" />
        <input value={query} placeholder={t("搜索项目名或路径…")} onChange={(e) => setQuery(e.target.value)} />
      </label>
      <div className="seg sm">
        <button className={agent === "" ? "on" : ""} onClick={() => setAgent("")}>{t("全部")}</button>
        {present.map((a) => <button key={a} className={agent === a ? "on" : ""} onClick={() => setAgent(a)}>{AGENT_LABEL[a]}</button>)}
      </div>
      <label className="ck"><input type="checkbox" checked={missingOnly} onChange={(e) => setMissingOnly(e.target.checked)} /> {t("只看目录已删除")}</label>
      <span className="s dim">{shown.length} / {rows.length} {t("个项目")}</span>
    </div>
    {error && <p role="alert" className="session-error">{t(error)}</p>}
    <div className="session-projects" role="table">
      <div className="session-project head" role="row">
        <span>{header("name", "项目")}</span><span>{t("智能体")}</span>
        {COLUMNS.map((c) => <span key={c.key}>{header(c.key, c.label)}</span>)}
        <span />
      </div>
      {shown.map((row) => <div key={row.project.key || "none"} className="session-project" role="row">
        <button className="session-project-name" title={t("查看这个项目的会话")} onClick={() => onPick(row.project.key)}>
          <b>{row.project.name}{!row.project.exists && row.project.path && <em>{t("已删除")}</em>} <i className="ti ti-arrow-right" /></b>
          <small title={row.project.path}>{row.project.path || t("未记录工作目录")}</small>
        </button>
        <span>{row.agents.map((a) => AGENT_LABEL[a]).join(" · ")}</span>
        <span>{row.sessions}</span>
        <span className={row.orphans ? "warn" : ""}>{row.orphans}</span>
        <span>{bytes(row.bytes)}</span>
        <span>{t(formatAge(row.updatedAt, now))}</span>
        <span className="session-project-acts">
          <button className="gh xs" title={t("打开项目目录")} disabled={!row.project.exists || !row.project.path} onClick={() => openFolder(row.project.path)}><i className="ti ti-folder-open" /></button>
          <button className="gh sm" title={t("分析项目目录里可以重建的部分：依赖、构建产物、缓存和旧的发布包")} disabled={!row.project.exists || !row.project.path} onClick={() => setCleaning(row)}><i className="ti ti-recycle" />{t("清理")}</button>
          <button className="gh sm" title={t("用本机智能体把该项目的会话整理成交接资料")} onClick={() => onHandoff(row.project.key)}><i className="ti ti-file-text" />{t("交接")}</button>
        </span>
      </div>)}
      {!shown.length && <div className="session-empty"><i className="ti ti-search-off" /><b>{t("没有匹配的项目")}</b></div>}
    </div>
    {cleaning && <ProjectCleanupDialog name={cleaning.project.name} path={cleaning.project.path} onClose={() => setCleaning(null)} />}
  </>;
}
