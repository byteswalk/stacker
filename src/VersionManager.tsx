import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "./invoke";
import { open } from "@tauri-apps/plugin-dialog";
import { useToast, Modal, useBusy, Loading, ErrorState } from "./ui";
import { Select } from "./Select";
import { TerminalBar } from "./TerminalBar";
import { EcoActions, type EcosystemId, type Shells, summaryLine } from "./EcoActions";
import { useNotifications } from "./notifications";

type SdkVersion = { kind: string; version: string; vendor: string; path: string; current: boolean; arch?: string; origin?: "managed" | "external" | "tool-bundled" | "project" | "unknown"; can_delete?: boolean };
type SdkGroup = { kind: string; label: string; current_desc: string; versions: SdkVersion[] };
type DriveInfo = { letter: string; fixed: boolean };
type SourcePing = { host: string; ms: number | null };
type CatalogTool = { id: string; mirrors: CatalogMirror[] };
type CatalogMirror = { id: string; name: string; url: string; host: string };

export type DlSource = {
  id: string;
  name: string;
  host: string;
  url: string;
};

type DlConfig = {
  title: string;
  subdir: string;
  folderName: (v: string) => string;
  sources: DlSource[];
  sourceToolId: string;
  urlFor: (source: DlSource, version: string) => string;
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
function initialBool(key: string, fallback: boolean) {
  const saved = typeof localStorage !== "undefined" ? localStorage.getItem(key) : null;
  return saved === null ? fallback : saved === "true";
}

/** 通用版本管理：扫描磁盘 / 列已装版本 / 设默认（HOME 变量 + PATH，含用户/系统级）。
 *  用于 Maven / Gradle / Go 这类「手动多版本」生态（无 vendor）。 */
export function VersionManager({ kind, icon, cmd, envvar, download, onChanged, onDownloadSourceChanged }: {
  kind: string; icon: string; cmd: string; envvar: string; download: DlConfig;
  onChanged?: () => void;
  onDownloadSourceChanged?: (source: string) => void;
}) {
  const toast = useToast();
  const runBusy = useBusy();
  const notices = useNotifications();
  const sourceKey = `stacker.${kind}.downloadSource`;
  const initialSource = () => {
    const saved = typeof localStorage !== "undefined" ? localStorage.getItem(sourceKey) : null;
    return saved || download.defaultSource || download.sources[0]?.id || "";
  };

  const [grp, setGrp] = useState<SdkGroup | null>(null);
  const [loadErr, setLoadErr] = useState(false);
  const [scanned, setScanned] = useState<SdkVersion[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [excludeToolBundled, setExcludeToolBundled] = useState(true);
  const [sysConfigured, setSysConfigured] = useState(false);
  const [dlg, setDlg] = useState<SdkVersion | null>(null);
  const [removeDlg, setRemoveDlg] = useState<SdkVersion | null>(null);
  const [scope, setScope] = useState<"user" | "system">("user");
  const [busy, setBusy] = useState(false);
  const [dlOpen, setDlOpen] = useState(false);
  const [appDir, setAppDir] = useState("");
  const [installRoot, setInstallRoot] = useState("");
  const [dlVersions, setDlVersions] = useState<string[] | null>(null);
  const [downloadSources, setDownloadSources] = useState<DlSource[]>(download.sources);
  const [downloadSource, setDownloadSource] = useState(initialSource);
  const [pendingDownloadSource, setPendingDownloadSource] = useState(initialSource);
  const [sourcePings, setSourcePings] = useState<Record<string, number | null>>({});
  const [onlyStable, setOnlyStable] = useState(() => initialBool(`stacker.${kind}.install.onlyStable`, true));
  const [latestOnly, setLatestOnly] = useState(() => initialBool(`stacker.${kind}.install.latestOnly`, true));
  const [installSetDefault, setInstallSetDefault] = useState(true);
  const [installScope, setInstallScope] = useState<"user" | "system">("user");
  const [shells, setShells] = useState<Shells>({ powershell: true, gitbash: false, cmd: true });

  const load = useCallback(async () => {
    const groups = await invoke<SdkGroup[]>("env_state");
    setGrp(groups.find((g) => g.kind === kind) ?? null);
    invoke<Record<string, boolean>>("env_system_info").then((m) => setSysConfigured(!!m[kind])).catch(() => {});
  }, [kind]);

  useEffect(() => {
    load().catch(() => setLoadErr(true));
  }, [load]);

  useEffect(() => {
    invoke<Shells>("shells_available").then(setShells).catch(() => {});
  }, []);

  useEffect(() => {
    invoke<string>("app_dir").then((d) => {
      setAppDir(d);
      setInstallRoot(`${d}\\${download.subdir}`);
    }).catch(() => setInstallRoot(`D:\\Environments\\${download.subdir}`));
  }, [download.subdir]);

  useEffect(() => {
    invoke<CatalogTool[]>("list_sources").then((tools) => {
      const mirrors = tools.find((tool) => tool.id === download.sourceToolId)?.mirrors;
      const next = mirrors?.length ? mirrors : download.sources;
      setDownloadSources(next);
      const fallback = next.find((source) => source.id === "official")?.id ?? next[0]?.id ?? "";
      setDownloadSource((current) => {
        if (next.some((source) => source.id === current)) return current;
        localStorage.setItem(sourceKey, fallback);
        setPendingDownloadSource(fallback);
        if (current) toast(`原 ${download.title.replace(/^下载\s*/, "")} 下载源已不在源清单中，已恢复为官方源`, "info");
        return fallback;
      });
      setPendingDownloadSource((current) => next.some((source) => source.id === current) ? current : fallback);
    }).catch(() => undefined);
  }, [download.sourceToolId, download.sources, download.title, sourceKey, toast]);

  function sourceName(id: string) {
    return downloadSources.find((s) => s.id === id)?.name ?? id;
  }
  function defaultInstallRoot() {
    return appDir ? `${appDir}\\${download.subdir}` : `D:\\Environments\\${download.subdir}`;
  }
  const root = installRoot.trim() || defaultInstallRoot();
  const destFor = (v: string) => `${root}\\${download.folderName(v)}`;
  const urlFor = (v: string) => {
    const source = downloadSources.find((item) => item.id === downloadSource) ?? downloadSources[0];
    if (!source) throw new Error("当前没有可用的下载源");
    return download.urlFor(source, v);
  };

  const versions = scanned ?? grp?.versions ?? [];
  const current = versions.find((v) => v.current);
  const rawUpdateHint = notices.ecosystemUpdates.find((item) => item.id === kind);
  const updateHint = rawUpdateHint && current && cmpVer(current.version, rawUpdateHint.latest) >= 0 ? undefined : rawUpdateHint;
  const label = download.title.replace(/^下载\s*/, "");
  const versionSectionLabel = kind === "maven" || kind === "gradle" ? "构建工具版本" : "运行时版本";
  const updateTitle = updateHint ? `发现新版本：当前 ${updateHint.current}，最新 ${updateHint.latest}，下载源 ${sourceName(updateHint.source)}` : undefined;
  const summary = [
    `## ${label} 环境摘要`,
    "",
    summaryLine("命令", cmd),
    summaryLine("默认版本", current?.version ?? "未配置"),
    summaryLine(envvar, current?.path ?? "未配置"),
    summaryLine("下载源", sourceName(downloadSource)),
    summaryLine("已安装版本", versions.map((v) => v.version).join(", ") || "无"),
    summaryLine("配置范围", sysConfigured ? "包含系统级配置" : "当前用户配置"),
    "",
    "## 给 AI 的使用说明",
    `- 使用 ${cmd} 前，先执行版本检查命令确认当前终端可用。`,
    `- 如需切换默认版本，请通过本页设置 ${envvar} 与 PATH，不要直接修改系统级配置。`,
  ].join("\n");

  function refreshNoticeState(reason: string) {
    void notices.checkNow(reason).catch(() => undefined);
  }

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
      }, () => invoke<string[]>(download.versionsCmd!, {
        source,
        sourceUrl: downloadSources.find((item) => item.id === source)?.url ?? "",
      }));
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
    if (!downloadSources.some((s) => s.id === v)) return;
    setDownloadSource(v);
    setPendingDownloadSource(v);
    localStorage.setItem(sourceKey, v);
    onDownloadSourceChanged?.(v);
    setDlVersions(null);
    toast(`已应用${download.title.replace(/^下载\s*/, "")}下载源：${sourceName(v)}`, "ok");
    refreshNoticeState(`${kind}-download-source`);
  }

  async function speedtestSources() {
    const hosts = [...new Set(downloadSources.map((s) => s.host).filter(Boolean))];
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
      downloadSources.forEach((s) => { bySource[s.id] = byHost[s.host] ?? null; });
      setSourcePings(bySource);
      const fastest = downloadSources
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
        message: "正在遍历本机固定磁盘并识别已安装版本。可随时取消扫描，已有列表不会被覆盖。",
        progressEvent: "env-scan-progress",
        cancel: { label: "取消扫描", onCancel: cancelScan },
      }, async () => {
        const result = await invoke<Record<string, SdkVersion[]>>("env_scan", { roots, excludeToolBundled, kinds: [kind] });
        if (!cancelledRef.current) await load();
        return result;
      });
      if (cancelledRef.current) return;
      setScanned(r[kind] ?? []);
      toast(`扫描完成，发现 ${(r[kind] ?? []).length} 个版本`, "ok");
    } catch (e) { toast("扫描磁盘失败。请确认磁盘可访问后重试。原因：" + e, "err"); }
    finally { setScanning(false); }
  }
  async function cancelScan() { cancelledRef.current = true; await invoke("env_cancel").catch(() => {}); }

  async function refreshState() {
    try {
      await runBusy({ title: `刷新 ${label} 状态`, message: `正在重新检测 ${cmd} 命令、默认版本和已登记目录。` }, async () => {
        setScanned(null);
        await load();
      });
      toast(`${label} 状态已刷新`, "ok");
    } catch (error) {
      toast(`刷新 ${label} 状态失败：${error}`, "err");
    }
  }

  async function removeManagedVersion() {
    if (!removeDlg) return;
    const target = removeDlg;
    setBusy(true);
    try {
      await runBusy({
        title: `删除 ${label} ${target.version}`,
        message: target.current
          ? `正在删除安装目录并清除指向该版本的 ${envvar} 与 PATH；如包含系统级配置，Windows 将请求管理员授权。`
          : "正在删除由 Stacker 安装的版本目录。",
      }, async () => {
        await invoke("env_remove_managed", { kind, path: target.path });
        setScanned(null);
        await load();
      });
      setRemoveDlg(null);
      toast(`${label} ${target.version} 已删除`, "ok");
      refreshNoticeState(`${kind}-remove`);
    } catch (error) {
      toast(`删除 ${label} ${target.version} 失败：${error}`, "err");
    } finally {
      setBusy(false);
    }
  }

  async function applyDefault() {
    if (!dlg) return;
    const picked = dlg;
    setBusy(true);
    try {
      const c = scope === "system" ? "env_set_default_system" : "env_set_default";
      await runBusy({
        title: `设置默认${download.title.replace(/^下载\s*/, "")}版本`,
        message: `正在写入${scope === "system" ? "系统级" : "当前用户"} ${envvar} 与 PATH，并验证新配置。`,
      }, async () => {
        await invoke(c, { kind, path: picked.path, siblings: versions.map((v) => v.path) });
        await load();
      });
      setScanned((s) => s ? s.map((v) => ({ ...v, current: v.path === picked.path })) : s);
      setDlg(null);
      toast("已设为默认" + (scope === "system" ? "（系统级）" : "（用户级）"), "ok");
      refreshNoticeState(`${kind}-default`);
    } catch (e) { toast("设置默认版本失败。请确认目标目录仍然存在后重试。原因：" + e, "err"); } finally { setBusy(false); }
  }

  async function installVersion(v: string) {
    const dest = destFor(v);
    setDlOpen(false);
    let cancelled = false;
    try {
      await runBusy({
        title: `安装 ${download.title.replace(/^下载\s*/, "")} ${v}`,
        message: `正在通过「${sourceName(downloadSource)}」下载安装文件，并解压到 ${dest}。`,
        progressEvent: "install-progress",
        cancel: {
          label: "取消安装",
          onCancel: () => {
            cancelled = true;
            invoke("op_cancel").catch(() => {});
          },
        },
      }, async () => {
        await invoke("installer_download", { url: urlFor(v), destDir: dest, stripTop: true });
        await invoke("env_register_install", { kind, path: dest });
        if (installSetDefault) {
          const c = installScope === "system" ? "env_set_default_system" : "env_set_default";
          await invoke(c, { kind, path: dest, siblings: versions.map((x) => x.path) });
        }
        await load();
      });
      setScanned((previous) => {
        const rows = previous ?? versions;
        const installed: SdkVersion = {
          kind,
          version: v,
          vendor: "",
          path: dest,
          current: installSetDefault,
          origin: "managed",
          can_delete: true,
        };
        return [
          ...rows
            .filter((item) => item.path.toLowerCase() !== dest.toLowerCase())
            .map((item) => installSetDefault ? { ...item, current: false } : item),
          installed,
        ];
      });
      onChanged?.();
      toast(installSetDefault ? `已安装 ${v} 并设为默认（${installScope === "system" ? "系统级" : "用户级"}）` : `已安装 ${v}`, "ok");
      refreshNoticeState(`${kind}-install`);
    } catch (e) {
      const detail = String(e);
      if (cancelled || detail.includes("已取消")) {
        toast("已取消安装", "info");
      } else {
        toast("安装失败。请切换下载源或检查安装目录权限后重试。原因：" + detail, "err");
      }
    }
  }

  if (loadErr) return <ErrorState title={`暂时无法读取 ${download.title.replace(/^下载\s*/, "")} 环境`} description="请确认相关进程未被安全软件拦截，然后重试。" onRetry={async () => { await load(); setLoadErr(false); }} />;
  const grpLoading = !grp;

  const fastestSource = Object.entries(sourcePings)
    .filter(([, ms]) => typeof ms === "number")
    .sort((a, b) => (a[1] as number) - (b[1] as number))[0]?.[0] ?? null;
  const sourceDirty = pendingDownloadSource !== downloadSource;
  const sourceOptions = downloadSources.map((s) => {
    const ms = sourcePings[s.id];
    const suffix = !(s.id in sourcePings)
      ? ""
      : ms === null ? " · 超时" : ` · ${ms}ms${s.id === fastestSource ? " · 最快" : ""}`;
    return { value: s.id, label: `${s.name}${suffix}` };
  });
  const visibleDlVersions = dlVersions ? filteredVersions(dlVersions) : null;

  return (
    <>
      {grpLoading ? (
        <Loading text={`正在检测 ${label} 命令、默认版本和已安装目录…`} />
      ) : (
        <TerminalBar
          avail={shells}
          ecosystem={kind as EcosystemId}
          tip={`${label} 命令通过 ${envvar} 与 PATH 生效。可打开终端验证当前版本，或复制摘要给 AI。`}
          action={<EcoActions ecosystem={kind as EcosystemId} shells={shells} summary={summary} />}
        />
      )}

      <div className="srcrow" style={{ marginBottom: 10 }}>
        <span className="av st"><i className="ti ti-download" /></span>
        <div className="mt">
        <div className="t">{download.title.replace(/^下载\s*/, "")}下载源 {updateHint && <span className="bd r update-badge" title={updateTitle}>发现新版本</span>}</div>
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
        <span className="gt"><i className={"ti " + icon} /> {versionSectionLabel} <span className="cnt">{grpLoading ? "检测中" : `${versions.length} 个`}</span></span>
        <div className="ghr">
          <label className="ck" style={{ fontSize: 11.5 }} title="排除 IDE、编辑器和其他开发工具随附的版本。">
            <input type="checkbox" checked={excludeToolBundled} disabled={scanning} onChange={(event) => setExcludeToolBundled(event.target.checked)} /> 排除工具自带
          </label>
          <button className="gh xs" disabled={scanning} onClick={refreshState}><i className="ti ti-refresh" /> 刷新状态</button>
          {!scanning
            ? <button className="gh xs" onClick={scan}><i className="ti ti-scan" /> 扫描本机</button>
            : <button className="gh xs" onClick={cancelScan}>取消扫描</button>}
          <button className="pr sm" onClick={openDownload}><i className="ti ti-download" /> 安装新版本</button>
        </div>
      </div>

      <div className="effbox">
        <div className="eh"><i className="ti ti-target" /> 生效情况 <span className="sub">依据当前默认配置</span></div>
        <div className="effrow"><span className="ek"><i className="ti ti-terminal-2" /> 命令 {cmd}</span>
          <span className="ev">{grpLoading ? "检测中…" : current ? <>版本 <b>{current.version}</b></> : "未配置默认"}</span>
          {current && <span className="bd g">生效中</span>}</div>
        <div className="effrow"><span className="ek"><i className="ti ti-variable" /> {envvar}</span>
          <span className="ev"><span className="mono">{grpLoading ? "检测中…" : current?.path ?? "—"}</span></span>
          {sysConfigured && <span className="bd w">含系统级</span>}</div>
      </div>

      {grpLoading ? (
        <Loading text={`正在读取已安装的 ${label} 版本…`} />
      ) : versions.length === 0 ? (
        <div className="stub"><div className="si"><i className="ti ti-package-off" /></div><h2>未检测到已装版本</h2>
          <p>可扫描磁盘识别已有安装，也可由 Stacker 下载并安装新版本。</p></div>
      ) : versions.map((v) => (
        <div className={"vrow" + (v.current ? " cur" : "")} key={v.path}>
          <span className="ver">{v.version}</span>
          <span className="meta">{v.path}</span>
          <div className="acts">
            {v.origin === "managed" && <span className="bd g">Stacker 安装</span>}
            {v.origin === "tool-bundled" && <span className="bd n">工具自带</span>}
            {v.origin === "project" && <span className="bd n">项目自带</span>}
            {v.current && <span className="live"><i className="ti ti-circle-check" /> 生效中</span>}
            {v.current
              ? <button className="gh xs" title={`重新写入 ${envvar} / PATH 并刷新`} onClick={() => { setScope(sysConfigured ? "system" : "user"); setDlg(v); }}><i className="ti ti-refresh" /> 重新应用</button>
              : <button className="pr sm" onClick={() => { setScope(sysConfigured ? "system" : "user"); setDlg(v); }}>设为默认</button>}
            {v.can_delete && <button className="gh xs danger" title="删除此版本" onClick={() => setRemoveDlg(v)}><i className="ti ti-trash" /></button>}
          </div>
        </div>
      ))}

      {dlg && (
        <Modal wide title={`设置默认版本 ${dlg.version}`} icon={icon} onClose={() => !busy && setDlg(null)}
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

      {removeDlg && (
        <Modal title={`删除 ${label} ${removeDlg.version}`} icon="ti-trash" onClose={() => !busy && setRemoveDlg(null)}
          footer={<>
            <button className="gh sm" disabled={busy} onClick={() => setRemoveDlg(null)}>取消</button>
            <button className="danger sm" disabled={busy} onClick={removeManagedVersion}><i className="ti ti-trash" /> {busy ? "删除中…" : "确认删除"}</button>
          </>}>
          <div className="banner red" style={{ margin: 0 }}>
            <i className="ti ti-alert-triangle lead" />
            <div className="bt">将永久删除 <span className="mono">{removeDlg.path}</span>。{removeDlg.current ? `该版本当前生效，相关 ${envvar} 与 PATH 配置也会一并清除。` : "此操作不可撤销。"}</div>
          </div>
        </Modal>
      )}

      {dlOpen && (
        <Modal title={download.title} icon="ti-download" onClose={() => setDlOpen(false)}
          sub={<div style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
            <label className="ck"><input type="checkbox" checked={onlyStable} onChange={(e) => { const next = e.target.checked; setOnlyStable(next); localStorage.setItem(`stacker.${kind}.install.onlyStable`, String(next)); }} /> 仅正式发布版</label>
            <label className="ck"><input type="checkbox" checked={latestOnly} onChange={(e) => { const next = e.target.checked; setLatestOnly(next); localStorage.setItem(`stacker.${kind}.install.latestOnly`, String(next)); }} /> 仅各版本最新</label>
            <label className="ck"><input type="checkbox" checked={installSetDefault} onChange={(e) => setInstallSetDefault(e.target.checked)} /> 安装后设为默认版本</label>
            <span style={{ color: "var(--mut)" }}>下载源：{sourceName(downloadSource)}</span>
          </div>}>
          <div className="field"><label>安装位置</label>
            <div className="row" style={{ gap: 8, display: "flex" }}>
              <input className="ip full" style={{ flex: 1 }} value={root} onChange={(e) => setInstallRoot(e.target.value)} />
              <button className="gh sm" onClick={browseRoot}><i className="ti ti-folder" /> 浏览…</button>
            </div>
            <div className="hint">每个版本使用独立子目录，避免不同版本互相覆盖。</div>
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
