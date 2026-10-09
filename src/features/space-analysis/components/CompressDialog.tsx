import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "../../../invoke";
import { useI18n } from "../../../i18n";
import { Modal, useBusy, useToast } from "../../../ui";
import { Select } from "../../../Select";
import { formatSpaceBytes } from "./SpaceOverview";
import { FILES_GONE, type FileToRemove } from "./FileRemoval";

type MediaTools = { ffmpeg: string | null; version: string; videoEncoders: string[]; imageFormats: string[] };
type Quality = "high" | "standard" | "small";
type Options = { videoQuality: Quality; videoHeight: number; videoEncoder: string; imageFormat: string; imageQuality: Quality };
export type Outcome = { path: string; newPath: string; method: "packed" | "video" | "image"; before: number; after: number; status: string };
type Progress = { done: number; total: number; path: string; percent: number };

const VIDEO = ["mp4", "mkv", "mov", "avi", "wmv", "flv", "m4v", "webm", "ts", "mts", "m2ts", "3gp", "mpg", "mpeg"];
// No TIFF, as on the Rust side: a multi-page scan would keep only its first page.
const IMAGE = ["jpg", "jpeg", "png", "bmp"];

export function mediaKind(path: string): "video" | "image" | "packed" {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return VIDEO.includes(ext) ? "video" : IMAGE.includes(ext) ? "image" : "packed";
}

const QUALITIES: { value: Quality; label: string }[] = [
  { value: "high", label: "高画质" },
  { value: "standard", label: "标准" },
  { value: "small", label: "省空间" },
];
const HEIGHTS = [
  { value: "0", label: "保持原分辨率" },
  { value: "720", label: "720p" },
  { value: "1080", label: "1080p" },
  { value: "1440", label: "2K（1440p）" },
  { value: "2160", label: "4K（2160p）" },
];
const ENCODERS: Record<string, string> = {
  x265: "CPU（x265，最省空间）", nvenc: "NVIDIA 显卡（快）", qsv: "Intel 核显（快）", amf: "AMD 显卡（快）",
};
const FORMATS: Record<string, string> = { jpeg: "JPEG（兼容最好）", webp: "WebP", avif: "AVIF（最省空间）" };
const METHODS: Record<Outcome["method"], string> = { packed: "透明压缩", video: "视频转码", image: "图片压缩" };
const STATUS: Record<string, string> = {
  ok: "完成", estimated: "", already: "已是压缩格式，跳过", notWorth: "省不了多少，保持原样", keepPng: "PNG 保持原格式",
  noFfmpeg: "需要先下载 ffmpeg", protected: "系统或程序目录里的文件不能改动", changed: "文件在扫描后变过，已跳过", cancelled: "已停止",
};
const OPTIONS_KEY = "stacker.space.compressOptions";

function readOptions(): Options {
  const fallback: Options = { videoQuality: "standard", videoHeight: 0, videoEncoder: "x265", imageFormat: "jpeg", imageQuality: "standard" };
  try { return { ...fallback, ...(JSON.parse(localStorage.getItem(OPTIONS_KEY) ?? "{}") as Partial<Options>) }; } catch { return fallback; }
}

/**
 * Makes the picked files smaller: transparent compression for files that are not compressed
 * already, H.265 for videos, JPEG/WebP/AVIF for photos. An estimate first, then the run;
 * replaced originals go to the Recycle Bin.
 */
export function CompressDialog({ files, onClose, onDone }: {
  files: FileToRemove[]; onClose: () => void; onDone: (outcomes: Outcome[]) => void;
}) {
  const { tr } = useI18n();
  const toast = useToast();
  const runBusy = useBusy();
  const [tools, setTools] = useState<MediaTools | null>(null);
  const [options, setOptionsState] = useState<Options>(readOptions);
  const [rows, setRows] = useState<Outcome[] | null>(null);
  const [mode, setMode] = useState<"idle" | "estimating" | "running" | "done">("idle");
  const [progress, setProgress] = useState<Progress | null>(null);
  const counts = { video: 0, image: 0, packed: 0 };
  for (const file of files) counts[mediaKind(file.path)] += 1;
  const needsFfmpeg = counts.video + counts.image > 0;
  const busy = mode === "estimating" || mode === "running";

  const setOptions = (patch: Partial<Options>) => {
    const next = { ...options, ...patch };
    setOptionsState(next);
    setRows(null);
    if (mode === "done") setMode("idle");
    try { localStorage.setItem(OPTIONS_KEY, JSON.stringify(next)); } catch { /* a convenience */ }
  };

  useEffect(() => {
    invoke<MediaTools>("space_media_tools").then((found) => {
      setTools(found);
      // A choice this ffmpeg cannot make falls back to one it can.
      setOptionsState((old) => ({
        ...old,
        videoEncoder: found.videoEncoders.includes(old.videoEncoder) ? old.videoEncoder : found.videoEncoders[0] ?? "x265",
        imageFormat: found.imageFormats.includes(old.imageFormat) ? old.imageFormat : "jpeg",
      }));
    }).catch(() => setTools({ ffmpeg: null, version: "", videoEncoders: [], imageFormats: [] }));
  }, []);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen<Progress>("space-compress-progress", (event) => setProgress(event.payload))
      .then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, []);

  async function installFfmpeg() {
    try {
      const found = await runBusy({
        title: tr("下载 ffmpeg"),
        message: tr("从 gyan.dev 下载 ffmpeg 官方 Windows 构建（约 109 MB），校验 SHA-256 后放到 Stacker 的工具目录。"),
        progressEvent: "install-progress",
        cancel: { label: tr("取消下载"), onCancel: () => { void invoke("op_cancel").catch(() => undefined); } },
      }, () => invoke<MediaTools>("space_ffmpeg_install"));
      setTools(found);
      setOptionsState((old) => ({ ...old, videoEncoder: found.videoEncoders[0] ?? "x265" }));
      toast(tr("ffmpeg 已就绪"), "ok");
    } catch (error) { toast(String(error), "err"); }
  }

  async function go(run: boolean) {
    setMode(run ? "running" : "estimating");
    setProgress(null);
    try {
      const found = await invoke<Outcome[]>(run ? "space_compress_run" : "space_compress_estimate", { targets: files, options });
      setRows(found);
      setMode(run ? "done" : "idle");
      if (run) {
        onDone(found);
        const saved = found.filter((row) => row.status === "ok").reduce((sum, row) => sum + row.before - row.after, 0);
        // Re-encoded originals sit in the Recycle Bin: until it is emptied the disk holds both.
        const replaced = found.some((row) => row.status === "ok" && row.method !== "packed");
        const originals = found.filter((row) => row.status === "ok" && row.method !== "packed").map((row) => row.path);
        if (originals.length) window.dispatchEvent(new CustomEvent<string[]>(FILES_GONE, { detail: originals }));
        toast(tr(replaced ? "压缩完成，省出 {size}；原文件在回收站，清空回收站后才真正释放" : "压缩完成，省出 {size}")
          .replace("{size}", formatSpaceBytes(Math.max(0, saved))), "ok");
      }
    } catch (error) {
      setMode("idle");
      toast(String(error), "err");
    }
  }

  const done = (rows ?? []).filter((row) => row.status === "ok" || row.status === "estimated");
  const saving = done.reduce((sum, row) => sum + Math.max(0, row.before - row.after), 0);
  const name = (path: string) => path.split(/[\\/]/).pop() ?? path;
  const statusText = (row: Outcome) => tr(STATUS[row.status] ?? row.status);

  return <Modal wide title={tr("压缩所选文件")} icon="ti-file-zip" onClose={busy ? undefined : onClose}
    sub={tr("已选 {count} 个：视频 {video} 个，图片 {image} 个，其他 {other} 个")
      .replace("{count}", String(files.length)).replace("{video}", String(counts.video))
      .replace("{image}", String(counts.image)).replace("{other}", String(counts.packed))}
    footer={<>
      {busy
        ? <button className="gh sm" onClick={() => void invoke("space_compress_cancel")}><i className="ti ti-player-stop" /> {tr("停止")}</button>
        : <button className="gh sm" onClick={onClose}>{tr(mode === "done" ? "完成" : "取消")}</button>}
      {mode !== "done" && <>
        <button className="gh sm" disabled={busy || !tools} onClick={() => void go(false)}>
          <i className={"ti " + (mode === "estimating" ? "ti-loader spin" : "ti-calculator")} /> {tr("估算")}
        </button>
        <button className="pr sm" disabled={busy || !tools} onClick={() => void go(true)}>
          <i className={"ti " + (mode === "running" ? "ti-loader spin" : "ti-file-zip")} /> {tr("开始压缩")}
        </button>
      </>}
    </>}>
    <div className="compress">
      {needsFfmpeg && tools && !tools.ffmpeg && <div className="callout" style={{ margin: 0 }}>
        <i className="ti ti-download" />
        <div>{tr("压缩视频和图片要用 ffmpeg（开源的音视频工具）。下载一次即可，约 109 MB。")}</div>
        <button className="pr sm" onClick={() => void installFfmpeg()}><i className="ti ti-download" /> {tr("下载 ffmpeg")}</button>
      </div>}

      {counts.video > 0 && <section className="compress-part">
        <div className="compress-head"><i className="ti ti-movie" /> <b>{tr("视频")}</b><span>{tr("重新编码为 H.265；只缩小不放大，原视频移到回收站")}</span></div>
        <div className="compress-row">
          <span>{tr("画质")}</span>
          <div className="seg sm">{QUALITIES.map((q) => <button key={q.value} className={options.videoQuality === q.value ? "on" : ""} disabled={busy}
            onClick={() => setOptions({ videoQuality: q.value })}>{tr(q.label)}</button>)}</div>
          <span>{tr("最高分辨率")}</span>
          <Select value={String(options.videoHeight)} width={150} disabled={busy} onChange={(v) => setOptions({ videoHeight: Number(v) })}
            options={HEIGHTS.map((h) => ({ value: h.value, label: tr(h.label) }))} />
          {tools && tools.videoEncoders.length > 0 && <>
            <span>{tr("编码方式")}</span>
            <Select value={options.videoEncoder} width={190} disabled={busy} onChange={(v) => setOptions({ videoEncoder: v })}
              options={tools.videoEncoders.map((id) => ({ value: id, label: tr(ENCODERS[id] ?? id) }))} />
          </>}
        </div>
        <p className="compress-note">{tr("高画质与原片几乎看不出差别，约省 40%；标准约省 50–60%；省空间更小但细节会少一些。显卡编码快几倍，同画质文件稍大。")}</p>
      </section>}

      {counts.image > 0 && <section className="compress-part">
        <div className="compress-head"><i className="ti ti-photo" /> <b>{tr("图片")}</b><span>{tr("保留拍摄信息（EXIF）和色彩配置；原图移到回收站")}</span></div>
        <div className="compress-row">
          <span>{tr("格式")}</span>
          <div className="seg sm">{(tools?.imageFormats ?? ["jpeg"]).map((f) => <button key={f} className={options.imageFormat === f ? "on" : ""} disabled={busy}
            onClick={() => setOptions({ imageFormat: f })}>{tr(FORMATS[f] ?? f)}</button>)}</div>
          <span>{tr("画质")}</span>
          <div className="seg sm">{QUALITIES.map((q) => <button key={q.value} className={options.imageQuality === q.value ? "on" : ""} disabled={busy}
            onClick={() => setOptions({ imageQuality: q.value })}>{tr(q.label)}</button>)}</div>
        </div>
        <p className="compress-note">{tr("JPEG 所有软件都能打开；WebP、AVIF 更小，个别老软件打不开。PNG 截图选 WebP/AVIF 时转为无损 WebP，选 JPEG 时保持不变。AVIF 不保留 EXIF。")}</p>
      </section>}

      {counts.packed > 0 && <section className="compress-part">
        <div className="compress-head"><i className="ti ti-file-zip" /> <b>{tr("其他文件")}</b><span>{tr("Windows 透明压缩（NTFS LZX）")}</span></div>
        <p className="compress-note">{tr("文件名、格式和内容都不变，所有程序照常读写，只是在磁盘上占得更少；文件被修改后会自动变回未压缩。zip、mp4、jpg 这类已压缩的格式会跳过。")}</p>
      </section>}

      {busy && progress && <div className="compress-progress">
        <i className="ti ti-loader spin" />
        <span>{tr(mode === "running" ? "正在压缩" : "正在估算")} {Math.min(progress.done + 1, progress.total)}/{progress.total}</span>
        <span className="compress-file" translate="no" title={progress.path}>{name(progress.path)}</span>
        {progress.percent > 0 && progress.percent < 100 && <b>{Math.round(progress.percent)}%</b>}
      </div>}

      {rows && <div className="compress-table">
        <div className="compress-line head">
          <span>{tr("文件")}</span><span>{tr("方式")}</span><span>{tr("原大小")}</span>
          <span>{tr(mode === "done" ? "压缩后" : "预计")}</span><span>{tr("节省")}</span>
        </div>
        {rows.map((row) => {
          const counted = row.status === "ok" || row.status === "estimated";
          const saved = Math.max(0, row.before - row.after);
          return <div className={"compress-line" + (counted ? "" : " skipped")} key={row.path}>
            <span translate="no" title={row.newPath !== row.path ? `${row.path}\n→ ${row.newPath}` : row.path}>{name(row.newPath)}</span>
            <span>{tr(METHODS[row.method])}</span>
            <span>{formatSpaceBytes(row.before)}</span>
            <span>{counted ? formatSpaceBytes(row.after) : "—"}</span>
            <span className={counted && saved > 0 ? "good" : "mut"} title={counted ? undefined : statusText(row)}>
              {counted ? `${formatSpaceBytes(saved)} · ${Math.round((saved / Math.max(1, row.before)) * 100)}%` : statusText(row)}
            </span>
          </div>;
        })}
        <div className="compress-total">{tr(mode === "done" ? "共省出 {size}" : "预计共省出 {size}").replace("{size}", formatSpaceBytes(saving))}</div>
      </div>}
    </div>
  </Modal>;
}
