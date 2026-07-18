import type { DirectoryNode } from "../types";
import { useI18n } from "../../../i18n";
import { CandidateRows, SelectionActions } from "./DevelopmentArtifacts";

export function CacheDownloads({ nodes }: { nodes: DirectoryNode[] }) {
  const { tr } = useI18n();
  return <>
    <div className="space-analysis-section-heading"><div><strong>{tr("缓存与下载")}</strong><span>{tr("清理后可能需要重新下载依赖；默认不勾选。")}</span></div><SelectionActions nodes={nodes} /></div>
    <CandidateRows nodes={nodes} />
  </>;
}
