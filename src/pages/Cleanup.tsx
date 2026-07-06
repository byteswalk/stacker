import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useToast, ConfirmModal, Modal } from "../ui";

type CacheItem = { id: string; name: string; path: string; size: number; category: string; icon: string; av: string };

function fmt(b: number) {
  if (b >= 1024 ** 3) return (b / 1024 ** 3).toFixed(1) + " GB";
  if (b >= 1024 ** 2) return (b / 1024 ** 2).toFixed(0) + " MB";
  if (b >= 1024) return (b / 1024).toFixed(0) + " KB";
  return b + " B";
}

export default function Cleanup() {
  const toast = useToast();
  const [items, setItems] = useState<CacheItem[] | null>(null);
  const [err, setErr] = useState(false);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [aged, setAged] = useState<string | null>(null);
  const [agedDays, setAgedDays] = useState(90);
  const [agedStats, setAgedStats] = useState<{ count: number; size: number } | null>(null);
  const [agedBusy, setAgedBusy] = useState(false);

  async function load() {
    const it = await invoke<CacheItem[]>("cleanup_scan");
    setItems(it);
    setSel(new Set(it.filter((x) => x.category === "safe").map((x) => x.path)));
  }
  useEffect(() => { load().catch(() => setErr(true)); }, []);

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

  if (err) return <div className="stub"><div className="si"><i className="ti ti-plug-x" /></div><h2>扫描失败</h2><p>请在 Tauri 应用内运行（浏览器预览没有后端）。</p></div>;
  if (!items) return <div className="stub"><p>扫描各生态缓存占用…</p></div>;

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
      <div className="clhero">
        <span className="clic"><i className="ti ti-database" /></span>
        <div className="clt">
          <div className="t1">可清理项共占用 {fmt(total)} · 可安全释放 <b>{fmt(safeTotal)}</b></div>
          <div className="t2">默认勾选纯缓存；JetBrains 历史版本、Windows 临时目录和谨慎项需手动选择。临时目录中被占用的文件会自动跳过。</div>
        </div>
        <button className="pr" disabled={busy || sel.size === 0} onClick={() => setConfirm([...sel])}><i className="ti ti-eraser" /> 清理所选（{fmt(selTotal)}）</button>
      </div>

      {items.length === 0 && (
        <div className="stub"><div className="si"><i className="ti ti-sparkles" /></div><h2>很干净</h2><p>没扫到可清理项。</p></div>
      )}
      {safe.length > 0 && <div className="seclabel"><i className="ti ti-shield-check" style={{ color: "#6bcf86" }} /> 可安全清理（纯缓存，删后会自动重新获取）</div>}
      {safe.map(row)}
      {history.length > 0 && <div className="seclabel"><i className="ti ti-code" style={{ color: "#e4b450" }} /> JetBrains IDE 历史版本（保留同产品最新版本）</div>}
      {history.map(row)}
      {temp.length > 0 && <div className="seclabel"><i className="ti ti-trash" style={{ color: "#e4b450" }} /> Windows 临时目录（超过 1 GB 才显示）</div>}
      {temp.map(row)}
      {cautious.length > 0 && <div className="seclabel"><i className="ti ti-alert-triangle" style={{ color: "#e4b450" }} /> 谨慎（非纯缓存 / 重下很慢）</div>}
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
          <div className="field"><label>清理多久没被访问过的文件</label>
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
