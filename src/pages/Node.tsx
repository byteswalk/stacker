import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useToast, Modal, ConfirmModal, useBusy, Loading } from "../ui";
import { SourcesPanel } from "../SourcesPanel";
import { TerminalBar } from "../TerminalBar";
import { Select } from "../Select";

type NodeVer = { version: string; is_default: boolean; path: string };
type Shells = { powershell: boolean; gitbash: boolean; cmd: boolean };
type FnmStatus = { installed: boolean; fnm_version: string | null; versions: NodeVer[]; default: string | null; shell: Shells; has_nvm: boolean };
type Mirror = { id: string; name: string; url: string; host: string };
type ToolState = { id: string; name: string; mirrors: Mirror[] };
type NodeSourcePing = { id: string; name: string; ms: number | null };
type BinaryMirrorVar = { name: string; value: string; current: string | null; matched: boolean };
type BinaryMirrorState = {
  id: string;
  name: string;
  icon: string;
  description: string;
  enabled: boolean;
  configured: boolean;
  status_label: string;
  vars: BinaryMirrorVar[];
};

const NODE_RUNTIME_TOOL_ID = "node-runtime";
const NODE_SOURCE_KEY = "stacker.node.downloadSource";
const FALLBACK_NODE_SOURCES: Mirror[] = [{ id: "official", name: "官方", url: "https://nodejs.org/dist", host: "nodejs.org" }];
function initialNodeSource() {
  const saved = typeof localStorage !== "undefined" ? localStorage.getItem(NODE_SOURCE_KEY) : null;
  return saved || "official";
}

export default function Node() {
  const toast = useToast();
  const runBusy = useBusy();
  const [st, setSt] = useState<FnmStatus | null>(null);
  const [avail, setAvail] = useState<Shells>({ powershell: true, gitbash: false, cmd: true });
  const [loadErr, setLoadErr] = useState(false);
  const [busy, setBusy] = useState("");
  const [installOpen, setInstallOpen] = useState(false);
  const [lts, setLts] = useState(true);
  const [nodeLatestOnly, setNodeLatestOnly] = useState(false);
  const [nodeInstallSetDef, setNodeInstallSetDef] = useState(true);
  const [nodeInstallScope, setNodeInstallScope] = useState<"user" | "system">("user");
  const [nodeSysConfigured, setNodeSysConfigured] = useState(false);
  const [installRoot, setInstallRoot] = useState("");
  const [defaultDlg, setDefaultDlg] = useState<NodeVer | null>(null);
  const [defaultScope, setDefaultScope] = useState<"user" | "system">("user");
  const [downloadSources, setDownloadSources] = useState<Mirror[]>(FALLBACK_NODE_SOURCES);
  const [downloadSource, setDownloadSourceState] = useState<string>(initialNodeSource);
  const [pendingDownloadSource, setPendingDownloadSource] = useState<string>(initialNodeSource);
  const [sourcePings, setSourcePings] = useState<Record<string, number | null>>({});
  const [remote, setRemote] = useState<string[] | null>(null);
  const [uninstall, setUninstall] = useState<string | null>(null);
  const [srcRefresh, setSrcRefresh] = useState(0);
  const [binaryMirrors, setBinaryMirrors] = useState<BinaryMirrorState[]>([]);

  // 仅各大版本最新：按主版本号分组取最大（v24.x 只显最新）
  function nodeFilter(list: string[]): string[] {
    if (!nodeLatestOnly) return list;
    const num = (v: string) => v.replace(/^v/, "").split(".").map((n) => +n || 0);
    const cmp = (a: string, b: string) => { const pa = num(a), pb = num(b); for (let i = 0; i < 3; i++) { const d = (pa[i] || 0) - (pb[i] || 0); if (d) return d; } return 0; };
    const best = new Map<string, string>();
    for (const v of list) { const major = num(v)[0]; const cur = best.get(String(major)); if (!cur || cmp(v, cur) > 0) best.set(String(major), v); }
    return [...best.values()].sort((a, b) => cmp(b, a));
  }

  async function load() {
    setSt(await invoke<FnmStatus>("fnm_status"));
    refreshBinaryMirrors();
    invoke<Shells>("shells_available").then(setAvail).catch(() => {});
    invoke<Record<string, boolean>>("env_system_info").then((m) => {
      const hasSystemNode = !!m.node;
      setNodeSysConfigured(hasSystemNode);
      setNodeInstallScope(hasSystemNode ? "system" : "user");
    }).catch(() => {});
    invoke<string>("fnm_root_dir").then((d) => {
      setInstallRoot((cur) => cur.trim() ? cur : d);
    }).catch(() => {});
    invoke<ToolState[]>("list_sources").then((tools) => {
      const runtime = tools.find((t) => t.id === NODE_RUNTIME_TOOL_ID);
      const mirrors = runtime?.mirrors?.length ? runtime.mirrors : FALLBACK_NODE_SOURCES;
      setDownloadSources(mirrors);
      setDownloadSourceState((cur) => {
        if (mirrors.some((m) => m.id === cur)) return cur;
        const fallback = mirrors.find((m) => m.id === "official")?.id ?? mirrors[0]?.id ?? "official";
        localStorage.setItem(NODE_SOURCE_KEY, fallback);
        setPendingDownloadSource(fallback);
        const label = fallback === "official" ? "官方源" : (mirrors.find((m) => m.id === fallback)?.name ?? fallback);
        if (cur && cur !== fallback) toast(`原 Node 下载源已不在源清单中，已恢复为${label}`, "info");
        return fallback;
      });
      setPendingDownloadSource((cur) => mirrors.some((m) => m.id === cur) ? cur : (mirrors.find((m) => m.id === "official")?.id ?? mirrors[0]?.id ?? "official"));
    }).catch(() => {});
    setSrcRefresh((n) => n + 1);
  }
  useEffect(() => { load().catch(() => setLoadErr(true)); }, []);

  function sourceName(id: string) {
    return downloadSources.find((s) => s.id === id)?.name ?? id;
  }
  function refreshBinaryMirrors() {
    invoke<BinaryMirrorState[]>("binary_mirror_status").then(setBinaryMirrors).catch(() => {});
  }
  async function browseInstallRoot() {
    const dir = await open({ directory: true, defaultPath: installRoot || undefined });
    if (typeof dir === "string") setInstallRoot(dir);
  }
  function applyDownloadSource(v = pendingDownloadSource) {
    if (!downloadSources.some((s) => s.id === v)) return;
    setDownloadSourceState(v);
    setPendingDownloadSource(v);
    localStorage.setItem(NODE_SOURCE_KEY, v);
    setRemote(null);
    toast(`已应用 Node 下载源：${sourceName(v)}`, "ok");
  }
  async function busyAct(title: string, message: string, fn: () => Promise<unknown>, ok: string, key: string): Promise<boolean> {
    setBusy(key);
    try {
      await runBusy({ title, message }, async () => {
        await fn();
        await load();
      });
      toast(ok, "ok");
      return true;
    } catch (e) {
      toast(`${title}失败。请检查当前环境后重试。原因：` + e, "err");
      return false;
    } finally {
      setBusy("");
    }
  }
  async function refreshNode() {
    try {
      await runBusy({ title: "刷新 Node 状态", message: "正在检测 Node 版本、默认版本与终端可用性…" }, load);
      toast("已刷新", "ok");
    } catch (e) {
      toast("刷新 Node 状态失败。请稍后重试。原因：" + e, "err");
    }
  }
  async function loadRemote(v: boolean, source = downloadSource) {
    setLts(v);
    setRemote(null);
    try {
      const label = sourceName(source);
      const rows = await runBusy({
        title: "获取 Node 版本列表",
        message: `正在从${label}读取 Node 版本并筛选 Windows 64 位安装包。`,
      }, () => invoke<string[]>("fnm_ls_remote", { ltsOnly: v, source }));
      setRemote(rows);
    }
    catch (e) { toast("获取 Node 版本列表失败。请切换下载源或稍后重试。原因：" + e, "err"); }
  }

  async function speedtestSources() {
    try {
      const rows = await runBusy({
        title: "Node 下载源测速",
        message: "正在测试各下载源的连接状态；单个源 1500ms 无响应算超时。",
        progressEvent: "node-source-speed-progress",
      }, () => invoke<NodeSourcePing[]>("fnm_speedtest_sources"));
      const map: Record<string, number | null> = {};
      rows.forEach((r) => { map[r.id] = r.ms; });
      setSourcePings(map);
      const fastest = rows
        .filter((r): r is NodeSourcePing & { ms: number } => typeof r.ms === "number")
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
    } catch (e) {
      toast("Node 下载源测速失败。请检查网络连接后重试。原因：" + e, "err");
    }
  }

  // 长操作走全局进度模态（挡切页、可转后台），完成后刷新状态
  async function installSelf() {
    try {
      await runBusy({ title: "安装 Node 版本管理工具", message: "Stacker 正在安装 fnm，请保持应用窗口打开。", progressEvent: "install-progress", cancel: { label: "取消", onCancel: () => { invoke("op_cancel").catch(() => {}); } } }, async () => {
        await invoke("fnm_install_self");
        await load();
      });
      toast("fnm 已安装，可继续安装 Node 版本", "ok");
    } catch (e) { toast("安装 fnm 失败。请重启 Stacker 或检查安装目录权限后重试。原因：" + e, "err"); }
  }
  async function installVer(v: string) {
    setInstallOpen(false);
    const setDef = nodeInstallSetDef;
    try {
      const label = sourceName(downloadSource);
      await runBusy({ title: `安装 Node ${v}`, message: `Stacker 正在通过「${label}」获取安装文件，并配置 Node 运行环境。请保持应用窗口打开。`, progressEvent: "install-progress", cancel: { label: "取消安装", onCancel: () => { invoke("op_cancel").catch(() => {}); } } }, async () => {
        await invoke("fnm_install_version", {
          version: v,
          source: downloadSource,
          setDefault: setDef,
          scope: setDef ? nodeInstallScope : null,
          siblings: st?.versions.map((x) => x.path) ?? [],
          installRoot: installRoot.trim() || null,
        });
        await load();
      });
      toast(setDef ? `Node ${v} 已安装并设为默认版本（${nodeInstallScope === "system" ? "系统级" : "用户级"}）` : `Node ${v} 已安装`, "ok");
    } catch (e) { toast("安装 Node 失败。请切换下载源或检查安装目录权限后重试。原因：" + e, "err"); }
  }
  async function applyDefaultNode() {
    if (!defaultDlg || !st) return;
    const picked = defaultDlg;
    setBusy("def" + picked.version);
    try {
      await runBusy({
        title: "设置默认 Node",
        message: `正在将 Node ${picked.version} 设为默认版本，并写入${defaultScope === "system" ? "系统级" : "当前用户"} PATH。`,
      }, async () => {
        await invoke("fnm_set_default", {
          version: picked.version,
          scope: defaultScope,
          siblings: st.versions.map((x) => x.path),
        });
        await load();
      });
      toast(`默认 Node 版本已更新为 ${picked.version}（${defaultScope === "system" ? "系统级" : "用户级"}）`, "ok");
      setDefaultDlg(null);
    } catch (e) {
      toast("设置默认 Node 失败。请确认该版本目录仍然存在后重试。原因：" + e, "err");
    } finally {
      setBusy("");
    }
  }
  async function checkFnmUpdate() {
    try {
      const u = await runBusy({ title: "检查版本管理工具更新", message: "正在查询官方源可用更新…", cancel: { label: "取消", onCancel: () => {} } }, () => invoke<{ current: string; latest: string; has_update: boolean }>("fnm_check_update"));
      if (u.has_update) {
        await runBusy({ title: "更新 Node 版本管理工具", message: `正在从官方源获取 v${u.latest}，完成后会自动刷新状态。`, progressEvent: "install-progress", cancel: { label: "取消", onCancel: () => { invoke("op_cancel").catch(() => {}); } } }, async () => {
          await invoke("fnm_self_update");
          await load();
        });
        toast(`Node 版本管理工具已更新到 v${u.latest}`, "ok");
      } else { toast(`Node 版本管理工具已是最新版本（v${u.current}）`, "ok"); }
    } catch (e) { toast("检查 fnm 更新失败。请稍后重试。原因：" + e, "err"); }
  }
  async function applyBinaryMirror(row: BinaryMirrorState) {
    const key = `binary:${row.id}`;
    setBusy(key);
    try {
      await runBusy({
        title: `启用 ${row.name} 下载镜像`,
        message: "正在写入当前用户环境变量；新打开的终端和安装命令会读取新配置。",
      }, () => invoke("binary_mirror_apply", { id: row.id }));
      refreshBinaryMirrors();
      toast(`${row.name} 下载镜像已启用（新终端生效）`, "ok");
    } catch (e) {
      toast(`启用 ${row.name} 下载镜像失败。请稍后重试。原因：` + e, "err");
    } finally {
      setBusy("");
    }
  }
  async function clearBinaryMirror(row: BinaryMirrorState) {
    const key = `binary-clear:${row.id}`;
    setBusy(key);
    try {
      await runBusy({
        title: `清除 ${row.name} 下载镜像`,
        message: "正在清除当前用户环境变量；新打开的终端会恢复默认下载地址。",
      }, () => invoke("binary_mirror_clear", { id: row.id }));
      refreshBinaryMirrors();
      toast(`${row.name} 下载镜像已清除（新终端生效）`, "ok");
    } catch (e) {
      toast(`清除 ${row.name} 下载镜像失败。请稍后重试。原因：` + e, "err");
    } finally {
      setBusy("");
    }
  }
  function binaryBadge(row: BinaryMirrorState) {
    if (row.enabled) return <span className="bd g">已加速</span>;
    if (row.configured) return <span className="bd w">自定义</span>;
    return <span className="bd n">默认</span>;
  }
  function binaryTitle(row: BinaryMirrorState) {
    return row.vars.map((v) => `${v.name} = ${v.current ?? "未设置"}\n目标：${v.value}`).join("\n\n");
  }

  if (loadErr) return <div className="stub"><div className="si"><i className="ti ti-plug-x" /></div><h2>读取 fnm 状态失败</h2><p>请在 Tauri 应用内运行（浏览器预览没有后端）。</p></div>;
  if (!st) return <Loading text="正在检测 fnm 与 Node 版本…" />;

  const noIntegration = st.installed && !st.shell.powershell && !st.shell.gitbash;
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
  const uninstallVersion = uninstall ? st.versions.find((v) => v.version === uninstall) : null;
  const uninstallIsDefault = !!uninstallVersion?.is_default;
  const nodeSourceRow = (
    <div className="srcrow" style={{ marginBottom: 10 }}>
      <span className="av npm"><i className="ti ti-download" /></span>
      <div className="mt">
        <div className="t">Node 下载源</div>
        <div className="s dim" title="用于获取和安装 Node 版本。fnm 首次安装使用内置版本，工具更新使用官方源。">用于安装 Node 版本；测速后点击「应用」生效。</div>
      </div>
      <Select value={pendingDownloadSource} width={220} onChange={setPendingDownloadSource} options={sourceOptions} />
      <button className="gh sm" onClick={speedtestSources}><i className="ti ti-bolt" /> 测速</button>
      <button className="pr sm" disabled={!!busy}
        title={sourceDirty ? `应用后安装 Node 版本将使用 ${sourceName(pendingDownloadSource)}` : `当前已应用：${sourceName(downloadSource)}`}
        onClick={() => applyDownloadSource()}>
        <i className="ti ti-check" /> 应用
      </button>
    </div>
  );

  return (
    <>
      {!st.installed ? (
        <>
          <div className="grouphd"><span className="gt"><i className="ti ti-stack-2" /> 运行时版本</span></div>
          <div className="banner blue" style={{ flexDirection: "column", alignItems: "stretch", gap: 9 }}>
            <div style={{ display: "flex", gap: 11, alignItems: "flex-start" }}>
              <i className="ti ti-download lead" />
              <div className="bt"><b>安装 Node 版本管理工具</b><br />Stacker 将安装内置 fnm，用于安装、切换和维护多个 Node 版本。</div>
            </div>
            <div style={{ paddingLeft: 29 }}>
              <button className="pr sm" onClick={installSelf}>
                <i className="ti ti-download" /> 一键安装 fnm</button>
            </div>
          </div>
        </>
      ) : (
        <>
          {noIntegration && (
            <div className="banner red">
              <i className="ti ti-plug-connected-x lead" />
              <div className="bt"><b>Node 命令入口尚未写入终端。</b> 写入后，新开的终端会自动使用当前默认 Node 版本。</div>
              <div className="acts"><button className="pr sm" disabled={busy === "int"} onClick={() => busyAct("写入 Node 命令入口", "正在写入 PowerShell / Git Bash / cmd 的 Node 命令入口，改动前会自动备份…", () => invoke("fnm_write_integration", { shells: ["powershell", "gitbash", "cmd"] }), "Node 命令入口已写入（新终端生效）", "int")}><i className="ti ti-plug" /> 一键写入</button></div>
            </div>
          )}
          <TerminalBar avail={avail}
            tip={"Node 命令通过终端集成生效，新终端会自动使用当前默认版本。\n绿色终端按钮会在 Stacker 目录打开对应终端，可运行 node -v 验证版本。\n终端中找不到 node 时，可点击「更新集成」刷新命令入口。"}
            action={<button className="gh sm" disabled={busy === "int"} style={{ marginLeft: 8 }}
              title="刷新 Node 命令入口，修复新终端中找不到 node 的问题"
              onClick={() => busyAct("更新 Node 命令入口", "正在刷新当前用户的 Node 命令入口，新终端生效…", () => invoke("fnm_write_integration", { shells: ["powershell", "gitbash", "cmd"] }), "Node 命令入口已更新（新终端生效）", "int")}>
              <i className={"ti " + (busy === "int" ? "ti-loader spin" : "ti-plug")} /> {busy === "int" ? "写入中…" : "更新集成"}</button>} />

          {nodeSourceRow}

          <div className="grouphd">
            <span className="gt"><i className="ti ti-stack-2" /> 运行时版本 <span className="cnt">已安装 {st.versions.length} 个</span></span>
            <div className="ghr"><button className="gh xs" onClick={refreshNode}><i className="ti ti-refresh" /> 刷新</button>
              <button className="gh xs" title="检查 Node 版本管理工具是否有更新" onClick={checkFnmUpdate}><i className="ti ti-cloud-download" /> 工具更新</button>
              <button className="pr sm" onClick={() => { setInstallOpen(true); loadRemote(lts); }}><i className="ti ti-plus" /> 安装新版本</button></div>
          </div>
          {st.versions.length > 0 && !st.default && (
            <div className="banner gray">
              <i className="ti ti-info-circle lead" />
              <div className="bt">尚未设置默认 Node 版本。新终端不会注入 fnm 的 Node；如果系统 PATH 中还有其他 Node，命令行可能仍会显示那个外部版本。</div>
            </div>
          )}
          {st.versions.length === 0
            ? <div className="banner gray"><i className="ti ti-info-circle lead" /><div className="bt">尚未安装 Node 版本。请选择需要的版本进行安装。</div></div>
            : st.versions.map((v) => (
              <div className={"vrow" + (v.is_default ? " cur" : "")} key={v.version}>
                <span className="ver">{v.version}</span>
                <span className="meta">{v.is_default ? "当前默认版本" : "已安装"}</span>
                <div className="acts">
                  {v.is_default
                    ? <><span className="live"><i className="ti ti-circle-check" /> 默认</span>
                        <button className="gh xs" disabled={!!busy} title="重新写入 fnm 默认并刷新"
                          onClick={() => { setDefaultScope(nodeSysConfigured ? "system" : "user"); setDefaultDlg(v); }}><i className="ti ti-refresh" /> 重新应用</button></>
                    : <button className="pr sm" disabled={!!busy} onClick={() => { setDefaultScope(nodeSysConfigured ? "system" : "user"); setDefaultDlg(v); }}>设为默认</button>}
                  <button className="spd" disabled={!!busy} title="卸载" onClick={() => setUninstall(v.version)}><i className="ti ti-trash" /></button>
                </div>
              </div>
            ))}
        </>
      )}

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-package" /> 包源 / 镜像</span>
      </div>
      <SourcesPanel toolIds={["npm", "yarn"]} refresh={srcRefresh} />

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-cloud-download" /> 下载镜像 <span className="cnt">二进制 / 浏览器 / 模型</span></span>
      </div>
      <div className="srctoolbar">
        <div className="mt">
          <div className="s dim" title="这些设置用于 npm 包安装脚本额外下载的大文件，例如 Electron、浏览器运行包、Cypress、原生依赖二进制和 HuggingFace 模型。">
            处理 npm registry 之外的大文件下载；写入当前用户环境变量，新终端生效。
          </div>
        </div>
      </div>
      {binaryMirrors.map((row) => (
        <div className="srcrow" key={row.id}>
          <span className="av npm"><i className={"ti " + row.icon} /></span>
          <div className="mt">
            <div className="t">{row.name} {binaryBadge(row)}</div>
            <div className="s dim" title={binaryTitle(row)}>{row.description}</div>
            <div className="s mono" title={binaryTitle(row)}>{row.vars[0]?.name} → {row.enabled ? "npmmirror / 国内镜像" : row.configured ? "自定义配置" : "官方默认"}</div>
          </div>
          <button className="pr sm" disabled={!!busy} onClick={() => applyBinaryMirror(row)}>
            <i className={"ti " + (busy === `binary:${row.id}` ? "ti-loader spin" : "ti-check")} /> {row.enabled ? "重新应用" : "启用"}
          </button>
          <button className="gh sm" disabled={!!busy || !row.configured} onClick={() => clearBinaryMirror(row)}>
            <i className={"ti " + (busy === `binary-clear:${row.id}` ? "ti-loader spin" : "ti-eraser")} /> 清除
          </button>
        </div>
      ))}

      {defaultDlg && (
        <Modal wide title={`把默认切到 ${defaultDlg.version}`} icon="ti-brand-nodejs" onClose={() => !busy && setDefaultDlg(null)}
          sub={<b style={{ color: "var(--tx)" }}>将 fnm 默认版本与 PATH 同步到同一作用范围</b>}
          footer={<>
            <button className="gh sm" onClick={() => setDefaultDlg(null)} disabled={!!busy}>取消</button>
            <button className="pr" style={{ background: "#d97a1f" }} onClick={applyDefaultNode} disabled={!!busy}>
              <i className="ti ti-shield-half" /> {busy ? "应用中…" : defaultScope === "system" ? "应用（将触发 UAC 提权）" : "应用"}</button>
          </>}>
          <div className="field"><label>作用范围</label>
            <div className={"opt" + (defaultScope === "user" ? " sel" : "")} onClick={() => setDefaultScope("user")}><span className="rd" />
              <div><div className="ot">仅当前用户 <span className="bd n" style={{ fontSize: 10 }}>免管理员</span></div>
                <div className="od">写当前用户 PATH。适合个人开发环境和普通终端。</div></div></div>
            <div className={"opt" + (defaultScope === "system" ? " sel" : "")} onClick={() => setDefaultScope("system")}><span className="rd" />
              <div><div className="ot"><i className="ti ti-shield-lock" style={{ color: "#f5a45a" }} /> 系统全局 <span className="bd w" style={{ fontSize: 10 }}>需管理员 · 需 UAC 提权</span></div>
                <div className="od">写系统 PATH。系统级 Node 或其他程序覆盖当前用户 PATH 时使用。</div></div></div>
          </div>
          <div className="banner gray" style={{ margin: 0 }}><i className="ti ti-history lead" /><div className="bt">改动前会自动备份 PATH，可在「历史」中还原。</div></div>
        </Modal>
      )}

      {installOpen && (
        <Modal title="安装 Node 版本" icon="ti-plus" onClose={() => setInstallOpen(false)}
          sub={<div style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
            <label style={{ display: "flex", alignItems: "center", gap: 6 }}><input type="checkbox" checked={lts} onChange={(e) => loadRemote(e.target.checked)} /> 仅 LTS</label>
            <label style={{ display: "flex", alignItems: "center", gap: 6 }}><input type="checkbox" checked={nodeLatestOnly} onChange={(e) => setNodeLatestOnly(e.target.checked)} /> 仅各版本最新（v24.x 只显最新）</label>
            <label style={{ display: "flex", alignItems: "center", gap: 6 }}><input type="checkbox" checked={nodeInstallSetDef} onChange={(e) => setNodeInstallSetDef(e.target.checked)} /> 安装后设为默认版本</label>
            <span style={{ color: "var(--mut)" }}>下载源：{sourceName(downloadSource)}</span>
          </div>}>
          <div className="field"><label>安装位置</label>
            <div className="row" style={{ gap: 8, display: "flex" }}>
              <input className="ip full" style={{ flex: 1 }} value={installRoot} onChange={(e) => setInstallRoot(e.target.value)} />
              <button className="gh sm" onClick={browseInstallRoot}><i className="ti ti-folder" /> 浏览…</button>
            </div>
            <div className="hint">Node 会安装到此目录下的 node-versions 文件夹，默认使用 fnm 当前版本目录。</div>
          </div>
          {nodeInstallSetDef && (
            <div className="field"><label>默认范围</label>
              <div className={"opt" + (nodeInstallScope === "user" ? " sel" : "")} onClick={() => setNodeInstallScope("user")}><span className="rd" />
                <div><div className="ot">仅当前用户 <span className="bd n" style={{ fontSize: 10 }}>免管理员</span></div>
                  <div className="od">安装完成后写入当前用户 PATH。</div></div></div>
              <div className={"opt" + (nodeInstallScope === "system" ? " sel" : "")} onClick={() => setNodeInstallScope("system")}><span className="rd" />
                <div><div className="ot"><i className="ti ti-shield-lock" style={{ color: "#f5a45a" }} /> 系统全局 <span className="bd w" style={{ fontSize: 10 }}>需管理员 · 需 UAC 提权</span></div>
                  <div className="od">安装完成后写入系统 PATH。系统级 Node 覆盖当前用户配置时选择。</div></div></div>
            </div>
          )}
          {!remote ? <div style={{ color: "var(--mut)", fontSize: 13 }}>获取版本列表…</div>
            : <div style={{ maxHeight: 280, overflow: "auto", display: "flex", flexDirection: "column", gap: 5 }}>
              {nodeFilter(remote).map((v) => {
                const has = st.versions.some((x) => x.version === v);
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
          <div className="banner gray" style={{ margin: 0 }}><i className="ti ti-info-circle lead" /><div className="bt">此处的下载源仅用于获取 Node 安装包；fnm 随 Stacker 内置，不受该选择影响。</div></div>
        </Modal>
      )}

      {uninstall && (
        <ConfirmModal title={"卸载 Node " + uninstall} icon="ti-trash" danger
          message={uninstallIsDefault
            ? <>该版本是当前默认 Node。卸载后将清除默认版本；如系统 PATH 中还有其他 Node，新终端可能仍会执行那个外部版本。此操作不可撤销。</>
            : <>将删除该 Node 版本及其全局安装的包。此操作不可撤销。</>}
          confirmLabel={busy === "uninst" ? "卸载中…" : "确认卸载"} busy={busy === "uninst"}
          onConfirm={async () => { if (await busyAct("卸载 Node " + uninstall, `正在卸载 Node ${uninstall}…`, () => invoke("fnm_uninstall_version", { version: uninstall }), "已卸载 " + uninstall, "uninst")) setUninstall(null); }}
          onClose={() => setUninstall(null)} />
      )}
    </>
  );
}
