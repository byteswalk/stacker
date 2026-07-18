import type { DirectoryNode } from "../types";
import { toggleCleanupNode, useCleanupStore } from "../cleanupStore";
import { useI18n } from "../../../i18n";

export function formatSpaceBytes(bytes: number) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

const impactLabels: Record<string, string> = {
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

export function CandidateRows({ nodes }: { nodes: DirectoryNode[] }) {
  const { tr } = useI18n();
  const cleanup = useCleanupStore();
  if (cleanup.loading) return <div className="space-analysis-state"><i className="ti ti-loader spin" />{tr("正在读取可清理项…")}</div>;
  if (cleanup.error) return <div className="space-analysis-state error"><i className="ti ti-alert-triangle" />{tr("无法读取可清理项，请重新扫描。")}</div>;
  if (nodes.length === 0) return <div className="space-analysis-empty">{tr("当前扫描结果没有此类可清理项。")}</div>;
  return <div className="space-cleanup-list">
    {nodes.map((node) => {
      const disabled = node.safety === "viewOnly" || cleanup.progress?.state === "running";
      const checked = cleanup.selected.has(node.nodeId);
      return <div className={`space-cleanup-row safety-${node.safety}`} key={node.nodeId}>
        <input type="checkbox" className="ck2" checked={checked && !disabled} disabled={disabled}
          aria-label={`${tr("选择清理项")}: ${node.name}`} onChange={() => toggleCleanupNode(node)} />
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
      </div>;
    })}
  </div>;
}

export function DevelopmentArtifacts({ nodes }: { nodes: DirectoryNode[] }) {
  const { tr } = useI18n();
  return <>
    <div className="space-analysis-section-heading"><div><strong>{tr("开发产物")}</strong><span>{tr("仅列出已识别项目中可重新生成的依赖、构建目录和发布产物。")}</span></div></div>
    <CandidateRows nodes={nodes} />
  </>;
}
