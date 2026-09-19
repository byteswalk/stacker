import { useState } from "react";
import { useI18n } from "../../i18n";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { AGENT_LABEL, formatAge } from "./sessionsView";
import type { ProjectRow } from "./types";

export function ProjectList({ rows, loading, onPick, onHandoff }: { rows: ProjectRow[]; loading: boolean; onPick: (key: string) => void; onHandoff: (key: string) => void }) {
  const { tr: t } = useI18n();
  const [now] = useState(() => Math.floor(Date.now() / 1000));
  if (!rows.length) return <div className="session-empty"><i className="ti ti-folders" /><b>{t(loading ? "正在读取会话" : "没有项目")}</b></div>;
  return <div className="session-projects" role="table">
    <div className="session-project head" role="row">
      <span>{t("项目")}</span><span>{t("智能体")}</span><span>{t("会话")}</span><span>{t("孤儿")}</span><span>{t("占用")}</span><span>{t("最后活动")}</span><span />
    </div>
    {rows.map((row) => <div key={row.project.key || "none"} className="session-project" role="row">
      <button className="session-project-name" onClick={() => onPick(row.project.key)}><b>{row.project.name}{!row.project.exists && row.project.path && <em>{t("已删除")}</em>}</b><small title={row.project.path}>{row.project.path || t("未记录工作目录")}</small></button>
      <span>{row.agents.map((a) => AGENT_LABEL[a]).join(" · ")}</span>
      <span>{row.sessions}</span>
      <span className={row.orphans ? "warn" : ""}>{row.orphans}</span>
      <span>{bytes(row.bytes)}</span>
      <span>{t(formatAge(row.updatedAt, now))}</span>
      <button className="gh sm" title={t("用本机智能体把该项目的会话整理成交接资料")} onClick={() => onHandoff(row.project.key)}><i className="ti ti-file-text" />{t("交接")}</button>
    </div>)}
  </div>;
}
