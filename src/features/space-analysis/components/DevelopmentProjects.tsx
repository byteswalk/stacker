import { useMemo, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useToast } from "../../../ui";
import type { DevelopmentProject, DirectoryNode, ProjectKind } from "../types";
import { CandidateRows, CleanupPathFilter, SelectionActions, formatSpaceBytes } from "./DevelopmentArtifacts";

const kindLabels: Record<ProjectKind, string> = {
  node: "Node.js", rust: "Rust", python: "Python", maven: "Maven",
  gradle: "Gradle", go: "Go", dotNet: ".NET",
};

const traceLabels: Record<string, string> = {
  agents: "AGENTS.md", codex: "Codex", claude: "Claude Code", cursor: "Cursor",
  gemini: "Gemini", opencode: "OpenCode", qoder: "Qoder", trae: "TRAE", copilot: "GitHub Copilot",
};

function projectActivity(value: string | null, locale: string) {
  if (!value) return "--";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "--";
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

export function DevelopmentProjects({ projects, candidates }: { projects: DevelopmentProject[]; candidates: DirectoryNode[] }) {
  const { locale, t } = useI18n();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [expandedProjectId, setExpandedProjectId] = useState<string | null>(null);
  const filtered = useMemo(() => {
    const keyword = query.trim().toLocaleLowerCase();
    if (!keyword) return projects;
    return projects.filter((project) => `${project.name}\n${project.path}\n${project.kinds.join(" ")}\n${project.agentTraces.join(" ")}`.toLocaleLowerCase().includes(keyword));
  }, [projects, query]);

  async function openDirectory(path: string) {
    try {
      await invoke("space_open_directory", { path });
    } catch {
      toast(t("space.projects.openFailed"), "err");
    }
  }

  if (projects.length === 0) return <div className="space-analysis-empty">{t("space.projects.empty")}</div>;

  return <>
    <div className="space-analysis-section-heading space-project-heading">
      <div><strong>{t("space.projects.title")}</strong><span>{t("space.projects.description")}</span></div>
      <CleanupPathFilter value={query} onChange={setQuery} />
    </div>
    <div className="space-project-summary">
      <span>{t("space.projects.detected")} <b>{filtered.length}</b> / {projects.length}</span>
      <span>{t("space.projects.reclaimable")} <b>{formatSpaceBytes(filtered.reduce((sum, project) => sum + project.reclaimableBytes, 0))}</b></span>
    </div>
    <div className="space-project-list">
      {filtered.map((project) => {
        const projectCandidates = candidates.filter((candidate) => candidate.projectId === project.projectId);
        const expanded = expandedProjectId === project.projectId;
        return <article className={`space-project-card${expanded ? " expanded" : ""}`} key={project.projectId}>
          <div className="space-project-main">
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
              <button type="button" className="gh sm" disabled={projectCandidates.length === 0} onClick={() => setExpandedProjectId(expanded ? null : project.projectId)}>
                <i className={`ti ${expanded ? "ti-chevron-up" : "ti-list-check"}`} /> {t(projectCandidates.length === 0 ? "space.projects.noArtifacts" : "space.projects.reviewArtifacts")}
              </button>
            </div>
          </div>
          {expanded && <div className="space-project-artifacts">
            <div className="space-project-artifact-head"><span>{t("space.projects.verifiedArtifacts")}</span><SelectionActions nodes={projectCandidates} /></div>
            <CandidateRows nodes={projectCandidates} />
          </div>}
        </article>;
      })}
    </div>
    {filtered.length === 0 && <div className="space-analysis-empty compact">{t("space.projects.noMatch")}</div>}
  </>;
}
