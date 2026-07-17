import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { open } from "@tauri-apps/plugin-dialog";
import { useToast, Modal, ConfirmModal, useBusy, Loading, ErrorState, operationWasCancelled } from "../ui";
import { SourcesPanel } from "../SourcesPanel";
import { TerminalBar } from "../TerminalBar";
import { Select } from "../Select";
import { summaryLine } from "../EcoActions";
import { useNotifications } from "../notifications";

type PyVer = { version: string; is_default: boolean };
type PyenvStatus = { installed: boolean; pyenv_version: string | null; versions: PyVer[]; default: string | null; has_conda: boolean };
type Shells = { powershell: boolean; gitbash: boolean; cmd: boolean };
type Mirror = { id: string; name: string; url: string; host: string };
type ToolState = { id: string; name: string; mirrors: Mirror[] };
type PyenvSourcePing = { id: string; name: string; ms: number | null };
type DriveInfo = { letter: string; fixed: boolean };
type ScannedRuntime = { version: string; path: string; origin?: string; current: boolean };

const PY_RUNTIME_TOOL_ID = "python-runtime";
const PY_SOURCE_KEY = "stacker.python.downloadSource";
const PY_FILTER_KEYS = {
  onlyStable: "stacker.python.install.onlyStable",
  latestOnly: "stacker.python.install.latestOnly",
};
const FALLBACK_DOWNLOAD_SOURCES: Mirror[] = [{ id: "official", name: "官方", url: "", host: "www.python.org" }];

function initialDownloadSource() {
  const saved = typeof localStorage !== "undefined" ? localStorage.getItem(PY_SOURCE_KEY) : null;
  return saved || "official";
}
function initialBool(key: string, fallback: boolean) {
  const saved = typeof localStorage !== "undefined" ? localStorage.getItem(key) : null;
  return saved === null ? fallback : saved === "true";
}

const cmpVer = (a: string, b: string) => {
  const pa = a.split("."), pb = b.split(".");
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) { const d = (+pa[i] || 0) - (+pb[i] || 0); if (d) return d; }
  return 0;
};

export default function Python() {
  const toast = useToast();
  const runBusy = useBusy();
  const notices = useNotifications();
  const [py, setPy] = useState<PyenvStatus | null>(null);
  const [avail, setAvail] = useState<Shells>({ powershell: true, gitbash: false, cmd: true });
  const [loadErr, setLoadErr] = useState(false);
  const [busy, setBusy] = useState("");
  const [installOpen, setInstallOpen] = useState(false);
  const [onlyStable, setOnlyStable] = useState(() => initialBool(PY_FILTER_KEYS.onlyStable, true));
  const [latestOnly, setLatestOnly] = useState(() => initialBool(PY_FILTER_KEYS.latestOnly, false));
  const [pyInstallSetDef, setPyInstallSetDef] = useState(true); // 安装后设为默认（pyenv global）
  const [installRoot, setInstallRoot] = useState("");
  const [downloadSources, setDownloadSources] = useState<Mirror[]>(FALLBACK_DOWNLOAD_SOURCES);
  const [downloadSource, setDownloadSourceState] = useState<string>(initialDownloadSource);
  const [pendingDownloadSource, setPendingDownloadSource] = useState<string>(initialDownloadSource);
  const [sourcePings, setSourcePings] = useState<Record<string, number | null>>({});
  const [remote, setRemote] = useState<string[] | null>(null);
  const [uninstall, setUninstall] = useState<string | null>(null);
  const [srcRefresh, setSrcRefresh] = useState(0);
  const [scannedRuntimes, setScannedRuntimes] = useState<ScannedRuntime[] | null>(null);
  const [excludeToolBundled, setExcludeToolBundled] = useState(true);

  function applyDownloadSource(v = pendingDownloadSource) {
    if (!downloadSources.some((s) => s.id === v)) return;
    setDownloadSourceState(v);
    setPendingDownloadSource(v);
    localStorage.setItem(PY_SOURCE_KEY, v);
    setRemote(null);
    toast(`已应用 Python 下载源：${sourceName(v)}`, "ok");
    notices.checkNow("python-download-source").catch(() => undefined);
  }
  function sourceName(id: string) {
    return downloadSources.find((s) => s.id === id)?.name ?? id;
  }

  const loadPy = useCallback(async () => {
    setPy(await invoke<PyenvStatus>("pyenv_status"));
    invoke<Shells>("shells_available").then(setAvail).catch(() => {});
    invoke<string | null>("pyenv_root_dir").then((d) => {
      if (d) setInstallRoot((cur) => cur.trim() ? cur : d);
    }).catch(() => {});
    invoke<ToolState[]>("list_sources").then((tools) => {
      const runtime = tools.find((t) => t.id === PY_RUNTIME_TOOL_ID);
      const mirrors = runtime?.mirrors?.length ? runtime.mirrors : FALLBACK_DOWNLOAD_SOURCES;
      setDownloadSources(mirrors);
      setDownloadSourceState((cur) => {
        if (mirrors.some((m) => m.id === cur)) return cur;
        const fallback = mirrors.find((m) => m.id === "official")?.id ?? mirrors[0]?.id ?? "official";
        localStorage.setItem(PY_SOURCE_KEY, fallback);
        setPendingDownloadSource(fallback);
        const label = fallback === "official" ? "官方源" : (mirrors.find((m) => m.id === fallback)?.name ?? fallback);
        if (cur && cur !== fallback) toast(`原 Python 下载源已不在源清单中，已恢复为${label}`, "info");
        return fallback;
      });
      setPendingDownloadSource((cur) => mirrors.some((m) => m.id === cur) ? cur : (mirrors.find((m) => m.id === "official")?.id ?? mirrors[0]?.id ?? "official"));
    }).catch(() => {});
    setSrcRefresh((n) => n + 1);
  }, [toast]);
  useEffect(() => { loadPy().catch(() => setLoadErr(true)); }, [loadPy]);

  // 安装列表过滤：仅正式版 + 仅各 major.minor 最新
  function pyFilter(list: string[]): string[] {
    let r = onlyStable ? list.filter((v) => /^\d+\.\d+\.\d+$/.test(v)) : list;
    if (latestOnly) {
      const best = new Map<string, string>();
      for (const v of r) {
        const m = v.match(/^(\d+\.\d+)\.\d+$/);
        const key = m ? m[1] : v;
        const cur = best.get(key);
        if (!cur || cmpVer(v, cur) > 0) best.set(key, v);
      }
      r = [...best.values()].sort((a, b) => cmpVer(b, a));
    }
    return r;
  }

  async function busyAct(title: string, message: string, fn: () => Promise<unknown>, ok: string, key = ""): Promise<boolean> {
    if (key) setBusy(key);
    try {
      await runBusy({ title, message }, async () => {
        await fn();
        await loadPy();
      });
      toast(ok, "ok");
      void notices.checkNow("python-action").catch(() => undefined);
      return true;
    } catch (e) {
      toast(`${title}失败。请检查当前环境后重试。原因：` + e, "err");
      return false;
    } finally {
      if (key) setBusy("");
    }
  }

  async function scanRuntimes() {
    try {
      const drives = await invoke<DriveInfo[]>("list_drives");
      const roots = drives.filter((drive) => drive.fixed).map((drive) => `${drive.letter}\\`);
      const result = await runBusy({
        title: "扫描本机 Python 运行时",
        message: "正在扫描固定磁盘中的独立 Python 运行时。虚拟环境、缓存目录和工具自带版本不会作为可管理版本处理。",
        progressEvent: "env-scan-progress",
        cancel: { label: "取消扫描", onCancel: () => invoke("env_cancel").catch(() => undefined) },
      }, () => invoke<{ python: ScannedRuntime[] }>("env_scan", { roots, excludeToolBundled, kinds: ["python"] }));
      setScannedRuntimes(result.python);
      toast(`扫描完成，发现 ${result.python.length} 个独立 Python 运行时`, "ok");
    } catch (error) {
      if (!operationWasCancelled(error)) toast("扫描 Python 运行时失败：" + error, "err");
    }
  }
  async function refreshPy() {
    try {
      await runBusy({ title: "刷新 Python 状态", message: "正在检测 pyenv、Python 版本、pip/conda 状态与终端可用性…" }, loadPy);
      toast("已刷新", "ok");
    } catch (e) { toast("刷新 Python 状态失败。请稍后重试。原因：" + e, "err"); }
  }
  async function cleanupPythonRegistrations() {
    setBusy("pyreg");
    try {
      const count = await runBusy({ title: "清理 Python 残留", message: "正在清理已失效的 Python 系统卸载登记与 PythonCore 注册信息…" }, async () => {
        const n = await invoke<number>("pyenv_cleanup_stale_registrations");
        await loadPy();
        return n;
      });
      toast(count > 0 ? `清理残留完成：已清理 ${count} 项` : "清理残留完成：未发现需要清理的项目", "ok");
    } catch (e) {
      toast("清理 Python 残留失败。请关闭相关安装器窗口后重试。原因：" + e, "err");
    } finally {
      setBusy("");
    }
  }
  async function speedtestSources() {
    try {
      const rows = await runBusy({
        title: "Python 下载源测速",
        message: "正在测试各下载源的连接状态；单个源 1500ms 无响应算超时。",
        progressEvent: "pyenv-source-speed-progress",
      }, () => invoke<PyenvSourcePing[]>("pyenv_speedtest_sources"));
      const map: Record<string, number | null> = {};
      rows.forEach((r) => { map[r.id] = r.ms; });
      setSourcePings(map);
      const fastest = rows
        .filter((r): r is PyenvSourcePing & { ms: number } => typeof r.ms === "number")
        .sort((a, b) => a.ms - b.ms)[0];
      if (fastest && downloadSources.some((s) => s.id === fastest.id)) {
        setPendingDownloadSource(fastest.id);
        toast(
          fastest.id === downloadSource
            ? `测速完成，${fastest.name} 已是当前下载源`
            : `测速完成，已预选 ${fastest.name}，点击「应用」后生效`,
          "ok",
        );
      } else {
        toast("下载源测速均超时，保留当前下载源", "info");
      }
    } catch (e) { toast("Python 下载源测速失败。请检查网络连接后重试。原因：" + e, "err"); }
  }

  async function fetchInstallList(nextOnlyStable = onlyStable) {
    const label = sourceName(downloadSource);
    const rows = await runBusy({
      title: "获取 Python 版本列表",
      message: `正在从${label}读取 Python 版本并校验 64 位 Windows 安装包；${nextOnlyStable ? "仅列正式版" : "同时尝试列出 a/b/rc/dev 预发布版"}。`,
    }, () => invoke<string[]>("pyenv_install_list", { source: downloadSource, includePrerelease: !nextOnlyStable }));
    setRemote(rows);
  }

  async function openInstall() {
    setInstallOpen(true);
    setRemote(null);
    try {
      await fetchInstallList(onlyStable);
    } catch (e) {
      toast("获取 Python 版本列表失败。请切换下载源或稍后重试。原因：" + e, "err");
    }
  }

  async function changeOnlyStable(next: boolean) {
    setOnlyStable(next);
    localStorage.setItem(PY_FILTER_KEYS.onlyStable, String(next));
    if (!installOpen) return;
    setRemote(null);
    try {
      await fetchInstallList(next);
    } catch (e) {
      toast("获取 Python 版本列表失败。请切换下载源或稍后重试。原因：" + e, "err");
    }
  }

  async function installPyenv() {
    try {
      const next = await runBusy({ title: "安装 Python 版本管理工具", message: "正在安装内置 pyenv-win 并配置 Python 版本管理环境。", progressEvent: "install-progress", cancel: { label: "取消", onCancel: () => { invoke("op_cancel").catch(() => {}); } } }, async () => {
        await invoke("pyenv_install_self", { source: downloadSource });
        const status = await invoke<PyenvStatus>("pyenv_status");
        setPy(status);
        await invoke<Shells>("shells_available").then(setAvail).catch(() => {});
        return status;
      });
      toast(next.installed ? "Python 版本管理工具已安装，可继续安装所需的 Python 版本。" : "安装已完成，但当前会话尚未检测到 pyenv-win。请刷新状态或重启 Stacker 后再试。", next.installed ? "ok" : "info");
      void notices.checkNow("python-tool").catch(() => undefined);
      invoke<string | null>("pyenv_root_dir").then((d) => { if (d) setInstallRoot(d); }).catch(() => {});
    } catch (e) { toast(operationWasCancelled(e) ? "已取消安装 Python 版本管理工具" : "安装 pyenv-win 失败。请重启 Stacker 或检查安装目录权限后重试。原因：" + e, operationWasCancelled(e) ? "info" : "err"); }
  }
  async function checkPyenvUpdate() {
    try {
      const label = sourceName(downloadSource);
      const u = await runBusy({ title: "检查版本管理工具更新", message: `正在通过「${label}」查询可用更新…` }, () => invoke<{ current: string; latest: string; has_update: boolean }>("pyenv_check_update", { source: downloadSource }));
      if (u.has_update) {
        await runBusy({ title: "更新 Python 版本管理工具", message: `正在通过「${label}」获取更新文件，完成后会自动刷新状态。`, progressEvent: "install-progress", cancel: { label: "取消", onCancel: () => { invoke("op_cancel").catch(() => {}); } } }, async () => {
          await invoke("pyenv_self_update", { source: downloadSource });
          await loadPy();
        });
        toast(`Python 版本管理工具已更新到 v${u.latest}`, "ok");
        void notices.checkNow("python-tool-update").catch(() => undefined);
      } else { toast(`Python 版本管理工具已是最新版本（v${u.current}）`, "ok"); }
    } catch (e) { toast(operationWasCancelled(e) ? "已取消更新 Python 版本管理工具" : "检查 pyenv-win 更新失败。请稍后重试。原因：" + e, operationWasCancelled(e) ? "info" : "err"); }
  }
  async function installVer(v: string) {
    setInstallOpen(false);
    const setDef = pyInstallSetDef;
    try {
      const label = sourceName(downloadSource);
      await runBusy({ title: `安装 Python ${v}`, message: `正在通过「${label}」获取安装文件并配置 Python 运行环境。首次安装可能需要几分钟。`, progressEvent: "install-progress", cancel: { label: "取消安装", onCancel: () => { invoke("op_cancel").catch(() => {}); } } }, async () => {
        await invoke("pyenv_install_version", { version: v, source: downloadSource, installRoot: installRoot.trim() || null });
        if (setDef) await invoke("pyenv_set_global", { version: v });
        await loadPy();
      });
      toast(setDef ? `Python ${v} 已安装并设为默认版本` : `Python ${v} 已安装`, "ok");
      void notices.checkNow("python-install").catch(() => undefined);
    } catch (e) { toast(operationWasCancelled(e) ? `已取消安装 Python ${v}` : "安装 Python 失败。请切换下载源或检查安装目录权限后重试。原因：" + e, operationWasCancelled(e) ? "info" : "err"); }
  }

  if (loadErr) return <ErrorState title="暂时无法读取 Python 环境" description="请确认 pyenv-win 与 Python 安装目录可访问，然后重试。" onRetry={async () => { await loadPy(); setLoadErr(false); }} />;
  const pyLoading = !py;
  const pyState: PyenvStatus = py ?? { installed: false, pyenv_version: null, versions: [], default: null, has_conda: false };

  async function browseInstallRoot() {
    const dir = await open({ directory: true, defaultPath: installRoot || undefined });
    if (typeof dir === "string") setInstallRoot(dir);
  }

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
  const defaultPy = pyState.versions.find((v) => v.is_default)?.version ?? pyState.default ?? "";
  const rawUpdateHint = notices.ecosystemUpdates.find((item) => item.id === "python");
  const updateHint = rawUpdateHint && defaultPy && cmpVer(defaultPy, rawUpdateHint.latest) >= 0 ? undefined : rawUpdateHint;
  const updateTitle = updateHint ? `发现新版本：当前 ${updateHint.current}，最新 ${updateHint.latest}，下载源 ${sourceName(updateHint.source)}` : undefined;
  const pythonSummary = [
    "## Python 环境摘要",
    "",
    summaryLine("pyenv-win", pyState.installed ? pyState.pyenv_version || "已安装" : "未安装"),
    summaryLine("默认 Python", defaultPy || "未设置"),
    summaryLine("已安装版本", pyState.versions.map((v) => v.version).join(", ") || "无"),
    summaryLine("Python 下载源", sourceName(downloadSource)),
    summaryLine("conda", pyState.has_conda ? "已检测到" : "未检测到"),
    "",
    "## 给 AI 的使用说明",
    "- 使用 Python 前，先在当前终端执行 python --version 与 pip --version 确认可用版本。",
    "- 如需切换默认 Python，请通过工具设置默认版本，不要直接改系统级 PATH。",
  ].join("\n");

  return (
    <>
      {pyState.installed && <TerminalBar avail={avail} ecosystem="python" summary={pythonSummary}
        tip={"Python 命令通过 PATH 生效，新终端会自动使用当前默认版本。\n绿色终端按钮会在 Stacker 目录打开对应终端，可运行 python -V 验证版本。\npy 是 Windows Python Launcher，不代表当前默认版本。\n终端中找不到 python 时，可点击「更新集成」刷新用户 PATH。"}
        action={<button className="gh sm" disabled={busy === "pyint"} style={{ marginLeft: 8 }}
          title="刷新 Python 命令入口，修复新终端中找不到 python 的问题"
          onClick={() => busyAct("更新 Python 命令入口", "正在刷新当前用户的 Python 命令入口，新终端生效…", () => invoke("pyenv_write_integration"), "Python 命令入口已更新（新终端生效）", "pyint")}>
          <i className={"ti " + (busy === "pyint" ? "ti-loader spin" : "ti-plug")} /> {busy === "pyint" ? "写入中…" : "更新集成"}</button>} />}

      <div className="srcrow" style={{ marginBottom: 10 }}>
        <span className="av py"><i className="ti ti-download" /></span>
        <div className="mt">
          <div className="t">Python 下载源 {updateHint && <span className="bd r update-badge" title={updateTitle}>发现新版本</span>}</div>
          <div className="s dim" title="用于安装和更新 Python 版本管理工具及 Python 运行时。测速只会预选下载源，点击「应用」后生效。">用于安装和更新 Python；测速后点击「应用」生效。</div>
        </div>
        <Select value={pendingDownloadSource} width={220} onChange={setPendingDownloadSource} options={sourceOptions} />
        <button className="gh sm" onClick={speedtestSources}><i className="ti ti-bolt" /> 测速</button>
        <button className="pr sm" disabled={!!busy}
          title={sourceDirty ? `应用后安装 / 更新将使用 ${sourceName(pendingDownloadSource)}` : `当前已应用：${sourceName(downloadSource)}`}
          onClick={() => applyDownloadSource()}>
          <i className="ti ti-check" /> 应用
        </button>
      </div>

      {/* ① pyenv 版本 */}
      <div className="grouphd">
        <span className="gt"><i className="ti ti-stack-2" /> 运行时版本 <span className="cnt">{pyLoading ? "检测中" : pyState.installed ? `已安装 ${pyState.versions.length} 个` : "未安装"}</span></span>
        <div className="ghr">
          <label className="ck" style={{ fontSize: 11.5 }} title="排除 IDE、编辑器和其他开发工具随附的 Python。"><input type="checkbox" checked={excludeToolBundled} onChange={(event) => setExcludeToolBundled(event.target.checked)} /> 排除工具自带</label>
          <button className="gh xs" onClick={refreshPy}><i className="ti ti-refresh" /> 刷新状态</button>
          <button className="gh xs" onClick={scanRuntimes}><i className="ti ti-scan" /> 扫描本机</button>
          {pyState.installed && <button className="gh xs" title="清理已经卸载但仍显示在 Windows 应用列表中的 Python 登记" onClick={cleanupPythonRegistrations}><i className={"ti " + (busy === "pyreg" ? "ti-loader spin" : "ti-eraser")} /> 清理安装残留</button>}
          {pyState.installed && <button className="gh xs" title="检查 pyenv-win 是否有更新" onClick={checkPyenvUpdate}><i className="ti ti-cloud-download" /> 管理工具更新</button>}
          {pyState.installed && <button className="pr sm" onClick={openInstall}><i className="ti ti-plus" /> 安装新版本</button>}
        </div>
      </div>
      {pyLoading ? (
        <Loading text="正在检测 pyenv、Python 版本、pip 与 conda 状态…" />
      ) : !pyState.installed ? (
        pyState.has_conda ? (
          <div className="banner blue" style={{ flexDirection: "column", alignItems: "stretch", gap: 9 }}>
            <div style={{ display: "flex", gap: 11, alignItems: "flex-start" }}>
              <i className="ti ti-info-circle lead" />
              <div className="bt"><b>检测到 conda 环境</b><br />conda 会独立管理 Python 版本与环境。本页可继续配置 pip / conda 镜像；如需管理非 conda 的 Python 版本，也可
                <button className="lnk" style={{ background: "none", border: "none", color: "var(--acc)", cursor: "pointer", padding: 0, font: "inherit" }} onClick={installPyenv}>一键安装 pyenv-win</button>。</div>
            </div>
          </div>
        ) : (
          <div className="banner blue" style={{ flexDirection: "column", alignItems: "stretch", gap: 9 }}>
            <div style={{ display: "flex", gap: 11, alignItems: "flex-start" }}>
              <i className="ti ti-download lead" />
              <div className="bt"><b>安装 Python 版本管理工具</b><br />Stacker 将安装内置 pyenv-win，用于安装、切换和维护多个 Python 版本。pip 镜像可在下方单独配置。</div>
            </div>
            <div style={{ paddingLeft: 29 }}>
              <button className="pr sm" onClick={installPyenv}><i className="ti ti-download" /> 一键安装 pyenv-win</button>
            </div>
          </div>
        )
      ) : pyState.versions.length === 0 ? (
        <div className="banner gray"><i className="ti ti-info-circle lead" /><div className="bt">尚未安装 Python 版本。请选择需要的版本进行安装。</div></div>
      ) : pyState.versions.map((v) => (
        <div className={"vrow" + (v.is_default ? " cur" : "")} key={v.version}>
          <span className="ver">{v.version}</span>
          <span className="meta">{v.is_default ? "当前默认版本" : "已安装"}</span>
          <div className="acts">
            {v.is_default
              ? <><span className="live"><i className="ti ti-circle-check" /> 默认</span>
                  <button className="gh xs" disabled={!!busy} title="重新应用当前默认版本"
                    onClick={() => busyAct("重新应用默认 Python", `正在重新应用 Python ${v.version}…`, () => invoke("pyenv_set_global", { version: v.version }), "默认 Python 版本已重新应用", "g" + v.version)}><i className="ti ti-refresh" /> 重新应用</button></>
              : <button className="pr sm" disabled={!!busy} onClick={() => busyAct("设置默认 Python", `正在将 Python ${v.version} 设为默认版本…`, () => invoke("pyenv_set_global", { version: v.version }), "默认 Python 版本已更新为 " + v.version, "g" + v.version)}>设为默认</button>}
            <button className="gh xs danger" disabled={!!busy} title="删除此版本" onClick={() => setUninstall(v.version)}><i className="ti ti-trash" /></button>
          </div>
        </div>
      ))}

      {scannedRuntimes && scannedRuntimes.length > 0 && (
        <>
          <div className="seclabel"><i className="ti ti-device-desktop-search" /> 本机其他 Python 运行时</div>
          {scannedRuntimes.map((runtime) => (
            <div className="vrow" key={runtime.path}>
              <span className="ver">{runtime.version}</span>
              <span className="meta">{runtime.path}</span>
              <div className="acts"><span className="bd n">仅识别</span></div>
            </div>
          ))}
        </>
      )}

      {/* ② 包源（用统一面板，带测速） */}
      <div className="grouphd" style={{ marginTop: 18 }}><span className="gt"><i className="ti ti-package" /> 包源 / 镜像</span></div>
      {pyLoading ? <Loading text="正在读取 pip 与 conda 镜像配置…" /> : <SourcesPanel toolIds={pyState.has_conda ? ["pip", "conda"] : ["pip"]} refresh={srcRefresh} />}
      {!pyLoading && !pyState.has_conda && <div className="banner gray"><i className="ti ti-eye-off lead" /><div className="bt"><b>未检测到 conda。</b> 安装 Anaconda 或 Miniconda 后，可在此配置 conda 镜像。</div></div>}

      {installOpen && (
        <Modal title="安装 Python 版本" icon="ti-plus" onClose={() => setInstallOpen(false)}
          sub={<div style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
            <label style={{ display: "flex", alignItems: "center", gap: 6 }}><input type="checkbox" checked={onlyStable} onChange={(e) => { changeOnlyStable(e.target.checked).catch(() => {}); }} /> 仅正式版（隐藏 a/b/rc/dev）</label>
            <label style={{ display: "flex", alignItems: "center", gap: 6 }}><input type="checkbox" checked={latestOnly} onChange={(e) => { const next = e.target.checked; setLatestOnly(next); localStorage.setItem(PY_FILTER_KEYS.latestOnly, String(next)); }} /> 仅各版本最新（3.14.x 只显最新）</label>
            <label style={{ display: "flex", alignItems: "center", gap: 6 }}><input type="checkbox" checked={pyInstallSetDef} onChange={(e) => setPyInstallSetDef(e.target.checked)} /> 安装后设为默认版本</label>
            <span style={{ color: "var(--mut)" }}>下载源：{sourceName(downloadSource)}</span>
          </div>}>
          <div className="field"><label>安装位置</label>
            <div className="row" style={{ gap: 8, display: "flex" }}>
              <input className="ip full" style={{ flex: 1 }} value={installRoot} onChange={(e) => setInstallRoot(e.target.value)} />
              <button className="gh sm" onClick={browseInstallRoot}><i className="ti ti-folder" /> 浏览…</button>
            </div>
            <div className="hint">Python 会安装到此 pyenv-win 根目录下的 versions 文件夹；更换位置时会同步迁移 pyenv-win 管理文件。</div>
          </div>
          {!remote ? <div style={{ color: "var(--mut)", fontSize: 13 }}>获取版本列表…</div>
            : <div style={{ maxHeight: 280, overflow: "auto", display: "flex", flexDirection: "column", gap: 5 }}>
              {pyFilter(remote).length === 0 && <div style={{ color: "var(--mut)", fontSize: 13, padding: 8 }}>当前下载源没有匹配的 Python 版本。</div>}
              {pyFilter(remote).map((v) => {
                const has = pyState.versions.some((x) => x.version === v);
                return (
                  <div className="vrow" key={v} style={{ margin: 0 }}>
                    <span className="ver">{v}</span>
                    <div className="acts">{has
                      ? <span className="live"><i className="ti ti-circle-check" /> 已安装</span>
                      : <button className="gh xs" onClick={() => installVer(v)}>安装</button>}</div>
                  </div>
                );
              })}
            </div>}
          <div className="banner gray" style={{ margin: 0 }}><i className="ti ti-info-circle lead" /><div className="bt">此处的下载源用于获取 Python 安装包；pip 镜像请在下方「包源 / 镜像」区域单独配置。</div></div>
        </Modal>
      )}

      {uninstall && (
        <ConfirmModal title={"卸载 Python " + uninstall} icon="ti-trash" danger
          message={<>将删除 Python {uninstall} 及该版本目录内的已安装包和虚拟环境。此操作不可撤销。</>}
          confirmLabel={busy === "u" + uninstall ? "卸载中…" : "确认卸载"} busy={busy === "u" + uninstall}
          onConfirm={async () => { if (await busyAct("卸载 Python " + uninstall, `正在删除 Python ${uninstall}…`, () => invoke("pyenv_uninstall_version", { version: uninstall }), "已卸载 Python " + uninstall, "u" + uninstall)) setUninstall(null); }}
          onClose={() => setUninstall(null)} />
      )}
    </>
  );
}
