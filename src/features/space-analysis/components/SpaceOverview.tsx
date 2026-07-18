import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useToast } from "../../../ui";
import { layoutTreemap } from "../treemap";
import type { AnalysisSummary, DirectoryNode, Paged } from "../types";

const TREEMAP_HEIGHT = 300;
const TREEMAP_COLORS = [
  "#397bbf",
  "#2f8f78",
  "#a87520",
  "#7b63b4",
  "#b45e5a",
  "#3f8998",
  "#747f45",
  "#98618b",
];

export function formatSpaceBytes(bytes: number) {
  if (bytes >= 1024 ** 4) return `${(bytes / 1024 ** 4).toFixed(1)} TB`;
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

function rootMap(nodes: readonly DirectoryNode[]) {
  return new Map(nodes.map((node) => [node.nodeId, node]));
}

export function SpaceOverview({
  taskId,
  summary,
  freeBytes,
}: {
  taskId: string;
  summary: AnalysisSummary;
  freeBytes: number | null;
}) {
  const { tr } = useI18n();
  const toast = useToast();
  const treemapRef = useRef<HTMLDivElement>(null);
  const requestGeneration = useRef(0);
  const [treemapWidth, setTreemapWidth] = useState(800);
  const [levels, setLevels] = useState<Array<{ node: DirectoryNode; nodes: DirectoryNode[] }>>([]);
  const [loadingNodeId, setLoadingNodeId] = useState<string | null>(null);
  const visibleNodes = levels.at(-1)?.nodes ?? summary.rootNodes;
  const nodesById = useMemo(() => rootMap(visibleNodes), [visibleNodes]);
  const rectangles = useMemo(() => layoutTreemap(
    visibleNodes.map((node) => ({ id: node.nodeId, value: node.allocatedBytes })),
    treemapWidth,
    TREEMAP_HEIGHT,
  ), [visibleNodes, treemapWidth]);

  useEffect(() => {
    requestGeneration.current += 1;
    setLevels([]);
    setLoadingNodeId(null);
  }, [taskId, summary]);

  useEffect(() => {
    const element = treemapRef.current;
    if (!element) return;
    const update = () => setTreemapWidth(Math.max(1, Math.round(element.getBoundingClientRect().width)));
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const metrics = [
    { label: tr("实际占用"), value: formatSpaceBytes(summary.allocatedBytes), icon: "ti-database" },
    { label: tr("逻辑大小"), value: formatSpaceBytes(summary.logicalBytes), icon: "ti-file-description" },
    {
      label: tr("可用空间"),
      value: freeBytes === null ? tr("不可用") : formatSpaceBytes(freeBytes),
      icon: "ti-chart-donut",
      title: freeBytes === null ? tr("目录扫描或磁盘信息已变化，无法可靠计算可用空间。") : undefined,
    },
    {
      label: tr("已跳过路径"),
      value: new Intl.NumberFormat().format(summary.skippedPaths),
      icon: "ti-alert-circle",
      title: tr("包括无权访问、扫描期间消失、无效或无法读取的路径；这些路径未计入占用统计。"),
    },
  ];

  async function drillInto(node: DirectoryNode) {
    if (node.childCount === 0 || loadingNodeId) return;
    const generation = ++requestGeneration.current;
    setLoadingNodeId(node.nodeId);
    try {
      const page = await invoke<Paged<DirectoryNode>>("space_scan_children", {
        taskId,
        parentId: node.nodeId,
        offset: 0,
        limit: 200,
      });
      if (generation !== requestGeneration.current) return;
      if (page.items.length === 0) {
        toast(tr("该目录没有可继续查看的子目录。"), "info");
        return;
      }
      setLevels((current) => [...current, { node, nodes: page.items }]);
    } catch {
      if (generation === requestGeneration.current) toast(tr("无法读取子目录，请重试。"), "err");
    } finally {
      if (generation === requestGeneration.current) setLoadingNodeId(null);
    }
  }

  function showLevel(index: number) {
    requestGeneration.current += 1;
    setLoadingNodeId(null);
    setLevels((current) => current.slice(0, index));
  }

  async function openDirectory(path: string) {
    try {
      await invoke("space_open_directory", { path });
    } catch {
      toast(tr("无法打开目录，请确认路径仍然存在。"), "err");
    }
  }

  return (
    <div className="space-overview">
      <div className="space-overview-metrics">
        {metrics.map((metric) => (
          <div className="space-overview-metric" key={metric.label} title={metric.title}>
            <i className={`ti ${metric.icon}`} aria-hidden="true" />
            <div>
              <span>{metric.label}</span>
              <strong>{metric.value}</strong>
            </div>
          </div>
        ))}
      </div>

      <div className="space-analysis-section-heading">
        <div>
          <strong>{tr("扫描范围占用")}</strong>
          <span>{tr("矩形面积按实际磁盘占用计算；单击下钻目录，右键直接打开目录。")}</span>
        </div>
        <span>{summary.directoryCount.toLocaleString()} {tr("个目录")} · {summary.fileCount.toLocaleString()} {tr("个文件")}</span>
      </div>

      {rectangles.length === 0 ? (
        <div className="space-analysis-empty">{tr("当前扫描结果没有可显示的占用数据。")}</div>
      ) : (
        <>
        <div className="space-treemap-breadcrumb" aria-label={tr("当前目录层级")}>
          <button type="button" title={tr("返回扫描范围")} onClick={() => showLevel(0)}>{tr("扫描范围")}</button>
          {levels.map((level, index) => <span key={level.node.nodeId}>
            <i className="ti ti-chevron-right" aria-hidden="true" />
            <button type="button" title={level.node.path} onClick={() => showLevel(index + 1)}>{level.node.name}</button>
          </span>)}
        </div>
        <div ref={treemapRef} className="space-treemap" style={{ height: TREEMAP_HEIGHT }} aria-busy={loadingNodeId !== null}>
          {rectangles.map((rectangle, index) => {
            const node = nodesById.get(rectangle.id);
            if (!node) return null;
            const areaRatio = (rectangle.width * rectangle.height) / (treemapWidth * TREEMAP_HEIGHT);
            const hideLabel = treemapWidth < 480 && areaRatio < 0.07;
            const title = `${node.path}\n${tr("实际占用")}: ${formatSpaceBytes(node.allocatedBytes)}\n${tr("逻辑大小")}: ${formatSpaceBytes(node.logicalBytes)}`;
            return (
              <button
                type="button"
                key={node.nodeId}
                className={`space-treemap-node${hideLabel ? " compact" : ""}`}
                title={title}
                disabled={loadingNodeId !== null}
                onClick={() => void drillInto(node)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  void openDirectory(node.path);
                }}
                style={{
                  left: rectangle.x,
                  top: rectangle.y,
                  width: rectangle.width,
                  height: rectangle.height,
                  background: TREEMAP_COLORS[index % TREEMAP_COLORS.length],
                }}
              >
                {!hideLabel && (
                  <>
                    <strong>{node.name}</strong>
                    <span>{formatSpaceBytes(node.allocatedBytes)}</span>
                  </>
                )}
              </button>
            );
          })}
          {loadingNodeId && <div className="space-treemap-loading"><i className="ti ti-loader spin" /> {tr("正在读取子目录…")}</div>}
        </div>
        </>
      )}
    </div>
  );
}
