import { useEffect, useState } from "react";
import { invoke } from "../invoke";
import { useToast, ConfirmModal, Modal, ErrorState } from "../ui";
import { useNotifications } from "../notifications";

type CacheItem = { id: string; name: string; path: string; size: number; category: string; icon: string; av: string };

type CleanupCache = {
  items: CacheItem[] | null;
  selected: string[];
  scanning: boolean;
  err: boolean;
};
const CLEANUP_INITIAL: CleanupCache = { items: null, selected: [], scanning: false, err: false };
let cleanupCache: CleanupCache = CLEANUP_INITIAL;
let cleanupRun: Promise<void> | null = null;
const cleanupListeners = new Set<(s: CleanupCache) => void>();

function publishCleanup(next: Partial<CleanupCache>) {
  cleanupCache = { ...cleanupCache, ...next };
  cleanupListeners.forEach((fn) => fn(cleanupCache));
}

function subscribeCleanup(fn: (s: CleanupCache) => void) {
  cleanupListeners.add(fn);
  return () => { cleanupListeners.delete(fn); };
}

function runCleanupScan() {
  if (cleanupRun) return cleanupRun;
  publishCleanup({ scanning: true });
  cleanupRun = (async () => {
    try {
      const it = await invoke<CacheItem[]>("cleanup_scan");
      publishCleanup({
        items: it,
        selected: it.filter((x) => x.category === "safe").map((x) => x.path),
        err: false,
      });
    } catch (e) {
      publishCleanup({ err: true });
      throw e;
    } finally {
      publishCleanup({ scanning: false });
      cleanupRun = null;
    }
  })();
  return cleanupRun;
}

function fmt(b: number) {
  if (b >= 1024 ** 3) return (b / 1024 ** 3).toFixed(1) + " GB";
  if (b >= 1024 ** 2) return (b / 1024 ** 2).toFixed(0) + " MB";
  if (b >= 1024) return (b / 1024).toFixed(0) + " KB";
  return b + " B";
}

export default function Cleanup() {
  const toast = useToast();
  const notices = useNotifications();
  const [items, setItems] = useState<CacheItem[] | null>(cleanupCache.items);
  const [err, setErr] = useState(cleanupCache.err);
  const [sel, setSel] = useState<Set<string>>(new Set(cleanupCache.selected));
  const [confirm, setConfirm] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(cleanupCache.scanning);
  const [aged, setAged] = useState<string | null>(null);
  const [agedDays, setAgedDays] = useState(90);
  const [agedStats, setAgedStats] = useState<{ count: number; size: number } | null>(null);
  const [agedBusy, setAgedBusy] = useState(false);

  useEffect(() => subscribeCleanup((s) => {
    setItems(s.items);
    setErr(s.err);
    setScanning(s.scanning);
    setSel(new Set(s.selected));
  }), []);

  async function load() { return runCleanupScan(); }
  async function rescan() {
    const wasScanned = items !== null;
    try {
      await load();
      toast(wasScanned ? "扫描结果已刷新" : "扫描完成", "ok");
    } catch (e) {
      toast("磁盘扫描失败：" + e, "err");
    }
  }

  async function del(paths: string[]) {
    setBusy(true);
    try { const freed = await invoke<number>("cleanup_delete", { paths }); toast("已释放 " + fmt(freed), "ok"); setConfirm(null); await load(); }
    catch (e) { toast("清理失败：" + e, "err"); } finally { setBusy(false); }
  }
  async function loadStats(path: string, days: number) {
    setAgedDays(days); setAgedStats(null);
    try { setAgedStats(await invoke<{ count: number; size: number }>("cleanup_aged_stats", { path, days })); }
    catch (e) { toast("统计失败：" + e, "err"); }
  }
  function openAged(path: string) { setAged(path); loadStats(path, 90); }
  async function delAged() {
    if (!aged) return;
    setAgedBusy(true);
    try { const freed = await invoke<number>("cleanup_delete_aged", { path: aged, days: agedDays }); toast("已释放 " + fmt(freed), "ok"); setAged(null); await load(); }
    catch (e) { toast("清理失败：" + e, "err"); } finally { setAgedBusy(false); }
  }

  if (err) return <ErrorState title="磁盘扫描未完成" description="部分目录可能无法访问。请关闭占用相关目录的程序后重试。" onRetry={rescan} />;
  if (!items) return (
    <div className={"clhero" + (scanning ? " scanning trace-card" : "")}>
      {scanning && <span className="border-runner" aria-hidden="true" />}
      <span className="clic"><i className={"ti " + (scanning ? "ti-database-search" : "ti-database")} /></span>
      <div className="clt">
        <div className="t1">{scanning ? "正在扫描可清理项" : "磁盘清理"}</div>
        <div className="t2">{scanning ? "正在统计开发工具缓存、历史版本和 Windows 临时目录占用…" : notices.cleanupBytes > 0 ? `后台估算可安全清理约 ${fmt(notices.cleanupBytes)}。点击「开始扫描」查看完整清理项。` : "点击「开始扫描」后，Stacker 将统计开发工具缓存、历史版本和 Windows 临时目录占用。"}</div>
      </div>
      <button className="gh" disabled={scanning} onClick={rescan}>
        <i className={"ti " + (scanning ? "ti-loader spin" : "ti-player-play")} /> {scanning ? "扫描中…" : "开始扫描"}
      </button>
    </div>
  );

  const safe = items.filter((x) => x.category === "safe");
  const history = items.filter((x) => x.category === "history");
  const temp = items.filter((x) => x.category === "temp");
  const cautious = items.filter((x) => x.category === "cautious");
  const total = items.reduce((a, x) => a + x.size, 0);
  const safeTotal = safe.reduce((a, x) => a + x.size, 0);
  const selItems = items.filter((x) => sel.has(x.path));
  const selTotal = selItems.reduce((a, x) => a + x.size, 0);

  function toggle(p: string) {
    setSel((s) => {
      const n = new Set(s);
      if (n.has(p)) n.delete(p);
      else n.add(p);
      publishCleanup({ selected: [...n] });
      return n;
    });
  }

  function categoryBadge(x: CacheItem) {
    if (x.category === "history") return <span className="bd w">历史版本</span>;
    if (x.category === "temp") return <span className="bd w">临时文件</span>;
    if (x.category === "cautious") return <span className="bd w">谨慎</span>;
    return null;
  }

  function categoryHint(x: CacheItem) {
    if (x.category === "history") return "手动清理";
    if (x.category === "temp") return "可清";
    if (x.category === "cautious") return "谨慎";
    return "可清";
  }

  function rowStyle(x: CacheItem) {
    if (x.category === "safe") return undefined;
    return { boxShadow: "inset 3px 0 0 var(--amber)", borderColor: "rgba(228,180,80,.3)" };
  }

  function row(x: CacheItem) {
    const agedClean = x.category === "cautious";
    return (
      <div className="clrow" key={x.path} style={rowStyle(x)}>
        <input type="checkbox" className="ck2" checked={sel.has(x.path)} onChange={() => toggle(x.path)} />
        <span className={"av " + x.av}><i className={"ti " + x.icon} /></span>
        <div className="ct2"><div className="ch">{x.name} {categoryBadge(x)}</div><div className="cs">{x.path}</div></div>
        <div className="csz"><div className="big">{fmt(x.size)}</div><div className="small">{categoryHint(x)}</div></div>
        <button className="gh sm" disabled={busy} onClick={() => agedClean ? openAged(x.path) : setConfirm([x.path])}>{agedClean ? "智能清理" : "清理"}</button>
      </div>
    );
  }

  return (
    <>
      <div className={"clhero" + (scanning ? " scanning trace-card" : "")}>
        {scanning && <span className="border-runner" aria-hidden="true" />}
        <span className="clic"><i className="ti ti-database" /></span>
        <div className="clt">
          <div className="t1">可清理项共占用 {fmt(total)} · 可安全释放 <b>{fmt(safeTotal)}</b></div>
          <div className="t2">默认勾选纯缓存；JetBrains 历史版本、Windows 临时目录和谨慎项需手动选择。临时目录中被占用的文件会自动跳过。</div>
        </div>
        <div className="ghr">
          <button className="gh" disabled={busy || scanning} onClick={rescan}>
            <i className={"ti " + (scanning ? "ti-loader spin" : "ti-player-play")} /> {scanning ? "扫描中…" : "开始扫描"}
          </button>
          <button className="pr" disabled={busy || scanning || sel.size === 0} onClick={() => setConfirm([...sel])}><i className="ti ti-eraser" /> 清理所选（{fmt(selTotal)}）</button>
        </div>
      </div>
      {items.length === 0 && (
        <div className="stub"><div className="si"><i className="ti ti-circle-check" /></div><h2>未发现可清理项</h2><p>当前扫描范围内没有达到显示条件的缓存、历史版本或临时文件。</p></div>
      )}
      {safe.length > 0 && <div className="seclabel"><i className="ti ti-shield-check" style={{ color: "#6bcf86" }} /> 可安全清理（纯缓存，删后会自动重新获取）</div>}
      {safe.map(row)}
      {history.length > 0 && <div className="seclabel"><i className="ti ti-code" style={{ color: "#e4b450" }} /> JetBrains IDE 历史版本（保留同产品最新版本）</div>}
      {history.map(row)}
      {temp.length > 0 && <div className="seclabel"><i className="ti ti-trash" style={{ color: "#e4b450" }} /> Windows 临时目录（超过 1 GB 才显示）</div>}
      {temp.map(row)}
      {cautious.length > 0 && <div className="seclabel"><i className="ti ti-alert-triangle" style={{ color: "#e4b450" }} /> 谨慎清理（重新下载可能耗时较长）</div>}
      {cautious.map(row)}

      {confirm && (
        <ConfirmModal title="确认清理" icon="ti-eraser"
          message={<>将清理 <b style={{ color: "var(--tx)" }}>{confirm.length} 项</b>，预计释放 <b style={{ color: "#6bcf86" }}>{fmt(items.filter((x) => confirm.includes(x.path)).reduce((a, x) => a + x.size, 0))}</b>。<br />缓存和临时目录会清理目录内容；JetBrains 历史版本会删除旧版本目录；被系统占用的临时文件会自动跳过。</>}
          confirmLabel={busy ? "清理中…" : "清理"} busy={busy} onConfirm={() => del(confirm)} onClose={() => setConfirm(null)} />
      )}

      {aged && (
        <Modal wide title="智能清理（按未访问时长）" icon="ti-clock" onClose={() => !agedBusy && setAged(null)}
          footer={<>
            <button className="gh sm" disabled={agedBusy} onClick={() => setAged(null)}>取消</button>
            <button className="pr sm" disabled={agedBusy || !agedStats || agedStats.count === 0} onClick={delAged}>
              <i className="ti ti-eraser" /> {agedBusy ? "清理中…" : agedStats ? `清理 ${fmt(agedStats.size)}` : "清理"}</button>
          </>}>
          <div style={{ fontSize: 12, color: "var(--mut)", fontFamily: "var(--font-mono)", wordBreak: "break-all" }}>{aged}</div>
          <div className="field"><label>清理超过以下天数未访问的文件</label>
            <div className="seg" style={{ alignSelf: "flex-start" }}>
              {[30, 90, 180, 365].map((d) => <button key={d} className={agedDays === d ? "on" : ""} disabled={agedBusy} onClick={() => loadStats(aged, d)}>{d >= 365 ? "1 年" : d + " 天"}</button>)}</div></div>
          {!agedStats
            ? <div className="banner gray" style={{ margin: 0 }}><i className="ti ti-loader lead" /><div className="bt">统计中…</div></div>
            : <div className="banner blue" style={{ margin: 0 }}><i className="ti ti-chart-bar lead" /><div className="bt">{agedDays >= 365 ? "1 年" : agedDays + " 天"}未访问的文件共 <b>{agedStats.count} 个 · 约 {fmt(agedStats.size)}</b>。删除后若再用到会自动重新获取。</div></div>}
        </Modal>
      )}
    </>
  );
}
