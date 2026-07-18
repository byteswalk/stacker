import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useToast } from "../../../ui";
import type { LargeFileRow, Paged } from "../types";
import { formatSpaceBytes } from "./SpaceOverview";

const PAGE_SIZE = 100;

export interface LargeFilePageState {
  items: LargeFileRow[];
  total: number;
  nextOffset: number;
}

export function mergeLargeFilePage(
  current: LargeFilePageState,
  page: Paged<LargeFileRow>,
): LargeFilePageState {
  const seen = new Set<string>();
  const items = [...current.items, ...page.items].filter((item) => {
    if (seen.has(item.nodeId)) return false;
    seen.add(item.nodeId);
    return true;
  });
  return {
    items,
    total: page.total,
    nextOffset: page.offset + page.items.length,
  };
}

function formatModified(value: string | null, locale: string, unavailable: string) {
  if (!value) return unavailable;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? unavailable : new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

export function LargeFiles({ taskId, thresholdBytes }: { taskId: string; thresholdBytes: number }) {
  const { locale, tr } = useI18n();
  const toast = useToast();
  const activeTask = useRef(taskId);
  const requestPending = useRef(false);
  const [page, setPage] = useState<LargeFilePageState>({ items: [], total: 0, nextOffset: 0 });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    activeTask.current = taskId;
    requestPending.current = false;
    setPage({ items: [], total: 0, nextOffset: 0 });
    setLoading(true);
    setError(null);

    const requestedTask = taskId;
    requestPending.current = true;
    void invoke<Paged<LargeFileRow>>("space_scan_large_files", {
      taskId: requestedTask,
      minBytes: thresholdBytes,
      offset: 0,
      limit: PAGE_SIZE,
    }).then((result) => {
      if (activeTask.current !== requestedTask) return;
      setPage(mergeLargeFilePage({ items: [], total: 0, nextOffset: 0 }, result));
    }).catch(() => {
      if (activeTask.current === requestedTask) setError(tr("无法读取大文件列表，请重试。"));
    }).finally(() => {
      if (activeTask.current === requestedTask) {
        requestPending.current = false;
        setLoading(false);
      }
    });
  }, [taskId, thresholdBytes, tr]);

  async function loadMore() {
    if (requestPending.current || (page.items.length > 0 && page.nextOffset >= page.total)) return;
    requestPending.current = true;
    setLoading(true);
    setError(null);
    const requestedTask = taskId;
    const offset = page.items.length === 0 ? 0 : page.nextOffset;
    try {
      const result = await invoke<Paged<LargeFileRow>>("space_scan_large_files", {
        taskId: requestedTask,
        minBytes: thresholdBytes,
        offset,
        limit: PAGE_SIZE,
      });
      if (activeTask.current !== requestedTask) return;
      setPage((current) => mergeLargeFilePage(current, result));
    } catch {
      if (activeTask.current === requestedTask) setError(tr("无法读取更多大文件，请重试。"));
    } finally {
      if (activeTask.current === requestedTask) {
        requestPending.current = false;
        setLoading(false);
      }
    }
  }

  async function openContainingDirectory(path: string) {
    try {
      await invoke("space_open_directory", { path });
    } catch {
      toast(tr("无法打开文件所在目录，请确认路径仍然存在。"), "err");
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

  return (
    <div className="space-large-files">
      <div className="space-analysis-section-heading">
        <div>
          <strong>{tr("大文件")}</strong>
          <span>{tr("按实际磁盘占用排序，仅展示达到设置阈值的文件。")}</span>
        </div>
        <span>{tr("当前阈值")}: {formatSpaceBytes(thresholdBytes)}</span>
      </div>

      {page.items.length === 0 && loading && (
        <div className="space-analysis-state"><i className="ti ti-loader spin" /> {tr("正在读取大文件…")}</div>
      )}
      {page.items.length === 0 && !loading && !error && (
        <div className="space-analysis-empty">{tr("没有达到当前阈值的大文件。")}</div>
      )}
      {page.items.length === 0 && error && (
        <div className="space-analysis-state error">
          <span>{error}</span>
          <button type="button" className="gh sm" onClick={() => void loadMore()}>{tr("重试")}</button>
        </div>
      )}

      <div className="space-large-file-list">
        {page.items.map((file) => (
          <div className="space-large-file-row" key={file.nodeId}>
            <span className="space-file-icon"><i className="ti ti-file" /></span>
            <div className="space-large-file-main">
              <div>
                <strong title={file.name}>{file.name}</strong>
                <span className="bd">{tr("仅查看")}</span>
              </div>
              <span title={file.path}>{file.path}</span>
            </div>
            <div className="space-large-file-meta">
              <strong title={`${tr("实际占用")}: ${formatSpaceBytes(file.allocatedBytes)}`}>{tr("实际占用")} {formatSpaceBytes(file.allocatedBytes)}</strong>
              <span title={`${tr("逻辑大小")}: ${formatSpaceBytes(file.logicalBytes)}`}>{tr("逻辑大小")} {formatSpaceBytes(file.logicalBytes)}</span>
            </div>
            <time className="space-large-file-time" dateTime={file.modifiedAt ?? undefined}>
              {formatModified(file.modifiedAt, locale, tr("未知时间"))}
            </time>
            <div className="space-row-actions">
              <button type="button" className="space-icon-button" title={tr("打开所在目录")} aria-label={tr("打开所在目录")} onClick={() => void openContainingDirectory(file.path)}>
                <i className="ti ti-folder-open" />
              </button>
              <button type="button" className="space-icon-button" title={tr("复制路径")} aria-label={tr("复制路径")} onClick={() => void copyPath(file.path)}>
                <i className="ti ti-copy" />
              </button>
            </div>
          </div>
        ))}
      </div>

      {error && page.items.length > 0 && <div className="space-inline-error">{error}</div>}
      {page.nextOffset < page.total && (
        <button type="button" className="space-load-more" disabled={loading} onClick={() => void loadMore()}>
          <i className={`ti ${loading ? "ti-loader spin" : "ti-chevron-down"}`} />
          {loading ? tr("正在加载…") : `${tr("加载更多")} (${page.items.length}/${page.total})`}
        </button>
      )}
    </div>
  );
}
