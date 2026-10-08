import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useBusyRead, useToast } from "../../../ui";
import type { LargeFileRow, Paged } from "../types";
import { formatSpaceBytes } from "./SpaceOverview";
import { RecycleBar, useProtectedPaths } from "./FileRemoval";
import { useDragPick, setInSet } from "../../../dragPick";
import { CompressDialog, mediaKind, type Outcome } from "./CompressDialog";

type TypeFilter = "" | "video" | "image" | "packed";
const TYPE_FILTERS: { value: TypeFilter; label: string }[] = [
  { value: "", label: "全部" },
  { value: "video", label: "视频" },
  { value: "image", label: "图片" },
  { value: "packed", label: "其他" },
];
const TYPE_ICONS = { video: "ti-movie", image: "ti-photo", packed: "ti-file" } as const;

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
  const [typeFilter, setTypeFilter] = useState<TypeFilter>("");
  const [compressing, setCompressing] = useState(false);
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

  // The type filter works on what is loaded so far.
  const shown = typeFilter ? page.items.filter((item) => mediaKind(item.path) === typeFilter) : page.items;
  const pickable = shown.filter((item) => !protectedPaths.has(item.path));
  const pick = useDragPick(shown.map((item) => protectedPaths.has(item.path) ? [] : [item.path]), (path) => picked.has(path), (paths, on) => setPicked((old) => setInSet(old, paths, on)));
  const allPicked = pickable.length > 0 && pickable.every((item) => picked.has(item.path));
  const chosen = shown.filter((item) => picked.has(item.path)).map((item) => ({ path: item.path, bytes: item.logicalBytes }));
  // A compressed file stays in the list with its new name and size.
  const compressed = (outcomes: Outcome[]) => {
    const byPath = new Map(outcomes.filter((row) => row.status === "ok").map((row) => [row.path, row]));
    setPage((current) => ({
      ...current,
      items: current.items.map((item) => {
        const row = byPath.get(item.path);
        if (!row) return item;
        const name = row.newPath.split(/[\\/]/).pop() ?? item.name;
        return row.method === "packed"
          ? { ...item, allocatedBytes: row.after }
          : { ...item, path: row.newPath, name, logicalBytes: row.after, allocatedBytes: row.after };
      }),
    }));
    setPicked(new Set());
  };
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
        <div className="space-cleanup-heading-actions">
          <span className="s dim">{tr("当前阈值")}{locale === "zh-CN" ? "：" : ": "}{formatSpaceBytes(thresholdBytes)}</span>
          <div className="seg sm">{TYPE_FILTERS.map((option) => <button key={option.value} type="button" className={typeFilter === option.value ? "on" : ""}
            onClick={() => setTypeFilter(option.value)}>{tr(option.label)}</button>)}</div>
          <div className="space-selection-actions">
            <button type="button" className="gh sm" disabled={!pickable.length || allPicked} title={tr("选择已加载的全部文件")}
              onClick={() => setPicked((old) => setInSet(old, pickable.map((item) => item.path), true))}><i className="ti ti-checkbox" /> {tr("全选")}</button>
            <button type="button" className="gh sm" disabled={!picked.size} title={tr("取消全部选择")} onClick={() => setPicked(new Set())}>
              <i className="ti ti-square" /> {tr("全部取消")}</button>
          </div>
        </div>
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
        {shown.map((file, i) => (
          <div className={"space-large-file-row" + (picked.has(file.path) ? " picked" : "")} key={file.nodeId} {...pick.row(i)}>
            {protectedPaths.has(file.path)
              ? <span className="recycle-lock" title={tr("系统或程序目录里的文件不能在这里删除")}><i className="ti ti-lock" /></span>
              : <input type="checkbox" className="recycle-check" checked={picked.has(file.path)} aria-label={tr("选择")} title={tr("按住拖过几行可以一起勾选；按住 Shift 点选一段")}
                {...pick.box(i)} onChange={(e) => pick.change(i, e.target.checked)} />}
            <span className="space-file-icon"><i className={"ti " + TYPE_ICONS[mediaKind(file.path)]} /></span>
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
      {page.items.length > 0 && shown.length === 0 && <div className="space-analysis-empty compact">{tr("已加载的文件里没有这一类。")}</div>}
      {page.items.length > 0 && <RecycleBar files={chosen} onDone={removed}
        extra={<button type="button" className="gh sm" disabled={!chosen.length}
          title={tr("视频转 H.265、图片重新压缩，其他文件用 Windows 透明压缩；先估算再压缩")} onClick={() => setCompressing(true)}>
          <i className="ti ti-file-zip" /> {tr("压缩所选")}{chosen.length ? ` (${chosen.length})` : ""}
        </button>} />}
      {compressing && <CompressDialog files={chosen} onClose={() => setCompressing(false)} onDone={compressed} />}
    </div>
  );
}
