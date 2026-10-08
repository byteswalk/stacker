import { useMemo, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useToast } from "../../../ui";
import { setInSet, useDragPick } from "../../../dragPick";
import type { DevelopmentProject, ProjectKind } from "../types";
import { CleanupPathFilter, formatSpaceBytes } from "./DevelopmentArtifacts";
import { RecycleBar, useProtectedPaths } from "./FileRemoval";
import { AiFindButton } from "../../ai/AiFind";

/** What AI search narrows the list to, beyond the words in the box. */
type Narrow = { kind: string; agent: string; idleDays: number; minGb: number; git: boolean };
const NO_NARROW: Narrow = { kind: "", agent: "", idleDays: 0, minGb: 0, git: false };
const DAY = 86_400_000;

const kindLabels: Record<ProjectKind, string> = {
  node: "Node.js", rust: "Rust", python: "Python", maven: "Maven",
  gradle: "Gradle", go: "Go", dotNet: ".NET",
};

const traceLabels: Record<string, string> = {
  agents: "AGENTS.md", codex: "Codex", claude: "Claude Code", cursor: "Cursor",
  gemini: "Antigravity", opencode: "OpenCode", qoder: "Qoder", trae: "TRAE", copilot: "GitHub Copilot",
};

function projectActivity(value: string | null, locale: string) {
  if (!value) return "--";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "--";
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

/**
 * The projects the scan found. Whole projects can be picked and removed here (to the Recycle
 * Bin unless asked otherwise); their rebuildable parts are cleaned on the artifact tab.
 */
export function DevelopmentProjects({ projects }: { projects: DevelopmentProject[] }) {
  const { locale, t, tr } = useI18n();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [narrow, setNarrow] = useState<Narrow>(NO_NARROW);
  // "Not changed for N days" counts from when the page opened.
  const [now] = useState(() => Date.now());
  const narrowed = narrow.kind !== "" || narrow.agent !== "" || narrow.idleDays > 0 || narrow.minGb > 0 || narrow.git;
  const [gone, setGone] = useState<Set<string>>(new Set());
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const filtered = useMemo(() => {
    const keyword = query.trim().toLocaleLowerCase();
    const present = projects.filter((project) => !gone.has(project.path)
      && (!narrow.kind || project.kinds.includes(narrow.kind as ProjectKind))
      && (!narrow.agent || project.agentTraces.includes(narrow.agent))
      && (!narrow.git || project.hasGitMetadata)
      && (!narrow.minGb || project.allocatedBytes >= narrow.minGb * 1024 ** 3)
      && (!narrow.idleDays || (!!project.lastModifiedAt && now - new Date(project.lastModifiedAt).getTime() >= narrow.idleDays * DAY)));
    if (!keyword) return present;
    return present.filter((project) => `${project.name}\n${project.path}\n${project.kinds.join(" ")}\n${project.agentTraces.join(" ")}`.toLocaleLowerCase().includes(keyword));
  }, [projects, query, gone, narrow, now]);
  const locked = useProtectedPaths(filtered.map((project) => project.path));
  const pick = useDragPick(filtered.map((project) => locked.has(project.path) ? [] : [project.path]), (path) => picked.has(path),
    (paths, on) => setPicked((old) => setInSet(old, paths, on)));
  const pickable = filtered.filter((project) => !locked.has(project.path));
  const allPicked = pickable.length > 0 && pickable.every((project) => picked.has(project.path));
  const chosen = filtered.filter((project) => picked.has(project.path)).map((project) => ({ path: project.path, bytes: project.allocatedBytes }));
  const removed = (paths: string[]) => {
    setGone((old) => setInSet(old, paths, true));
    setPicked((old) => setInSet(old, paths, false));
  };

  async function openDirectory(path: string) {
    try {
      await invoke("space_open_directory", { path });
    } catch {
      toast(t("space.projects.openFailed"), "err");
    }
  }

  if (projects.length === 0) return <div className="space-analysis-empty">{t("space.projects.empty")}</div>;

  return <>
    <div className="space-analysis-section-heading space-cleanup-heading">
      <div><strong>{t("space.projects.title")}</strong><span>{t("space.projects.description")}</span></div>
      <div className="space-cleanup-heading-actions">
        <CleanupPathFilter value={query} onChange={setQuery} />
        <AiFindButton list="开发项目" query={query} hint="先在筛选框里用一句话描述要找的项目，比如“半年没动过的 Node 项目”"
          fields={[
            { name: "search", meaning: "项目名或路径里的关键词", kind: "text" },
            { name: "kind", meaning: "项目类型", kind: "choice", options: Object.keys(kindLabels) },
            { name: "agent", meaning: "用过的 AI 工具留下的痕迹", kind: "choice", options: Object.keys(traceLabels) },
            { name: "idleDays", meaning: "至少多少天没有改动", kind: "number" },
            { name: "minGb", meaning: "总大小至少多少 GB", kind: "number" },
            { name: "git", meaning: "是否只要 Git 仓库", kind: "bool" },
          ]}
          onFilter={(found) => {
            setQuery(String(found.search ?? ""));
            setNarrow({ kind: String(found.kind ?? ""), agent: String(found.agent ?? ""), idleDays: Number(found.idleDays) || 0, minGb: Number(found.minGb) || 0, git: found.git === true });
          }} />
        <div className="space-selection-actions">
          <button type="button" className="gh sm" disabled={!pickable.length || allPicked}
            title={tr(query ? "全选筛选出的项目" : "选择全部可删除的项目")} onClick={() => setPicked(new Set(pickable.map((project) => project.path)))}>
            <i className="ti ti-checkbox" /> {tr("全选")}
          </button>
          <button type="button" className="gh sm" disabled={!chosen.length} title={tr("取消全部选择")} onClick={() => setPicked(new Set())}>
            <i className="ti ti-square" /> {tr("全部取消")}
          </button>
        </div>
      </div>
    </div>
    {narrowed && <div className="space-cleanup-filter-note">
      {tr("AI 筛选")}：{[
        narrow.kind && kindLabels[narrow.kind as ProjectKind],
        narrow.agent && (traceLabels[narrow.agent] ?? narrow.agent),
        narrow.idleDays > 0 && tr("{days} 天以上未改动").replace("{days}", String(narrow.idleDays)),
        narrow.minGb > 0 && tr("{size} GB 以上").replace("{size}", String(narrow.minGb)),
        narrow.git && "Git",
      ].filter(Boolean).join(" · ")}
      <button type="button" className="gh xs" onClick={() => setNarrow(NO_NARROW)}><i className="ti ti-x" /> {tr("清除")}</button>
    </div>}
    {(query || narrowed) && <div className="space-cleanup-filter-note">{tr("批量操作仅作用于当前筛选结果。")} {filtered.length} / {projects.length}</div>}
    <div className="space-project-list">
      {filtered.map((project, index) => {
        const lockedHere = locked.has(project.path);
        return <article className={`space-project-card${picked.has(project.path) ? " picked" : ""}`} key={project.projectId} {...pick.row(index)}>
          <div className="space-project-main">
            {lockedHere
              ? <span className="recycle-lock" title={tr("系统或程序目录里的项目不能在这里删除")}><i className="ti ti-lock" /></span>
              : <input type="checkbox" className="recycle-check" checked={picked.has(project.path)} aria-label={tr("选择")}
                title={tr("按住拖过几行可以一起勾选；按住 Shift 点选一段")} {...pick.box(index)} onChange={(event) => pick.change(index, event.target.checked)} />}
            <span className="space-project-icon"><i className="ti ti-code-dots" /></span>
            <div className="space-project-copy">
              <div className="space-project-title"><strong title={project.name}>{project.name}</strong>{project.hasGitMetadata && <span className="bd g">Git</span>}</div>
              <span className="space-project-path" title={project.path}>{project.path}</span>
              <div className="space-project-tags">
                {project.kinds.map((kind) => <span className="space-project-tag" key={kind}>{kindLabels[kind]}</span>)}
                {project.agentTraces.map((trace) => <span className="space-project-tag agent" key={trace} title={t("space.projects.agentTraceHint")}><i className="ti ti-sparkles" /> {traceLabels[trace] ?? trace}</span>)}
              </div>
            </div>
            <div className="space-project-metrics">
              <span><small>{t("space.projects.totalSize")}</small><b>{formatSpaceBytes(project.allocatedBytes)}</b></span>
              <span><small>{t("space.projects.reclaimable")}</small><b className={project.reclaimableBytes > 0 ? "accent" : ""}>{formatSpaceBytes(project.reclaimableBytes)}</b></span>
              <span><small>{t("space.projects.lastActivity")}</small><b>{projectActivity(project.lastModifiedAt, locale)}</b></span>
            </div>
            <div className="space-project-actions">
              <button type="button" className="gh sm" title={t("space.projects.openDirectory")} onClick={() => void openDirectory(project.path)}><i className="ti ti-folder-open" /></button>
            </div>
          </div>
        </article>;
      })}
    </div>
    {filtered.length > 0 && <RecycleBar folders files={chosen} onDone={removed} />}
    {filtered.length === 0 && <div className="space-analysis-empty compact">{t("space.projects.noMatch")}</div>}
  </>;
}
