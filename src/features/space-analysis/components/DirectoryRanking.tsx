import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useToast } from "../../../ui";
import type { DirectoryNode, Paged } from "../types";
import { formatSpaceBytes } from "./SpaceOverview";

const PAGE_SIZE = 100;

export interface DirectoryPageState {
  items: DirectoryNode[];
  total: number;
  nextOffset: number;
  loading: boolean;
  error: string | null;
}

export function mergeDirectoryPage(
  current: DirectoryPageState | undefined,
  page: Paged<DirectoryNode>,
): DirectoryPageState {
  const seen = new Set<string>();
  const items = [...(current?.items ?? []), ...page.items].filter((item) => {
    if (seen.has(item.nodeId)) return false;
    seen.add(item.nodeId);
    return true;
  });
  return {
    items,
    total: page.total,
    nextOffset: page.offset + page.items.length,
    loading: false,
    error: null,
  };
}

function sortedRoots(roots: readonly DirectoryNode[]) {
  return [...roots].sort((left, right) => (
    right.allocatedBytes - left.allocatedBytes
    || left.name.localeCompare(right.name)
    || left.nodeId.localeCompare(right.nodeId)
  ));
}

export function DirectoryRanking({ taskId, roots }: { taskId: string; roots: DirectoryNode[] }) {
  const { tr } = useI18n();
  const toast = useToast();
  const activeTask = useRef(taskId);
  const loadingNodes = useRef(new Set<string>());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [pages, setPages] = useState<Record<string, DirectoryPageState>>({});
  const orderedRoots = useMemo(() => sortedRoots(roots), [roots]);

  useEffect(() => {
    activeTask.current = taskId;
    loadingNodes.current.clear();
    setExpanded(new Set());
    setPages({});
  }, [taskId]);

  async function loadChildren(node: DirectoryNode, offset: number) {
    if (loadingNodes.current.has(node.nodeId)) return;
    loadingNodes.current.add(node.nodeId);
    const requestedTask = taskId;
    setPages((current) => ({
      ...current,
      [node.nodeId]: {
        items: current[node.nodeId]?.items ?? [],
        total: current[node.nodeId]?.total ?? node.childCount,
        nextOffset: current[node.nodeId]?.nextOffset ?? 0,
        loading: true,
        error: null,
      },
    }));
    try {
      const page = await invoke<Paged<DirectoryNode>>("space_scan_children", {
        taskId: requestedTask,
        parentId: node.nodeId,
        offset,
        limit: PAGE_SIZE,
      });
      if (activeTask.current !== requestedTask) return;
      setPages((current) => ({
        ...current,
        [node.nodeId]: mergeDirectoryPage(current[node.nodeId], page),
      }));
    } catch {
      if (activeTask.current !== requestedTask) return;
      setPages((current) => ({
        ...current,
        [node.nodeId]: {
          items: current[node.nodeId]?.items ?? [],
          total: current[node.nodeId]?.total ?? node.childCount,
          nextOffset: current[node.nodeId]?.nextOffset ?? offset,
          loading: false,
          error: tr("无法读取子目录，请重试。"),
        },
      }));
    } finally {
      loadingNodes.current.delete(node.nodeId);
    }
  }

  function toggle(node: DirectoryNode) {
    const willExpand = !expanded.has(node.nodeId);
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(node.nodeId)) next.delete(node.nodeId);
      else next.add(node.nodeId);
      return next;
    });
    if (willExpand && node.childCount > 0 && !pages[node.nodeId]) {
      void loadChildren(node, 0);
    }
  }

  async function openDirectory(path: string) {
    try {
      await invoke("space_open_directory", { path });
    } catch {
      toast(tr("无法打开目录，请确认路径仍然存在。"), "err");
    }
  }

  async function copyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      toast(tr("路径已复制"), "ok");
    } catch {
      toast(tr("复制路径失败，请重试。"), "err");
    }
  }

  function renderNode(node: DirectoryNode, depth: number): ReactNode {
    const isExpanded = expanded.has(node.nodeId);
    const page = pages[node.nodeId];
    const canLoadMore = page && page.nextOffset < page.total;
    return (
      <div className="space-directory-branch" key={node.nodeId}>
        <div className="space-directory-row" style={{ paddingLeft: 12 + depth * 20 }}>
          <button
            type="button"
            className="space-tree-toggle"
            disabled={node.childCount === 0}
            aria-label={isExpanded ? tr("收起目录") : tr("展开目录")}
            title={isExpanded ? tr("收起目录") : tr("展开目录")}
            onClick={() => toggle(node)}
          >
            <i className={`ti ${node.childCount === 0 ? "ti-point" : isExpanded ? "ti-chevron-down" : "ti-chevron-right"}`} />
          </button>
          <i className="ti ti-folder space-directory-icon" aria-hidden="true" />
          <div className="space-directory-main">
            <strong title={node.name}>{node.name}</strong>
            <span title={node.path}>{node.path}</span>
          </div>
          <div className="space-directory-size" title={`${tr("实际占用")}: ${formatSpaceBytes(node.allocatedBytes)} · ${tr("逻辑大小")}: ${formatSpaceBytes(node.logicalBytes)}`}>
            <strong>{tr("实际占用")} {formatSpaceBytes(node.allocatedBytes)}</strong>
            <span>{tr("逻辑大小")} {formatSpaceBytes(node.logicalBytes)}</span>
          </div>
          <div className="space-row-actions">
            <button type="button" className="space-icon-button" title={tr("打开目录")} aria-label={tr("打开目录")} onClick={() => void openDirectory(node.path)}>
              <i className="ti ti-folder-open" />
            </button>
            <button type="button" className="space-icon-button" title={tr("复制路径")} aria-label={tr("复制路径")} onClick={() => void copyPath(node.path)}>
              <i className="ti ti-copy" />
            </button>
          </div>
        </div>
        {isExpanded && (
          <div className="space-directory-children">
            {page?.items.map((child) => renderNode(child, depth + 1))}
            {page?.loading && <div className="space-directory-message"><i className="ti ti-loader spin" /> {tr("正在读取子目录…")}</div>}
            {page?.error && (
              <div className="space-directory-message error">
                <span>{page.error}</span>
                <button type="button" className="gh sm" onClick={() => void loadChildren(node, page.nextOffset)}>{tr("重试")}</button>
              </div>
            )}
            {canLoadMore && !page.loading && !page.error && (
              <button type="button" className="space-load-more" onClick={() => void loadChildren(node, page.nextOffset)}>
                <i className="ti ti-chevron-down" /> {tr("加载更多")} ({page.items.length}/{page.total})
              </button>
            )}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="space-directory-ranking">
      <div className="space-analysis-section-heading">
        <div>
          <strong>{tr("目录排行")}</strong>
          <span>{tr("按实际磁盘占用排序；展开时才读取下一层目录。")}</span>
        </div>
        <span>{tr("每页最多 100 项")}</span>
      </div>
      {orderedRoots.length === 0
        ? <div className="space-analysis-empty">{tr("当前扫描结果没有目录数据。")}</div>
        : <div className="space-directory-list">{orderedRoots.map((root) => renderNode(root, 0))}</div>}
    </div>
  );
}
