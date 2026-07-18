import { useEffect, useMemo, useState } from "react";
import { ScanHeader } from "../features/space-analysis/components/ScanHeader";
import { ScanLauncher } from "../features/space-analysis/components/ScanLauncher";
import { AnalysisTabs } from "../features/space-analysis/components/AnalysisTabs";
import { cancelScan, startQuickScan, useSpaceScan } from "../features/space-analysis/store";
import type { KnownSpaceItem } from "../features/space-analysis/types";
import { quickScanView } from "../features/space-analysis/viewModel";
import { useI18n } from "../i18n";
import { invoke } from "../invoke";
import { useNotifications } from "../notifications";
import { ConfirmModal, Modal, useToast } from "../ui";

type CleanupCategory = "safe" | "history" | "temp" | "cautious";

type CacheItem = {
  name: string;
  path: string;
  size: number;
  category: CleanupCategory;
  safety: string;
  safetyLabel: string;
  icon: string;
  av: string;
  canSelect: boolean;
};

type Translate = (value: string) => string;

const KNOWN_ITEM_NAMES: Record<string, string> = {
  "spaceAnalysis.known.gradle": "Gradle 缓存",
  "spaceAnalysis.known.goModules": "Go 模块缓存",
  "spaceAnalysis.known.pnpm": "pnpm 存储",
  "spaceAnalysis.known.npm": "npm 缓存",
  "spaceAnalysis.known.cargoRegistry": "Cargo registry 缓存",
  "spaceAnalysis.known.pip": "pip 缓存",
  "spaceAnalysis.known.electron": "Electron 下载缓存",
  "spaceAnalysis.known.playwright": "Playwright 浏览器",
  "spaceAnalysis.known.huggingFace": "Hugging Face 模型缓存",
  "spaceAnalysis.known.mavenRepository": "Maven 本地仓库",
  "spaceAnalysis.known.jetbrainsHistory": "JetBrains 历史版本",
  "spaceAnalysis.known.windowsTemp": "Windows 临时目录",
  "spaceAnalysis.known.userTemp": "用户临时目录",
};

const SAFETY_LABELS: Record<string, string> = {
  safe: "可安全清理",
  rebuildable: "可重新生成",
  needs_confirmation: "需要确认",
  view_only: "仅供查看",
};

function fmt(bytes: number) {
  if (bytes >= 1024 ** 3) return (bytes / 1024 ** 3).toFixed(1) + " GB";
  if (bytes >= 1024 ** 2) return (bytes / 1024 ** 2).toFixed(0) + " MB";
  if (bytes >= 1024) return (bytes / 1024).toFixed(0) + " KB";
  return bytes + " B";
}

function itemCategory(item: KnownSpaceItem): CleanupCategory {
  if (item.nameKey === "spaceAnalysis.known.jetbrainsHistory") return "history";
  if (
    item.nameKey === "spaceAnalysis.known.windowsTemp"
    || item.nameKey === "spaceAnalysis.known.userTemp"
  ) return "temp";
  return item.safety === "safe" ? "safe" : "cautious";
}

function itemVisuals(item: KnownSpaceItem): Pick<CacheItem, "icon" | "av"> {
  if (item.id === "gradle") return { icon: "ti-box", av: "gr" };
  if (item.id === "gomod") return { icon: "ti-brand-golang", av: "go" };
  if (item.id === "pnpm" || item.id === "npm") return { icon: "ti-brand-npm", av: "npm" };
  if (item.id === "cargo") return { icon: "ti-brand-rust", av: "rs" };
  if (item.id === "pip") return { icon: "ti-brand-python", av: "py" };
  if (item.id === "electron") return { icon: "ti-bolt", av: "el" };
  if (item.id === "playwright") return { icon: "ti-theater", av: "el" };
  if (item.id === "hf") return { icon: "ti-robot", av: "hf" };
  if (item.id === "m2repo") return { icon: "ti-feather", av: "mv2" };
  if (item.nameKey === "spaceAnalysis.known.jetbrainsHistory") return { icon: "ti-code", av: "st" };
  if (item.nameKey.endsWith("Temp")) return { icon: "ti-trash", av: "st" };
  return { icon: "ti-box", av: "st" };
}

function normalizeSafety(safety: string) {
  return safety.replaceAll("-", "_").replaceAll(" ", "_").toLowerCase();
}

function leafName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

function toCacheItem(item: KnownSpaceItem, tr: Translate): CacheItem {
  const safety = normalizeSafety(item.safety);
  const sourceName = KNOWN_ITEM_NAMES[item.nameKey] ?? "已知空间项目";
  const name = item.nameKey === "spaceAnalysis.known.jetbrainsHistory"
    ? `${tr(sourceName)} · ${leafName(item.path)}`
    : tr(sourceName);
  return {
    name,
    path: item.path,
    size: item.bytes,
    category: itemCategory(item),
    safety,
    safetyLabel: tr(SAFETY_LABELS[safety] ?? "需要确认"),
    ...itemVisuals(item),
    canSelect: safety !== "view_only",
  };
}

export default function Cleanup() {
  const toast = useToast();
  const notices = useNotifications();
  const { locale, tr } = useI18n();
  const scan = useSpaceScan();
  const view = quickScanView(scan);
  const items = useMemo(
    () => scan.result?.items.map((item) => toCacheItem(item, tr)) ?? [],
    [scan.result, tr],
  );
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<string[] | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [cleanupBusy, setCleanupBusy] = useState(false);
  const [aged, setAged] = useState<string | null>(null);
  const [agedDays, setAgedDays] = useState(90);
  const [agedStats, setAgedStats] = useState<{ count: number; size: number } | null>(null);
  const [agedBusy, setAgedBusy] = useState(false);

  useEffect(() => {
    setSel(new Set(
      scan.result?.items
        .filter((item) => normalizeSafety(item.safety) === "safe")
        .map((item) => item.path) ?? [],
    ));
  }, [scan.result]);

  async function cancelCurrentScan() {
    setActionBusy(true);
    try {
      await cancelScan();
    } catch {
      toast(tr("无法取消扫描，请重试。"), "err");
    } finally {
      setActionBusy(false);
    }
  }

  async function refreshAfterCleanup() {
    try {
      await startQuickScan();
    } catch {
      toast(tr("清理已完成，但无法启动重新扫描。"), "err");
    }
  }

  async function del(paths: string[]) {
    setCleanupBusy(true);
    try {
      const freed = await invoke<number>("cleanup_delete", { paths });
      toast(`${tr("已释放")} ${fmt(freed)}`, "ok");
      setConfirm(null);
      await refreshAfterCleanup();
    } catch {
      toast(tr("清理失败，请重试。"), "err");
    } finally {
      setCleanupBusy(false);
    }
  }

  async function loadStats(path: string, days: number) {
    setAgedDays(days);
    setAgedStats(null);
    try {
      setAgedStats(await invoke<{ count: number; size: number }>("cleanup_aged_stats", { path, days }));
    } catch {
      toast(tr("统计失败，请重试。"), "err");
    }
  }

  function openAged(path: string) {
    setAged(path);
    void loadStats(path, 90);
  }

  async function delAged() {
    if (!aged) return;
    setAgedBusy(true);
    try {
      const freed = await invoke<number>("cleanup_delete_aged", { path: aged, days: agedDays });
      toast(`${tr("已释放")} ${fmt(freed)}`, "ok");
      setAged(null);
      await refreshAfterCleanup();
    } catch {
      toast(tr("清理失败，请重试。"), "err");
    } finally {
      setAgedBusy(false);
    }
  }

  const safe = items.filter((item) => item.category === "safe");
  const history = items.filter((item) => item.category === "history");
  const temp = items.filter((item) => item.category === "temp");
  const cautious = items.filter((item) => item.category === "cautious");
  const total = scan.result?.totalBytes ?? items.reduce((sum, item) => sum + item.size, 0);
  const safeTotal = scan.result?.safelyReleasableBytes ?? safe.reduce((sum, item) => sum + item.size, 0);
  const selItems = items.filter((item) => sel.has(item.path));
  const selTotal = selItems.reduce((sum, item) => sum + item.size, 0);

  function toggle(item: CacheItem) {
    if (!item.canSelect) return;
    setSel((current) => {
      const next = new Set(current);
      if (next.has(item.path)) next.delete(item.path);
      else next.add(item.path);
      return next;
    });
  }

  function categoryBadge(item: CacheItem) {
    if (item.category === "history") return <span className="bd w">{tr("历史版本")}</span>;
    if (item.category === "temp") return <span className="bd w">{tr("临时文件")}</span>;
    if (item.category === "cautious") return <span className="bd w">{tr("谨慎")}</span>;
    return null;
  }

  function row(item: CacheItem) {
    const agedClean = item.category === "cautious";
    const cautiousStyle = item.category === "safe"
      ? undefined
      : { boxShadow: "inset 3px 0 0 var(--amber)", borderColor: "rgba(228,180,80,.3)" };
    return (
      <div className="clrow" key={item.path} style={cautiousStyle}>
        <input
          type="checkbox"
          className="ck2"
          checked={item.canSelect && sel.has(item.path)}
          disabled={!item.canSelect || cleanupBusy}
          aria-label={`${tr("选择清理项")}: ${item.name}`}
          onChange={() => toggle(item)}
        />
        <span className={`av ${item.av}`}><i className={`ti ${item.icon}`} /></span>
        <div className="ct2">
          <div className="ch" title={item.name}>
            <span className="ch-name">{item.name}</span>
            {categoryBadge(item)}
          </div>
          <div className="cs" title={item.path}>{item.path}</div>
        </div>
        <div className="csz" title={`${fmt(item.size)} · ${item.safetyLabel}`}>
          <div className="big">{fmt(item.size)}</div>
          <div className="small">{item.safetyLabel}</div>
        </div>
        {item.canSelect && (
          <button
            className="gh sm"
            disabled={cleanupBusy}
            onClick={() => agedClean ? openAged(item.path) : setConfirm([item.path])}
          >
            {agedClean ? tr("智能清理") : tr("清理")}
          </button>
        )}
      </div>
    );
  }

  const heroTitle = view.phase === "completed" && scan.result
    ? `${tr("可清理项共占用")} ${fmt(total)} · ${tr("可安全释放")} ${fmt(safeTotal)}`
    : tr(view.title);
  const idleDescription = notices.cleanupBytes > 0
    ? `${tr("后台估算可安全清理约")} ${fmt(notices.cleanupBytes)}${locale === "zh-CN" ? "。" : ". "}${tr("开始扫描后可查看完整清理项。")}`
    : tr(view.description);

  return (
    <>
      <ScanLauncher disabled={cleanupBusy} />
      <ScanHeader
        scan={scan}
        headline={view.phase === "completed" && scan.result ? heroTitle : undefined}
        idleDescription={idleDescription}
        actionBusy={actionBusy}
        onCancel={cancelCurrentScan}
        trailingAction={view.phase === "completed" && scan.request?.mode === "quick" ? (
          <button
            className="pr"
            disabled={cleanupBusy || sel.size === 0}
            onClick={() => setConfirm([...sel])}
          >
            <i className="ti ti-eraser" /> {tr("清理所选")}{locale === "zh-CN" ? `（${fmt(selTotal)}）` : ` (${fmt(selTotal)})`}
          </button>
        ) : null}
      />

      {view.phase === "completed"
        && scan.taskId
        && scan.request
        && scan.request.mode !== "quick"
        && <AnalysisTabs taskId={scan.taskId} request={scan.request} />}

      {view.phase === "completed" && items.length === 0 && scan.result && (
        <div className="stub">
          <div className="si"><i className="ti ti-circle-check" /></div>
          <h2>{tr("未发现可清理项")}</h2>
          <p>{tr("当前扫描范围内没有达到显示条件的缓存、历史版本或临时文件。")}</p>
        </div>
      )}
      {safe.length > 0 && <div className="seclabel"><i className="ti ti-shield-check" style={{ color: "#6bcf86" }} /> {tr("可安全清理（纯缓存，删除后会自动重新获取）")}</div>}
      {safe.map(row)}
      {history.length > 0 && <div className="seclabel"><i className="ti ti-code" style={{ color: "#e4b450" }} /> {tr("JetBrains IDE 历史版本（保留同产品最新版本）")}</div>}
      {history.map(row)}
      {temp.length > 0 && <div className="seclabel"><i className="ti ti-trash" style={{ color: "#e4b450" }} /> {tr("Windows 临时目录（超过 1 GB 才显示）")}</div>}
      {temp.map(row)}
      {cautious.length > 0 && <div className="seclabel"><i className="ti ti-alert-triangle" style={{ color: "#e4b450" }} /> {tr("谨慎清理（重新下载可能耗时较长）")}</div>}
      {cautious.map(row)}

      {confirm && (
        <ConfirmModal
          title={tr("确认清理")}
          icon="ti-eraser"
          message={<>{tr("将清理")} <b style={{ color: "var(--tx)" }}>{confirm.length} {tr("项")}</b>{locale === "zh-CN" ? "，" : "; "}{tr("预计释放")} <b style={{ color: "#6bcf86" }}>{fmt(items.filter((item) => confirm.includes(item.path)).reduce((sum, item) => sum + item.size, 0))}</b>{locale === "zh-CN" ? "。" : "."}<br />{tr("缓存和临时目录会清理目录内容；JetBrains 历史版本会删除旧版本目录；被系统占用的临时文件会自动跳过。")}</>}
          confirmLabel={cleanupBusy ? tr("清理中…") : tr("清理")}
          busy={cleanupBusy}
          onConfirm={() => del(confirm)}
          onClose={() => setConfirm(null)}
        />
      )}

      {aged && (
        <Modal
          wide
          title={tr("智能清理（按未访问时长）")}
          icon="ti-clock"
          onClose={() => !agedBusy && setAged(null)}
          footer={<>
            <button className="gh sm" disabled={agedBusy} onClick={() => setAged(null)}>{tr("取消")}</button>
            <button className="pr sm" disabled={agedBusy || !agedStats || agedStats.count === 0} onClick={delAged}>
              <i className="ti ti-eraser" /> {agedBusy ? tr("清理中…") : agedStats ? `${tr("清理")} ${fmt(agedStats.size)}` : tr("清理")}
            </button>
          </>}
        >
          <div className="cleanup-aged-path" title={aged}>{aged}</div>
          <div className="field">
            <label>{tr("清理超过以下天数未访问的文件")}</label>
            <div className="seg cleanup-aged-options">
              {[30, 90, 180, 365].map((days) => (
                <button key={days} className={agedDays === days ? "on" : ""} disabled={agedBusy} onClick={() => loadStats(aged, days)}>
                  {days >= 365 ? tr("1 年") : `${days} ${tr("天")}`}
                </button>
              ))}
            </div>
          </div>
          {!agedStats
            ? <div className="banner gray cleanup-aged-banner"><i className="ti ti-loader lead spin" /><div className="bt">{tr("统计中…")}</div></div>
            : <div className="banner blue cleanup-aged-banner"><i className="ti ti-chart-bar lead" /><div className="bt">{agedDays >= 365 ? tr("1 年") : `${agedDays} ${tr("天")}`} {tr("未访问的文件共")} <b>{agedStats.count} {tr("个")} · {tr("约")} {fmt(agedStats.size)}</b>{locale === "zh-CN" ? "。" : ". "}{tr("删除后若再次使用会自动重新获取。")}</div></div>}
        </Modal>
      )}
    </>
  );
}
