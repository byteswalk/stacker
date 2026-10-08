import { useMemo, useState } from "react";
import { AiExplain } from "../../ai/AiExplain";
import type { DirectoryNode } from "../types";
import { canSelectSafety, prepareCleanupPlan, setCleanupNodesSelected, useCleanupStore } from "../cleanupStore";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useBusyRead, useToast } from "../../../ui";
import { useDragPick } from "../../../dragPick";

export function formatSpaceBytes(bytes: number) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

const impactLabels: Record<string, string> = {
  "spaceAnalysis.impact.pythonVirtualEnvironment": "Python 虚拟环境，清理后需要重新创建并安装依赖",
  "spaceAnalysis.impact.nodeDependencies": "Node.js 依赖目录，清理后需要重新安装依赖",
  "spaceAnalysis.impact.rustBuildOutput": "Rust 构建产物，清理后首次构建会重新编译",
  "spaceAnalysis.impact.mavenBuildOutput": "Maven 构建产物，清理后首次构建会重新生成",
  "spaceAnalysis.impact.gradleProjectCache": "Gradle 项目缓存，清理后会重新下载或生成",
  "spaceAnalysis.impact.gradleBuildOutput": "Gradle 构建产物，清理后首次构建会重新生成",
  "spaceAnalysis.impact.goReleaseOutput": "Go 发布产物，清理后需要重新构建",
};

export function candidateImpact(node: Pick<DirectoryNode, "impactKey">) {
  return impactLabels[node.impactKey ?? ""] ?? "可重新生成的开发文件";
}

export function CandidateRows({ nodes, emptyText }: { nodes: DirectoryNode[]; emptyText?: string }) {
  const { tr } = useI18n();
  const toast = useToast();
  const cleanup = useCleanupStore();
  const [explain, setExplain] = useState<DirectoryNode | null>(null);
  const running = cleanup.progress?.state === "running";
  const pick = useDragPick(nodes.map((node) => node.safety === "viewOnly" || running ? [] : [node.nodeId]), (id) => cleanup.selected.has(id),
    (ids, on) => setCleanupNodesSelected(nodes.filter((node) => ids.includes(node.nodeId)), on));
  async function openDirectory(path: string) {
    try {
      await invoke("space_open_directory", { path });
    } catch {
      toast(tr("无法打开文件夹。请确认路径仍然存在，并且当前账户拥有访问权限。"), "err");
    }
  }
  if (cleanup.loading) return <div className="space-analysis-state"><i className="ti ti-loader spin" />{tr("正在读取可清理项…")}</div>;
  if (cleanup.error) return <div className="space-analysis-state error"><i className="ti ti-alert-triangle" />{tr("无法读取可清理项，请重新扫描。")}</div>;
  if (nodes.length === 0) return <div className="space-analysis-empty">{emptyText ?? tr("当前扫描结果没有此类可清理项。")}</div>;
  return <div className="space-cleanup-list">
    {nodes.map((node, i) => {
      const disabled = node.safety === "viewOnly" || running;
      const checked = cleanup.selected.has(node.nodeId);
      return <div className={`space-cleanup-row safety-${node.safety}${checked && !disabled ? " picked" : ""}`} key={node.nodeId} {...pick.row(i)}>
        <input type="checkbox" className="ck2" checked={checked && !disabled} disabled={disabled} title={tr("按住拖过几行可以一起勾选；按住 Shift 点选一段")}
          aria-label={`${tr("选择清理项")}: ${node.name}`} {...pick.box(i)} onChange={(e) => pick.change(i, e.target.checked)} />
        <span className="space-cleanup-icon"><i className="ti ti-folders" /></span>
        <div className="space-cleanup-copy">
          <strong title={node.name}>{node.name}</strong>
          <span title={node.path}>{node.path}</span>
          <small title={tr(candidateImpact(node))}>{tr(candidateImpact(node))}</small>
        </div>
        <div className="space-cleanup-meta">
          <b>{formatSpaceBytes(node.allocatedBytes)}</b>
          <span>{tr(node.safety === "safe" ? "安全清理" : node.safety === "rebuildable" ? "可重新生成" : node.safety === "needsConfirmation" ? "需要确认" : "仅供查看")}</span>
        </div>
        <button type="button" className="space-icon-button" title={tr("问问 AI：这是什么，删了会怎样")} aria-label={`${tr("问问 AI")}: ${node.name}`} onClick={() => setExplain(node)}>
          <i className="ti ti-sparkles" />
        </button>
        <button type="button" className="space-icon-button" title={tr("打开目录")} aria-label={`${tr("打开目录")}: ${node.name}`} onClick={() => void openDirectory(node.path)}>
          <i className="ti ti-folder-open" />
        </button>
      </div>;
    })}
    {explain && <AiExplain path={explain.path} bytes={explain.allocatedBytes} kind={tr(candidateImpact(explain))}
      onClose={() => setExplain(null)} />}
  </div>;
}

export function SelectionActions({ nodes }: { nodes: DirectoryNode[] }) {
  const { tr } = useI18n();
  const cleanup = useCleanupStore();
  const selectable = nodes.filter((node) => canSelectSafety(node.safety));
  const selectedCount = selectable.filter((node) => cleanup.selected.has(node.nodeId)).length;
  const running = cleanup.progress?.state === "running";
  return <div className="space-selection-actions">
    <button type="button" className="gh sm" disabled={running || selectable.length === 0 || selectedCount === selectable.length}
      title={tr("选择本分类中的全部可清理项")} onClick={() => setCleanupNodesSelected(nodes, true)}>
      <i className="ti ti-checkbox" /> {tr("全选")}
    </button>
    <button type="button" className="gh sm" disabled={running || selectedCount === 0}
      title={tr("取消本分类中的全部选择")} onClick={() => setCleanupNodesSelected(nodes, false)}>
      <i className="ti ti-square" /> {tr("全部取消")}
    </button>
  </div>;
}

/** What is picked for cleaning (artifacts and caches share it) and the button that cleans it. */
export function CleanupAction() {
  const { tr } = useI18n();
  const toast = useToast();
  const read = useBusyRead();
  const cleanup = useCleanupStore();
  const picked = cleanup.candidates.filter((node) => cleanup.selected.has(node.nodeId));
  const bytes = picked.reduce((sum, node) => sum + node.allocatedBytes, 0);
  return <div className="space-cleanup-action float-bar">
    <span>{tr("已选择")} {picked.length} {tr("项")} · {formatSpaceBytes(bytes)}</span>
    <button className="pr sm" type="button" disabled={cleanup.loading || cleanup.planning || picked.length === 0 || cleanup.progress?.state === "running"}
      title={tr("核对所选项目后进入清理确认")}
      onClick={async () => {
        try {
          await read("正在准备清理", prepareCleanupPlan);
        } catch (error) {
          toast(`${tr("无法准备清理：")}${String(error)}`, "err");
        }
      }}><i className={`ti ${cleanup.planning ? "ti-loader spin" : "ti-eraser"}`} /> {tr(cleanup.planning ? "正在准备…" : "清理")}</button>
  </div>;
}

export function CleanupPathFilter({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const { tr } = useI18n();
  return <label className="space-cleanup-filter">
    <i className="ti ti-search" aria-hidden="true" />
    <input value={value} onChange={(event) => onChange(event.target.value)} placeholder={tr("筛选路径或目录名")} />
    {value && <button type="button" title={tr("清除筛选")} aria-label={tr("清除筛选")} onClick={() => onChange("")}>
      <i className="ti ti-x" aria-hidden="true" />
    </button>}
  </label>;
}

export function filterCleanupNodes(nodes: DirectoryNode[], query: string) {
  const keyword = query.trim().toLocaleLowerCase();
  if (!keyword) return nodes;
  return nodes.filter((node) => `${node.name}\n${node.path}`.toLocaleLowerCase().includes(keyword));
}

export function useFilteredCleanupNodes(nodes: DirectoryNode[], query: string) {
  return useMemo(() => filterCleanupNodes(nodes, query), [nodes, query]);
}

export function DevelopmentArtifacts({ nodes }: { nodes: DirectoryNode[] }) {
  const { tr } = useI18n();
  const [query, setQuery] = useState("");
  const filteredNodes = useFilteredCleanupNodes(nodes, query);
  return <>
    <div className="space-analysis-section-heading space-cleanup-heading">
      <div><strong>{tr("开发产物")}</strong><span>{tr("仅列出已识别项目中可重新生成的依赖、构建目录和发布产物。")}</span></div>
      <div className="space-cleanup-heading-actions"><CleanupPathFilter value={query} onChange={setQuery} /><SelectionActions nodes={filteredNodes} /></div>
    </div>
    {query && <div className="space-cleanup-filter-note">{tr("批量操作仅作用于当前筛选结果。")} {filteredNodes.length} / {nodes.length}</div>}
    <CandidateRows nodes={filteredNodes} emptyText={query ? tr("未找到匹配的可清理项。") : undefined} />
    {nodes.length > 0 && <CleanupAction />}
  </>;
}
