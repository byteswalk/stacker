import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useBusyRead, useToast } from "../../../ui";
import type { LargeFileRow, Paged } from "../types";
import { formatSpaceBytes } from "./SpaceOverview";
import { RecycleBar, useProtectedPaths } from "./FileRemoval";

const PAGE_SIZE = 100;

export interface LargeFilePageState {
  items: LargeFileRow[];
  total: number;
  nextOffset: number;
}

export interface LargeFileRequestIdentity {
  taskId: string;
  thresholdBytes: number;
  generation: number;
}

export function sameLargeFileRequest(
  current: LargeFileRequestIdentity,
  requested: LargeFileRequestIdentity,
): boolean {
  return current.taskId === requested.taskId
    && current.thresholdBytes === requested.thresholdBytes
    && current.generation === requested.generation;
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
  const read = useBusyRead();
  const activeRequest = useRef<LargeFileRequestIdentity>({
    taskId,
    thresholdBytes,
    generation: 0,
  });
  const requestPending = useRef(false);
  const [page, setPage] = useState<LargeFilePageState>({ items: [], total: 0, nextOffset: 0 });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // Picked for removal, by path; files under Windows or a program's folder cannot be picked.
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const protectedPaths = useProtectedPaths(page.items.map((item) => item.path));

  useEffect(() => {
    const requested = {
      taskId,
      thresholdBytes,
      generation: activeRequest.current.generation + 1,
    };
    activeRequest.current = requested;
    requestPending.current = false;
    setPage({ items: [], total: 0, nextOffset: 0 });
    setPicked(new Set());
    setLoading(true);
    setError(null);

    requestPending.current = true;
    void read("正在读取大文件", () => invoke<Paged<LargeFileRow>>("space_scan_large_files", {
      taskId: requested.taskId,
      minBytes: thresholdBytes,
      offset: 0,
      limit: PAGE_SIZE,
    })).then((result) => {
      if (!sameLargeFileRequest(activeRequest.current, requested)) return;
      setPage(mergeLargeFilePage({ items: [], total: 0, nextOffset: 0 }, result));
    }).catch(() => {
      if (sameLargeFileRequest(activeRequest.current, requested)) setError(tr("无法读取大文件列表，请重试。"));
    }).finally(() => {
      if (sameLargeFileRequest(activeRequest.current, requested)) {
        requestPending.current = false;
        setLoading(false);
      }
    });
    return () => {
      if (sameLargeFileRequest(activeRequest.current, requested)) {
        activeRequest.current = { ...requested, generation: requested.generation + 1 };
        requestPending.current = false;
      }
    };
  }, [read, taskId, thresholdBytes, tr]);

  async function loadMore() {
    if (requestPending.current || (page.items.length > 0 && page.nextOffset >= page.total)) return;
    requestPending.current = true;
    setLoading(true);
    setError(null);
    const requested = activeRequest.current;
    const offset = page.items.length === 0 ? 0 : page.nextOffset;
    try {
      const result = await read("正在读取大文件", () => invoke<Paged<LargeFileRow>>("space_scan_large_files", {
        taskId: requested.taskId,
        minBytes: requested.thresholdBytes,
        offset,
        limit: PAGE_SIZE,
      }));
      if (!sameLargeFileRequest(activeRequest.current, requested)) return;
      setPage((current) => mergeLargeFilePage(current, result));
    } catch {
      if (sameLargeFileRequest(activeRequest.current, requested)) setError(tr("无法读取更多大文件，请重试。"));
    } finally {
      if (sameLargeFileRequest(activeRequest.current, requested)) {
        requestPending.current = false;
        setLoading(false);
      }
    }
  }

  async function openContainingDirectory(path: string) {
    try {
      await invoke("space_open_directory", { path });
    } catch {
      toast(tr("无法打开文件夹。请确认路径仍然存在，并且当前账户拥有访问权限。"), "err");
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

  const pickable = page.items.filter((item) => !protectedPaths.has(item.path));
  const allPicked = pickable.length > 0 && pickable.every((item) => picked.has(item.path));
  const toggle = (path: string) => setPicked((old) => {
    const next = new Set(old);
    if (next.has(path)) next.delete(path); else next.add(path);
    return next;
  });
  const removed = (paths: string[]) => {
    const gone = new Set(paths);
    setPage((current) => ({ ...current, items: current.items.filter((item) => !gone.has(item.path)), total: current.total - gone.size }));
    setPicked((old) => new Set([...old].filter((path) => !gone.has(path))));
  };

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

      {page.items.length > 0 && <RecycleBar
        files={page.items.filter((item) => picked.has(item.path)).map((item) => ({ path: item.path, bytes: item.logicalBytes }))}
        extra={<label className="recycle-all">
          <input type="checkbox" checked={allPicked} disabled={!pickable.length}
            onChange={(e) => setPicked(e.target.checked ? new Set(pickable.map((item) => item.path)) : new Set())} />
          {tr("全选已加载的")}
        </label>}
        onDone={removed} />}

      <div className="space-large-file-list">
        {page.items.map((file) => (
          <div className={"space-large-file-row" + (picked.has(file.path) ? " picked" : "")} key={file.nodeId}>
            {protectedPaths.has(file.path)
              ? <span className="recycle-lock" title={tr("系统或程序目录里的文件不能在这里删除")}><i className="ti ti-lock" /></span>
              : <input type="checkbox" className="recycle-check" checked={picked.has(file.path)} aria-label={tr("选择")}
                onChange={() => toggle(file.path)} />}
            <span className="space-file-icon"><i className="ti ti-file" /></span>
            <div className="space-large-file-main">
              <div>
                <strong title={file.name}>{file.name}</strong>
                {protectedPaths.has(file.path) && <span className="bd">{tr("系统文件")}</span>}
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
