import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useToast, Modal, useBusy, Loading } from "./ui";
import { Select } from "./Select";

type SdkVersion = { kind: string; version: string; vendor: string; path: string; current: boolean; arch?: string };
type SdkGroup = { kind: string; label: string; current_desc: string; versions: SdkVersion[] };
type DriveInfo = { letter: string; fixed: boolean };
type SourcePing = { host: string; ms: number | null };

export type DlSource = {
  id: string;
  name: string;
  host: string;
  urlFor: (v: string) => string;
};

type DlConfig = {
  title: string;
  subdir: string;
  folderName: (v: string) => string;
  sources: DlSource[];
  note?: string;
  versionsCmd?: string;
  staticVersions?: string[];
  defaultSource?: string;
};

const cmpVer = (a: string, b: string) => {
  const rank = (v: string) => v.includes("rc") ? 2 : v.includes("beta") || v.includes("milestone") ? 1 : v.includes("alpha") ? 0 : 3;
  const parts = (v: string) => {
    const lower = v.toLowerCase();
    const markers = ["alpha", "beta", "milestone", "rc"].map((m) => lower.indexOf(m)).filter((i) => i >= 0);
    const splitAt = markers.length ? Math.min(...markers) : v.length;
    const mainNums = (v.slice(0, splitAt).match(/\d+/g) ?? []).map((n) => +n || 0);
    const preNums = (v.slice(splitAt).match(/\d+/g) ?? []).map((n) => +n || 0);
    return { mainNums, preRank: rank(v.toLowerCase()), preNum: preNums.at(-1) ?? 0 };
  };
  const ap = parts(a), bp = parts(b);
  const an = ap.mainNums, bn = bp.mainNums;
  for (let i = 0; i < Math.max(an.length, bn.length, 3); i++) {
    const d = (an[i] || 0) - (bn[i] || 0);
    if (d) return d;
  }
  return (ap.preRank - bp.preRank) || (ap.preNum - bp.preNum);
};

const stableVersion = (v: string) => /^\d+(?:\.\d+){1,2}$/.test(v);
const versionLineKey = (v: string) => {
  const m = v.match(/^(\d+\.\d+)/);
  return m ? m[1] : v;
};

/** 通用版本管理：扫描磁盘 / 列已装版本 / 设默认（HOME 变量 + PATH，含用户/系统级）。
 *  用于 Maven / Gradle / Go 这类「手动多版本」生态（无 vendor）。 */
export function VersionManager({ kind, icon, cmd, envvar, download, onChanged }: {
  kind: string; icon: string; cmd: string; envvar: string; download: DlConfig;
  onChanged?: () => void;
}) {
  const toast = useToast();
  const runBusy = useBusy();
  const sourceKey = `stacker.${kind}.downloadSource`;
  const initialSource = () => {
    const saved = typeof localStorage !== "undefined" ? localStorage.getItem(sourceKey) : null;
    return saved && download.sources.some((s) => s.id === saved) ? saved : (download.defaultSource ?? download.sources[0]?.id ?? "");
  };

  const [grp, setGrp] = useState<SdkGroup | null>(null);
  const [loadErr, setLoadErr] = useState(false);
  const [scanned, setScanned] = useState<SdkVersion[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [sysConfigured, setSysConfigured] = useState(false);
  const [dlg, setDlg] = useState<SdkVersion | null>(null);
  const [scope, setScope] = useState<"user" | "system">("user");
  const [busy, setBusy] = useState(false);
  const [dlOpen, setDlOpen] = useState(false);
  const [appDir, setAppDir] = useState("");
  const [installRoot, setInstallRoot] = useState("");
  const [dlVersions, setDlVersions] = useState<string[] | null>(null);
  const [downloadSource, setDownloadSource] = useState(initialSource);
  const [pendingDownloadSource, setPendingDownloadSource] = useState(initialSource);
  const [sourcePings, setSourcePings] = useState<Record<string, number | null>>({});
  const [onlyStable, setOnlyStable] = useState(true);
  const [latestOnly, setLatestOnly] = useState(true);
  const [installSetDefault, setInstallSetDefault] = useState(true);
  const [installScope, setInstallScope] = useState<"user" | "system">("user");

  async function load() {
    const groups = await invoke<SdkGroup[]>("env_state");
    setGrp(groups.find((g) => g.kind === kind) ?? null);
    invoke<Record<string, boolean>>("env_system_info").then((m) => setSysConfigured(!!m[kind])).catch(() => {});
  }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => {
    load().catch(() => setLoadErr(true));
    invoke<string>("app_dir").then((d) => {
      setAppDir(d);
      setInstallRoot(`${d}\\${download.subdir}`);
    }).catch(() => setInstallRoot(`D:\\Environments\\${download.subdir}`));
  }, [kind]);

  function sourceName(id: string) {
    return download.sources.find((s) => s.id === id)?.name ?? id;
  }
  function defaultInstallRoot() {
    return appDir ? `${appDir}\\${download.subdir}` : `D:\\Environments\\${download.subdir}`;
  }
  const root = installRoot.trim() || defaultInstallRoot();
  const destFor = (v: string) => `${root}\\${download.folderName(v)}`;
  const urlFor = (v: string) => (download.sources.find((s) => s.id === downloadSource) ?? download.sources[0]).urlFor(v);

  const versions = scanned ?? grp?.versions ?? [];
  const current = versions.find((v) => v.current);

  function filteredVersions(list: string[]) {
    let rows = onlyStable ? list.filter(stableVersion) : list;
    if (latestOnly) {
      const best = new Map<string, string>();
      for (const v of rows) {
        const key = versionLineKey(v);
        const cur = best.get(key);
        if (!cur || cmpVer(v, cur) > 0) best.set(key, v);
      }
      rows = [...best.values()];
    }
    return rows.sort((a, b) => cmpVer(b, a));
  }

  async function fetchDownloadVersions(source = downloadSource) {
    if (download.versionsCmd) {
      const rows = await runBusy({
        title: `获取${download.title.replace(/^下载\s*/, "")}版本列表`,
        message: `正在从「${sourceName(source)}」读取该下载源实际提供的版本。`,
      }, () => invoke<string[]>(download.versionsCmd!, { source }));
      setDlVersions(rows);
    } else {
      setDlVersions(download.staticVersions ?? []);
    }
  }

  function openDownload() {
    setDlOpen(true);
    setDlVersions(null);
    setInstallRoot((cur) => cur.trim() ? cur : defaultInstallRoot());
    fetchDownloadVersions().catch((e) => {
      setDlVersions([]);
      toast("获取版本列表失败。请切换下载源或稍后重试。原因：" + e, "err");
    });
  }

  async function browseRoot() {
    const dir = await open({ directory: true, defaultPath: root });
    if (typeof dir === "string") setInstallRoot(dir);
  }

  function applyDownloadSource(v = pendingDownloadSource) {
    if (!download.sources.some((s) => s.id === v)) return;
    setDownloadSource(v);
    setPendingDownloadSource(v);
    localStorage.setItem(sourceKey, v);
    setDlVersions(null);
    toast(`已应用${download.title.replace(/^下载\s*/, "")}下载源：${sourceName(v)}`, "ok");
  }

  async function speedtestSources() {
    const hosts = [...new Set(download.sources.map((s) => s.host).filter(Boolean))];
    if (hosts.length === 0) {
      toast("没有可测速的下载源", "info");
      return;
    }
    try {
      const rows = await runBusy({
        title: `${download.title.replace(/^下载\s*/, "")}下载源测速`,
        message: "正在并行测试各下载源连接延迟；单个主机 1500ms 无响应算超时。",
      }, () => invoke<SourcePing[]>("speedtest_hosts", { hosts }));
      const byHost: Record<string, number | null> = {};
      rows.forEach((r) => { byHost[r.host] = r.ms; });
      const bySource: Record<string, number | null> = {};
      download.sources.forEach((s) => { bySource[s.id] = byHost[s.host] ?? null; });
      setSourcePings(bySource);
      const fastest = download.sources
        .map((s) => ({ ...s, ms: bySource[s.id] }))
        .filter((s): s is DlSource & { ms: number } => typeof s.ms === "number")
        .sort((a, b) => a.ms - b.ms)[0];
      if (fastest) {
        setPendingDownloadSource(fastest.id);
        toast(fastest.id === downloadSource
          ? `测速完成，${fastest.name} 已是当前下载源`
          : `测速完成，已预选 ${fastest.name}，点击「应用」后生效`, "ok");
      } else {
        toast("下载源测速均超时，保留当前下载源", "info");
      }
    } catch (e) {
      toast("下载源测速失败。请检查网络连接后重试。原因：" + e, "err");
    }
  }

  const cancelledRef = useRef(false);
  async function scan() {
    cancelledRef.current = false;
    setScanning(true);
    const drives = await invoke<DriveInfo[]>("list_drives").catch(() => [] as DriveInfo[]);
    const roots = drives.filter((d) => d.fixed).map((d) => d.letter + "\\");
    try {
      const r = await runBusy({
        title: `扫描磁盘上的 ${download.title.replace(/^下载\s*/, "")}`,
        message: "正在遍历各固定磁盘查找已安装版本，请稍候。完成会自动关闭；也可随时「取消扫描」。",
        progressEvent: "env-scan-progress",
        cancel: { label: "取消扫描", onCancel: cancelScan },
      }, () => invoke<Record<string, SdkVersion[]>>("env_scan", { roots }));
      if (cancelledRef.current) return;
      setScanned(r[kind] ?? []); await load();
      toast(`扫描完成，发现 ${(r[kind] ?? []).length} 个版本`, "ok");
    } catch (e) { toast("扫描磁盘失败。请确认磁盘可访问后重试。原因：" + e, "err"); }
    finally { setScanning(false); }
  }
  async function cancelScan() { cancelledRef.current = true; await invoke("env_cancel").catch(() => {}); }

  async function applyDefault() {
    if (!dlg) return;
    setBusy(true);
    try {
      const c = scope === "system" ? "env_set_default_system" : "env_set_default";
      await invoke(c, { kind, path: dlg.path, siblings: versions.map((v) => v.path) });
      toast("已设为默认" + (scope === "system" ? "（系统级）" : "（用户级）"), "ok");
      const picked = dlg; setDlg(null);
      await load();
      setScanned((s) => s ? s.map((v) => ({ ...v, current: v.path === picked.path })) : s);
    } catch (e) { toast("设置默认版本失败。请确认目标目录仍然存在后重试。原因：" + e, "err"); } finally { setBusy(false); }
  }

  async function installVersion(v: string) {
    const dest = destFor(v);
    setDlOpen(false);
    try {
      await runBusy({
        title: `安装 ${download.title.replace(/^下载\s*/, "")} ${v}`,
        message: `正在通过「${sourceName(downloadSource)}」下载安装文件，并解压到 ${dest}。`,
        progressEvent: "install-progress",
        cancel: { label: "取消安装", onCancel: () => { invoke("op_cancel").catch(() => {}); } },
      }, async () => {
        await invoke("installer_download", { url: urlFor(v), destDir: dest, stripTop: true });
        if (installSetDefault) {
          const c = installScope === "system" ? "env_set_default_system" : "env_set_default";
          await invoke(c, { kind, path: dest, siblings: versions.map((x) => x.path) });
        }
      });
      toast(installSetDefault ? `已安装 ${v} 并设为默认（${installScope === "system" ? "系统级" : "用户级"}）` : `已安装 ${v}`, "ok");
      await scan();
      onChanged?.();
    } catch (e) {
      toast("安装失败。请切换下载源或检查安装目录权限后重试。原因：" + e, "err");
    }
  }

  if (loadErr) return <div className="stub"><div className="si"><i className="ti ti-plug-x" /></div><h2>读取环境失败</h2><p>请在 Tauri 应用内运行（浏览器预览没有后端）。</p></div>;
  if (!grp) return <Loading text="正在读取已装版本…" />;

  const fastestSource = Object.entries(sourcePings)
    .filter(([, ms]) => typeof ms === "number")
    .sort((a, b) => (a[1] as number) - (b[1] as number))[0]?.[0] ?? null;
  const sourceDirty = pendingDownloadSource !== downloadSource;
  const sourceOptions = download.sources.map((s) => {
    const ms = sourcePings[s.id];
    const suffix = !(s.id in sourcePings)
      ? ""
      : ms === null ? " · 超时" : ` · ${ms}ms${s.id === fastestSource ? " · 最快" : ""}`;
    return { value: s.id, label: `${s.name}${suffix}` };
  });
  const visibleDlVersions = dlVersions ? filteredVersions(dlVersions) : null;

  return (
    <>
      <div className="srcrow" style={{ marginBottom: 10 }}>
        <span className="av st"><i className="ti ti-download" /></span>
        <div className="mt">
          <div className="t">{download.title.replace(/^下载\s*/, "")}下载源</div>
          <div className="s dim" title="用于获取运行时安装包。测速只会预选下载源，点击「应用」后生效。">用于获取安装包；测速后点击「应用」生效。</div>
        </div>
        <Select value={pendingDownloadSource} width={220} onChange={setPendingDownloadSource} options={sourceOptions} />
        <button className="gh sm" onClick={speedtestSources}><i className="ti ti-bolt" /> 测速</button>
        <button className="pr sm"
          title={sourceDirty ? `应用后安装将使用 ${sourceName(pendingDownloadSource)}` : `当前已应用：${sourceName(downloadSource)}`}
          onClick={() => applyDownloadSource()}>
          <i className="ti ti-check" /> 应用
        </button>
      </div>

      <div className="grouphd">
        <span className="gt"><i className={"ti " + icon} /> 版本 <span className="cnt">{versions.length} 个 · 手动多版本</span></span>
        <div className="ghr">
          {!scanning
            ? <button className="gh xs" onClick={scan}><i className="ti ti-refresh" /> 扫描磁盘</button>
            : <button className="gh xs" onClick={cancelScan}>取消扫描</button>}
          <button className="pr sm" onClick={openDownload}><i className="ti ti-download" /> 下载新版本</button>
        </div>
      </div>

      <div className="effbox">
        <div className="eh"><i className="ti ti-target" /> 生效情况 <span className="sub">依据当前默认配置</span></div>
        <div className="effrow"><span className="ek"><i className="ti ti-terminal-2" /> 命令 {cmd}</span>
          <span className="ev">{current ? <>版本 <b>{current.version}</b></> : "未配置默认"}</span>
          {current && <span className="bd g">生效中</span>}</div>
        <div className="effrow"><span className="ek"><i className="ti ti-variable" /> {envvar}</span>
          <span className="ev"><span className="mono">{current?.path ?? "—"}</span></span>
          {sysConfigured && <span className="bd w">含系统级</span>}</div>
      </div>

      {versions.length === 0 ? (
        <div className="stub"><div className="si"><i className="ti ti-package-off" /></div><h2>未检测到已装版本</h2>
          <p>点「扫描磁盘」发现已装版本，或「下载新版本」由 Stacker 直接安装。</p></div>
      ) : versions.map((v) => (
        <div className={"vrow" + (v.current ? " cur" : "")} key={v.path}>
          <span className="ver">{v.version}</span>
          <span className="meta">{v.path}</span>
          <div className="acts">
            {v.current && <span className="live"><i className="ti ti-circle-check" /> 生效中</span>}
            {v.current
              ? <button className="gh xs" title={`重新写入 ${envvar} / PATH 并刷新`} onClick={() => { setScope(sysConfigured ? "system" : "user"); setDlg(v); }}><i className="ti ti-refresh" /> 重新应用</button>
              : <button className="pr sm" onClick={() => { setScope(sysConfigured ? "system" : "user"); setDlg(v); }}>设为默认</button>}
          </div>
        </div>
      ))}

      {dlg && (
        <Modal wide title={`把默认切到 ${dlg.version}`} icon={icon} onClose={() => !busy && setDlg(null)}
          sub={<b style={{ color: "var(--tx)" }}>{envvar} 和 PATH 始终一起改、同级同步</b>}
          footer={<>
            <button className="gh sm" onClick={() => setDlg(null)} disabled={busy}>取消</button>
            <button className="pr" style={{ background: "#d97a1f" }} onClick={applyDefault} disabled={busy}>
              <i className="ti ti-shield-half" /> {busy ? "应用中…" : scope === "system" ? "应用（将触发 UAC 提权）" : "应用"}</button>
          </>}>
          <div className="field"><label>作用范围</label>
            <div className={"opt" + (scope === "user" ? " sel" : "")} onClick={() => setScope("user")}><span className="rd" />
              <div><div className="ot">仅当前用户 <span className="bd n" style={{ fontSize: 10 }}>免管理员</span></div>
                <div className="od">写 HKCU：用户级 {envvar} + 用户 PATH。无需 UAC 提权。</div></div></div>
            <div className={"opt" + (scope === "system" ? " sel" : "")} onClick={() => setScope("system")}><span className="rd" />
              <div><div className="ot"><i className="ti ti-shield-lock" style={{ color: "#f5a45a" }} /> 系统全局 <span className="bd w" style={{ fontSize: 10 }}>需管理员 · 需 UAC 提权</span></div>
                <div className="od">写 HKLM：所有用户生效。命令被系统 PATH 覆盖时选它。</div></div></div>
          </div>
          <div className="banner gray" style={{ margin: 0 }}><i className="ti ti-history lead" /><div className="bt">改动前自动备份 {envvar} 与 PATH，可在「历史」还原。</div></div>
        </Modal>
      )}

      {dlOpen && (
        <Modal title={download.title} icon="ti-download" onClose={() => setDlOpen(false)}
          sub={<div style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
            <label className="ck"><input type="checkbox" checked={onlyStable} onChange={(e) => setOnlyStable(e.target.checked)} /> 仅正式发布版</label>
            <label className="ck"><input type="checkbox" checked={latestOnly} onChange={(e) => setLatestOnly(e.target.checked)} /> 仅各版本最新</label>
            <label className="ck"><input type="checkbox" checked={installSetDefault} onChange={(e) => setInstallSetDefault(e.target.checked)} /> 安装后设为默认版本</label>
            <span style={{ color: "var(--mut)" }}>下载源：{sourceName(downloadSource)}</span>
          </div>}>
          <div className="field"><label>安装位置</label>
            <div className="row" style={{ gap: 8, display: "flex" }}>
              <input className="ip full" style={{ flex: 1 }} value={root} onChange={(e) => setInstallRoot(e.target.value)} />
              <button className="gh sm" onClick={browseRoot}><i className="ti ti-folder" /> 浏览…</button>
            </div>
            <div className="hint">每个版本会安装到此目录下的独立版本文件夹，默认位置保持当前规则。</div>
          </div>
          {installSetDefault && (
            <div className="field"><label>默认范围</label>
              <div className={"opt" + (installScope === "user" ? " sel" : "")} onClick={() => setInstallScope("user")}><span className="rd" />
                <div><div className="ot">仅当前用户 <span className="bd n" style={{ fontSize: 10 }}>免管理员</span></div>
                  <div className="od">安装后写入当前用户的 {envvar} 和 PATH。</div></div></div>
              <div className={"opt" + (installScope === "system" ? " sel" : "")} onClick={() => setInstallScope("system")}><span className="rd" />
                <div><div className="ot"><i className="ti ti-shield-lock" style={{ color: "#f5a45a" }} /> 系统全局 <span className="bd w" style={{ fontSize: 10 }}>需管理员 · 需 UAC 提权</span></div>
                  <div className="od">安装后写入系统级 {envvar} 和 PATH。</div></div></div>
            </div>
          )}
          {!visibleDlVersions ? <div style={{ color: "var(--mut)", fontSize: 13 }}>获取版本列表…</div>
            : <div style={{ maxHeight: 280, overflow: "auto", display: "flex", flexDirection: "column", gap: 5 }}>
              {visibleDlVersions.length === 0 && <div style={{ color: "var(--mut)", fontSize: 13, padding: 8 }}>当前下载源没有匹配的版本。</div>}
              {visibleDlVersions.map((v) => {
                const has = versions.some((x) => x.version === v);
                return (
                  <div className="vrow" key={v} style={{ margin: 0 }}>
                    <span className="ver">{v}</span>
                    <span className="meta">{destFor(v)}</span>
                    <div className="acts">{has
                      ? <span className="live"><i className="ti ti-circle-check" /> 已安装</span>
                      : <button className="gh xs" onClick={() => installVersion(v)}>安装</button>}</div>
                  </div>
                );
              })}
            </div>}
          {download.note ? <div className="banner gray" style={{ margin: 0 }}><i className="ti ti-info-circle lead" /><div className="bt">{download.note}</div></div> : null}
        </Modal>
      )}
    </>
  );
}
