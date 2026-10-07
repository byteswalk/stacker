import { useMemo, useState } from "react";
import { ProjectCleanupDialog } from "./ProjectCleanupDialog";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { AGENT_LABEL, formatAge } from "./sessionsView";
import type { AgentName, ProjectRow } from "./types";
import { useColumns, type Column } from "../../columns";
import { Loading } from "../../ui";

/** The table's tracks: every one with data can be resized; the actions keep the width of their buttons. */
const TRACKS: Column[] = [
  { key: "name", track: "minmax(200px,1fr)", resizable: true, min: 160 },
  { key: "agent", track: "120px", resizable: true, min: 80 },
  { key: "sessions", track: "60px", resizable: true, min: 44 },
  { key: "orphans", track: "60px", resizable: true, min: 44 },
  { key: "bytes", track: "80px", resizable: true, min: 60 },
  { key: "updatedAt", track: "80px", resizable: true, min: 60 },
  { key: "ops", track: "auto", grows: true },
];

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
  const columns = useColumns("stacker.projects.columns.v1", TRACKS);
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

  if (!rows.length) return loading
    ? <Loading title="正在读取项目" text="按会话里记下的工作目录归到各个项目，第一次要扫描各智能体的会话文件。可以先去别的页面，回来时结果还在。" />
    : <div className="session-empty"><i className="ti ti-folders" /><b>{t("没有项目")}</b></div>;
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
    <div className="session-projects" role="table" style={{ ["--project-cols" as string]: columns.template }}>
      <div className="session-project head" role="row" data-columns="">
        <span className="col-cell">{header("name", "项目")}{columns.handle("name")}</span><span className="col-cell">{t("智能体")}{columns.handle("agent")}</span>
        {COLUMNS.map((c) => <span key={c.key} className="col-cell">{header(c.key, c.label)}{columns.handle(c.key)}</span>)}
        <span />
      </div>
      {shown.map((row) => <div key={row.project.key || "none"} className="session-project" role="row">
        <button className="session-project-name" title={t("查看这个项目的会话")} onClick={() => onPick(row.project.key)}>
          <b>{row.project.name}{!row.project.exists && row.project.path && <em>{t("已删除")}</em>} <i className="ti ti-arrow-right" /></b>
          <small title={row.project.path}>{row.project.path || t("未记录工作目录")}</small>
        </button>
        <span>{row.agents.map((a) => AGENT_LABEL[a]).join(" · ")}</span>
        <span>{row.sessions}{!!row.discarded && <em className="dim" title={t("应用内已删除、转录还在磁盘上的对话")}> +{row.discarded}</em>}</span>
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
