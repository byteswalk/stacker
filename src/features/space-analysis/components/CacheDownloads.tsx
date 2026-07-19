import type { DirectoryNode } from "../types";
import { useState } from "react";
import { useI18n } from "../../../i18n";
import { CandidateRows, CleanupPathFilter, SelectionActions, useFilteredCleanupNodes } from "./DevelopmentArtifacts";

export function CacheDownloads({ nodes }: { nodes: DirectoryNode[] }) {
  const { tr } = useI18n();
  const [query, setQuery] = useState("");
  const filteredNodes = useFilteredCleanupNodes(nodes, query);
  return <>
    <div className="space-analysis-section-heading space-cleanup-heading">
      <div><strong>{tr("缓存与下载")}</strong><span>{tr("清理后可能需要重新下载依赖；默认不勾选。")}</span></div>
      <div className="space-cleanup-heading-actions"><CleanupPathFilter value={query} onChange={setQuery} /><SelectionActions nodes={filteredNodes} /></div>
    </div>
    {query && <div className="space-cleanup-filter-note">{tr("批量操作仅作用于当前筛选结果。")} {filteredNodes.length} / {nodes.length}</div>}
    <CandidateRows nodes={filteredNodes} emptyText={query ? tr("未找到匹配的可清理项。") : undefined} />
  </>;
}
