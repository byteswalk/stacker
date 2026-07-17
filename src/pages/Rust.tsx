import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "../invoke";
import { useToast, Modal, ConfirmModal, useBusy, Loading, ErrorState, operationWasCancelled } from "../ui";
import { SourcesPanel } from "../SourcesPanel";
import { TerminalBar } from "../TerminalBar";
import { Select } from "../Select";
import { EcoActions, type Shells, summaryLine } from "../EcoActions";
import { useNotifications } from "../notifications";

type Toolchain = { name: string; is_default: boolean };
type RustupStatus = {
  installed: boolean;
  rustup_version: string | null;
  toolchains: Toolchain[];
  default: string | null;
  default_version?: string | null;
  probe_error?: string | null;
};
type Mirror = { id: string; name: string; url: string; host: string };
type ToolState = { id: string; mirrors: Mirror[] };
type SourcePing = { host: string; ms: number | null };
type RustupAddon = { name: string; installed: boolean };

const SOURCE_KEY = "stacker.rust.downloadSource";
const RUST_FILTER_KEYS = {
  onlyStable: "stacker.rust.install.onlyStable",
  latestOnly: "stacker.rust.install.latestOnly",
};
const RUST_RUNTIME_TOOL_ID = "rust-runtime";

const cmpVer = (a: string, b: string) => {
  const nums = (v: string) => v.match(/\d+/g)?.map((n) => Number(n)) ?? [];
  for (let i = 0; i < Math.max(nums(a).length, nums(b).length, 3); i++) {
    const d = (nums(a)[i] || 0) - (nums(b)[i] || 0);
    if (d) return d;
  }
  return 0;
};
const stableVersion = (v: string) => /^\d+\.\d+\.\d+$/.test(v);
const minorLine = (v: string) => v.match(/^(\d+\.\d+)/)?.[1] ?? v;
function initialBool(key: string, fallback: boolean) {
  const saved = typeof localStorage !== "undefined" ? localStorage.getItem(key) : null;
  return saved === null ? fallback : saved === "true";
}

export default function Rust() {
  const toast = useToast();
  const runBusy = useBusy();
  const notices = useNotifications();
  const [ru, setRu] = useState<RustupStatus | null>(null);
  const [loadErr, setLoadErr] = useState(false);
  const [busy, setBusy] = useState("");
  const [installOpen, setInstallOpen] = useState(false);
  const [uninstall, setUninstall] = useState<string | null>(null);
  const [srcKey, setSrcKey] = useState(0);
  const [shells, setShells] = useState<Shells>({ powershell: true, gitbash: false, cmd: true });

  const [sources, setSources] = useState<Mirror[]>([]);
  const [source, setSource] = useState(() => localStorage.getItem(SOURCE_KEY) || "official");
  const [pendingSource, setPendingSource] = useState(() => localStorage.getItem(SOURCE_KEY) || "official");
  const [sourcePings, setSourcePings] = useState<Record<string, number | null>>({});

  const [versions, setVersions] = useState<string[] | null>(null);
  const [onlyStable, setOnlyStable] = useState(() => initialBool(RUST_FILTER_KEYS.onlyStable, true));
  const [latestOnly, setLatestOnly] = useState(() => initialBool(RUST_FILTER_KEYS.latestOnly, true));
  const [installSetDefault, setInstallSetDefault] = useState(true);
  const [manualVersion, setManualVersion] = useState("");
  const [addonsOpen, setAddonsOpen] = useState(false);
  const [addonTab, setAddonTab] = useState<"component" | "target">("component");
  const [components, setComponents] = useState<RustupAddon[]>([]);
  const [targets, setTargets] = useState<RustupAddon[]>([]);
  const [addonLoading, setAddonLoading] = useState(false);
  const [addonBusy, setAddonBusy] = useState("");

  const load = useCallback(async () => {
    setRu(await invoke<RustupStatus>("rustup_status"));
    invoke<Shells>("shells_available").then(setShells).catch(() => {});
  }, []);

  const loadSources = useCallback(async () => {
    const tools = await invoke<ToolState[]>("list_sources");
    const rows = tools.find((tool) => tool.id === RUST_RUNTIME_TOOL_ID)?.mirrors ?? [];
    setSources(rows);
    const fallback = rows.find((item) => item.id === "official")?.id ?? rows[0]?.id ?? "";
    const saved = localStorage.getItem(SOURCE_KEY) || fallback;
    const next = rows.some((item) => item.id === saved) ? saved : fallback;
    if (saved && next !== saved) {
      localStorage.setItem(SOURCE_KEY, next);
      toast("原 Rust 工具链下载源已不在源清单中，已恢复为官方源", "info");
    }
    setSource(next);
    setPendingSource(next);
  }, [toast]);

  useEffect(() => {
    load().catch(() => setLoadErr(true));
    loadSources().catch(() => undefined);
  }, [load, loadSources]);

  const sourceObj = sources.find((item) => item.id === source) ?? sources[0];
  const sourceName = (id: string) => sources.find((item) => item.id === id)?.name ?? id;
  const sourceUrl = sourceObj?.url ?? "https://static.rust-lang.org";

  const defaultToolchain = ru?.toolchains.find((t) => t.is_default)?.name ?? ru?.default ?? "";
  const defaultToolchainVersion = ru?.default_version ?? (/^\d+\.\d+\.\d+$/.test(defaultToolchain) ? defaultToolchain : "");
  const rawUpdateHint = notices.ecosystemUpdates.find((item) => item.id === "rust");
  const updateHint = rawUpdateHint && defaultToolchainVersion && cmpVer(defaultToolchainVersion, rawUpdateHint.latest) >= 0 ? undefined : rawUpdateHint;
  const updateTitle = updateHint ? `发现新版本：当前 ${updateHint.current}，最新 ${updateHint.latest}，下载源 ${sourceName(updateHint.source)}` : undefined;
  const rustSummary = [
    "## Rust 环境摘要",
    "",
    summaryLine("rustup", ru?.installed ? ru.rustup_version || "已安装" : "未安装"),
    summaryLine("默认工具链", defaultToolchain || "未设置"),
    summaryLine("默认工具链版本", ru?.default_version || "未检测到"),
    summaryLine("已安装工具链", ru?.toolchains.map((t) => t.name).join(", ") || "无"),
    summaryLine("工具链下载源", sourceName(source)),
    summaryLine("Cargo 源", "见本页 Cargo 源配置"),
    "",
    "## 给 AI 的使用说明",
    "- 构建 Rust 项目前，先执行 rustc --version、cargo --version 与 rustup show。",
    "- 工具链切换应使用 rustup default；组件和 target 使用 rustup component / target 管理。",
  ].join("\n");

  async function act(fn: () => Promise<unknown>, ok: string, key: string, action: string): Promise<boolean> {
    setBusy(key);
    try {
      await runBusy({
        title: action,
        message: `正在${action}并验证 Rust 工具链状态。`,
      }, async () => {
        await fn();
        await load();
      });
      toast(ok, "ok");
      void notices.checkNow("rust-action").catch(() => undefined);
      return true;
    } catch (e) {
      toast(`${action}失败：` + e, "err");
      return false;
    } finally {
      setBusy("");
    }
  }

  async function installRustup() {
    try {
      await runBusy({
        title: "安装 rustup",
        message: "正在安装 Rust 工具链管理器 rustup。安装完成后可继续选择具体工具链版本。",
        progressEvent: "install-progress",
        cancel: { label: "取消", onCancel: () => { invoke("op_cancel").catch(() => {}); } },
      }, async () => {
        await invoke("rustup_install_self", { sourceUrl });
        await load();
      });
      setSrcKey((k) => k + 1);
      toast("rustup 已安装，新终端生效", "ok");
      void notices.checkNow("rust-tool").catch(() => undefined);
    } catch (e) {
      toast(operationWasCancelled(e) ? "已取消安装 rustup" : "安装 rustup 失败：" + e, operationWasCancelled(e) ? "info" : "err");
    }
  }

  async function updateToolchains() {
    try {
      await runBusy({
        title: "检查 Rust 工具链更新",
        message: `正在通过「${sourceName(source)}」检查已安装工具链的更新；stable、beta、nightly 等渠道工具链可能会升级，固定版本通常不会变更。`,
        progressEvent: "install-progress",
        cancel: { label: "取消", onCancel: () => { invoke("op_cancel").catch(() => {}); } },
      }, async () => {
        await invoke("rustup_update", { sourceUrl });
        await load();
      });
      toast("Rust 工具链检查完成", "ok");
      void notices.checkNow("rust-tool-update").catch(() => undefined);
    } catch (e) {
      toast(operationWasCancelled(e) ? "已取消检查 Rust 工具链更新" : "检查 Rust 工具链更新失败：" + e, operationWasCancelled(e) ? "info" : "err");
    }
  }

  async function updateRustupManager() {
    try {
      await runBusy({
        title: "更新 rustup 管理器",
        message: "正在检查 rustup 管理器本身的更新。工具链版本不会因此切换。",
        progressEvent: "install-progress",
      }, async () => {
        await invoke("rustup_self_update");
        await load();
      });
      toast("rustup 管理器检查完成", "ok");
    } catch (e) {
      toast("更新 rustup 管理器失败：" + e, "err");
    }
  }

  async function loadAddons() {
    setAddonLoading(true);
    try {
      const [componentRows, targetRows] = await Promise.all([
        invoke<RustupAddon[]>("rustup_components"),
        invoke<RustupAddon[]>("rustup_targets"),
      ]);
      setComponents(componentRows);
      setTargets(targetRows);
    } catch (e) {
      toast("读取 Rust 组件与编译目标失败：" + e, "err");
    } finally {
      setAddonLoading(false);
    }
  }

  async function openAddons() {
    setAddonsOpen(true);
    await loadAddons();
  }

  async function changeAddon(kind: "component" | "target", item: RustupAddon) {
    const key = `${kind}:${item.name}`;
    setAddonBusy(key);
    try {
      await runBusy({
        title: `${item.installed ? "卸载" : "安装"} ${item.name}`,
        message: `正在${item.installed ? "卸载" : "安装"} Rust ${kind === "component" ? "组件" : "编译目标"}。`,
        progressEvent: "install-progress",
      }, () => invoke(kind === "component" ? "rustup_component_set" : "rustup_target_set", {
        name: item.name,
        install: !item.installed,
        sourceUrl,
      }));
      await loadAddons();
      toast(`${item.name} 已${item.installed ? "卸载" : "安装"}`, "ok");
    } catch (e) {
      toast(`${item.installed ? "卸载" : "安装"} ${item.name} 失败：` + e, "err");
    } finally {
      setAddonBusy("");
    }
  }

  function applySource(next = pendingSource) {
    if (!sources.some((item) => item.id === next)) return;
    setSource(next);
    setPendingSource(next);
    localStorage.setItem(SOURCE_KEY, next);
    setVersions(null);
    toast(`已应用 Rust 工具链下载源：${sourceName(next)}`, "ok");
    notices.checkNow("rust-download-source").catch(() => undefined);
  }

  async function speedtestSources() {
    const hosts = [...new Set(sources.map((item) => item.host).filter(Boolean))];
    if (!hosts.length) {
      toast("没有可测速的 Rust 工具链下载源", "info");
      return;
    }
    try {
      const rows = await runBusy({
        title: "Rust 工具链下载源测速",
        message: "正在比较各下载源连接延迟；单个主机 1500ms 无响应视为超时。",
      }, () => invoke<SourcePing[]>("speedtest_hosts", { hosts }));
      const byHost: Record<string, number | null> = {};
      rows.forEach((row) => { byHost[row.host] = row.ms; });
      const bySource: Record<string, number | null> = {};
      sources.forEach((item) => { bySource[item.id] = byHost[item.host] ?? null; });
      setSourcePings(bySource);
      const fastest = sources
        .map((item) => ({ ...item, ms: bySource[item.id] }))
        .filter((item): item is Mirror & { ms: number } => typeof item.ms === "number")
        .sort((a, b) => a.ms - b.ms)[0];
      if (fastest) {
        setPendingSource(fastest.id);
        toast(fastest.id === source
          ? `测速完成，${fastest.name} 已是当前下载源`
          : `测速完成，已预选 ${fastest.name}，点击「应用」后生效`, "ok");
      } else {
        toast("下载源测速均超时，保留当前下载源", "info");
      }
    } catch (e) {
      toast("下载源测速失败。请检查网络连接后重试。原因：" + e, "err");
    }
  }

  const sourceOptions = useMemo(() => {
    const fastest = Object.entries(sourcePings)
      .filter(([, ms]) => typeof ms === "number")
      .sort((a, b) => (a[1] as number) - (b[1] as number))[0]?.[0];
    return sources.map((item) => {
      const ms = sourcePings[item.id];
      const suffix = !(item.id in sourcePings)
        ? ""
        : ms === null ? " · 超时" : ` · ${ms}ms${item.id === fastest ? " · 最快" : ""}`;
      return { value: item.id, label: `${item.name}${suffix}` };
    });
  }, [sources, sourcePings]);

  async function openInstall() {
    setInstallOpen(true);
    setVersions(null);
    try {
      const rows = await runBusy({
        title: "获取 Rust 工具链版本",
        message: `正在读取「${sourceName(source)}」可安装的 Rust 工具链版本。`,
      }, () => invoke<string[]>("rust_versions", { sourceUrl }));
      setVersions(rows);
    } catch (e) {
      setVersions([]);
      toast("获取 Rust 工具链版本失败。请切换下载源后重试。原因：" + e, "err");
    }
  }

  function filteredVersions(list: string[]) {
    let rows = onlyStable ? list.filter(stableVersion) : list;
    if (latestOnly) {
      const best = new Map<string, string>();
      for (const version of rows) {
        const key = minorLine(version);
        const cur = best.get(key);
        if (!cur || cmpVer(version, cur) > 0) best.set(key, version);
      }
      rows = [...best.values()];
    }
    const sorted = rows.filter(stableVersion).sort((a, b) => cmpVer(b, a));
    return onlyStable ? ["stable", ...sorted] : ["stable", "beta", "nightly", ...sorted];
  }

  async function installToolchain(version: string) {
    const target = version.trim();
    if (!target) return;
    setInstallOpen(false);
    try {
      await runBusy({
        title: `安装 Rust ${target}`,
        message: `正在通过「${sourceName(source)}」下载并安装 Rust 工具链。`,
        progressEvent: "install-progress",
        cancel: { label: "取消安装", onCancel: () => { invoke("op_cancel").catch(() => {}); } },
      }, async () => {
        await invoke("rustup_install", { channel: target, sourceUrl, setDefault: installSetDefault });
        await load();
      });
      toast(installSetDefault ? `已安装 Rust ${target} 并设为默认工具链` : `已安装 Rust ${target}`, "ok");
      void notices.checkNow("rust-install").catch(() => undefined);
    } catch (e) {
      toast(operationWasCancelled(e) ? `已取消安装 Rust ${target}` : `安装 Rust ${target} 失败：` + e, operationWasCancelled(e) ? "info" : "err");
    }
  }

  if (loadErr) {
    return <ErrorState title="暂时无法读取 Rust 环境" description="请确认 rustup 与 Cargo 安装目录可访问，然后重试。" onRetry={async () => { await load(); setLoadErr(false); }} />;
  }
  const ruLoading = !ru;
  const ruState: RustupStatus = ru ?? { installed: false, rustup_version: null, toolchains: [], default: null, default_version: null };

  const visibleVersions = versions ? filteredVersions(versions) : null;

  return (
    <>
      {ruState.installed && (
        <TerminalBar
          avail={shells}
          ecosystem="rust"
          tip="Rust 命令通过 rustup 与 Cargo 的 PATH 生效。可打开终端验证当前工具链，或复制摘要给 AI。"
          action={<EcoActions ecosystem="rust" shells={shells} summary={rustSummary} />}
        />
      )}

      <div className="srcrow" style={{ marginBottom: 10 }}>
        <span className="av rs"><i className="ti ti-download" /></span>
        <div className="mt">
          <div className="t">Rust 工具链下载源 {updateHint && <span className="bd r update-badge" title={updateTitle}>发现新版本</span>}</div>
          <div className="s dim" title="用于安装和更新 Rust 工具链；测速后点击「应用」生效。">用于安装和更新 Rust；测速后点击「应用」生效。</div>
        </div>
        <Select value={pendingSource} width={220} onChange={setPendingSource} options={sourceOptions} />
        <button className="gh sm" onClick={speedtestSources}><i className="ti ti-bolt" /> 测速</button>
        <button className="pr sm" onClick={() => applySource()} title={pendingSource === source ? `当前已应用：${sourceName(source)}` : `应用后将使用：${sourceName(pendingSource)}`}>
          <i className="ti ti-check" /> 应用
        </button>
      </div>

      <div className="grouphd">
        <span className="gt"><i className="ti ti-stack-2" /> 工具链 <span className="cnt">{ruLoading ? "检测中" : ruState.installed ? `rustup · ${ruState.toolchains.length} 个` : "rustup"}</span></span>
        {ruState.installed && (
          <div className="ghr">
            <button className="gh xs" onClick={async () => { await load(); toast("Rust 状态已刷新", "ok"); }}><i className="ti ti-refresh" /> 刷新状态</button>
            <button className="gh xs" title="检查 rustup 管理器本身的更新" onClick={updateRustupManager}><i className="ti ti-tool" /> 管理器更新</button>
            <button className="gh xs" title="检查 stable、beta、nightly 等渠道工具链更新；固定版本通常不会升级到新版本。" onClick={updateToolchains}><i className="ti ti-cloud-download" /> 检查更新</button>
            <button className="gh xs" onClick={openAddons}><i className="ti ti-components" /> 组件与目标</button>
            <button className="pr sm" onClick={openInstall}><i className="ti ti-plus" /> 安装工具链</button>
          </div>
        )}
      </div>

      {ruState.probe_error && (
        <div className="banner amber"><i className="ti ti-alert-triangle lead" /><div className="bt"><b>已检测到 rustup，但状态读取异常。</b><br />{ruState.probe_error}。请确认安全软件未拦截 Rust 工具链后刷新。</div></div>
      )}

      {ruLoading ? (
        <Loading text="正在检测 rustup、已安装工具链和默认工具链…" />
      ) : !ruState.installed ? (
        <div className="banner blue" style={{ flexDirection: "column", alignItems: "stretch", gap: 9 }}>
          <div style={{ display: "flex", gap: 11, alignItems: "flex-start" }}>
            <i className="ti ti-download lead" />
            <div className="bt"><b>安装 Rust 工具链管理器</b><br />Stacker 会安装 rustup。安装完成后，可在本页选择并安装具体 Rust 工具链版本。</div>
          </div>
          <div style={{ paddingLeft: 29 }}>
            <button className="pr sm" onClick={installRustup}><i className="ti ti-download" /> 一键安装 rustup</button>
          </div>
        </div>
      ) : ruState.probe_error ? null : ruState.toolchains.length === 0 ? (
        <div className="banner gray"><i className="ti ti-info-circle lead" /><div className="bt">尚未安装 Rust 工具链。点击「安装工具链」选择版本。</div></div>
      ) : ruState.toolchains.map((t) => (
        <div className={"vrow" + (t.is_default ? " cur" : "")} key={t.name}>
          <span className="ver">{t.name}</span>
          <span className="meta">{t.is_default ? "默认工具链（rustup default）" : "已安装"}</span>
          <div className="acts">
            {t.is_default
              ? <>
                <span className="live"><i className="ti ti-circle-check" /> 默认</span>
                <button className="gh xs" disabled={!!busy} title="重新写入 rustup default 并刷新"
                  onClick={() => act(() => invoke("rustup_set_default", { name: t.name }), "已重新应用默认工具链", "d" + t.name, "设置默认工具链")}>
                  <i className="ti ti-refresh" /> 重新应用
                </button>
              </>
              : <button className="pr sm" disabled={!!busy} onClick={() => act(() => invoke("rustup_set_default", { name: t.name }), "已设为默认工具链：" + t.name, "d" + t.name, "设置默认工具链")}>设为默认</button>}
            <button className="gh xs danger" disabled={!!busy} title="删除此工具链" onClick={() => setUninstall(t.name)}><i className="ti ti-trash" /></button>
          </div>
        </div>
      ))}

      <div className="callout"><i className="ti ti-info-circle" /><div>rustfmt、clippy 和交叉编译目标由 rustup 管理，可通过上方「组件与目标」查看和调整。</div></div>

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-package" /> Cargo 源 <span className="cnt">CARGO_HOME/config.toml</span></span>
        <span className="hint2">写入当前用户配置，改动前自动备份</span>
      </div>
      {ruLoading ? <Loading text="正在读取 Cargo 镜像配置…" /> : <SourcesPanel toolIds={["cargo"]} refresh={srcKey} />}

      {installOpen && (
        <Modal title="安装 Rust 工具链" icon="ti-plus" onClose={() => setInstallOpen(false)}
          sub={<div style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
            <label className="ck"><input type="checkbox" checked={onlyStable} onChange={(e) => { const next = e.target.checked; setOnlyStable(next); localStorage.setItem(RUST_FILTER_KEYS.onlyStable, String(next)); }} /> 仅正式发布版</label>
            <label className="ck"><input type="checkbox" checked={latestOnly} onChange={(e) => { const next = e.target.checked; setLatestOnly(next); localStorage.setItem(RUST_FILTER_KEYS.latestOnly, String(next)); }} /> 仅各小版本最新</label>
            <label className="ck"><input type="checkbox" checked={installSetDefault} onChange={(e) => setInstallSetDefault(e.target.checked)} /> 安装后设为默认工具链</label>
            <span style={{ color: "var(--mut)" }}>下载源：{sourceName(source)}</span>
          </div>}>
          <div className="field">
            <label>手动输入</label>
            <div className="row" style={{ display: "flex", gap: 8 }}>
              <input className="ip full" value={manualVersion} onChange={(e) => setManualVersion(e.target.value)} placeholder="例如 1.97.0、stable、beta、nightly" />
              <button className="pr sm" disabled={!manualVersion.trim()} onClick={() => installToolchain(manualVersion)}>安装</button>
            </div>
            <div className="hint">支持 rustup 原生工具链名称；常用正式版本可直接从下方列表安装。</div>
          </div>
          {!visibleVersions ? <div style={{ color: "var(--mut)", fontSize: 13 }}>正在获取版本列表…</div>
            : <div style={{ maxHeight: 300, overflow: "auto", display: "flex", flexDirection: "column", gap: 5 }}>
              {visibleVersions.length === 0 && <div style={{ color: "var(--mut)", fontSize: 13, padding: 8 }}>当前下载源没有匹配的工具链版本。</div>}
              {visibleVersions.map((version) => {
                const has = ruState.toolchains.some((toolchain) => toolchain.name === version);
                return (
                  <div className="vrow" key={version} style={{ margin: 0 }}>
                    <span className="ver">{version}</span>
                    <span className="meta">{has ? "已安装" : `通过 ${sourceName(source)} 安装`}</span>
                    <div className="acts">
                      {has ? <span className="live"><i className="ti ti-circle-check" /> 已安装</span> : <button className="gh xs" onClick={() => installToolchain(version)}>安装</button>}
                    </div>
                  </div>
                );
              })}
            </div>}
          <div className="banner gray" style={{ margin: 0 }}>
            <i className="ti ti-info-circle lead" />
            <div className="bt">Rust 工具链默认由 rustup 管理，设为默认不需要管理员权限；新打开的终端生效。</div>
          </div>
        </Modal>
      )}

      {addonsOpen && (
        <Modal title="Rust 组件与编译目标" icon="ti-components" onClose={() => !addonBusy && setAddonsOpen(false)}
          sub="组件随默认工具链安装；编译目标用于构建其他平台或架构的程序。">
          <div className="seg" style={{ marginBottom: 12 }}>
            <button className={addonTab === "component" ? "on" : ""} onClick={() => setAddonTab("component")}>组件</button>
            <button className={addonTab === "target" ? "on" : ""} onClick={() => setAddonTab("target")}>编译目标</button>
          </div>
          {addonLoading ? <Loading text="正在读取 Rust 组件与编译目标…" /> : (
            <div style={{ maxHeight: 360, overflow: "auto", display: "flex", flexDirection: "column", gap: 6 }}>
              {(addonTab === "component" ? components : targets).map((item) => (
                <div className="vrow" key={item.name}>
                  <span className="ver" title={item.name}>{item.name}</span>
                  <span className="meta">{item.installed ? "已安装" : "未安装"}</span>
                  <div className="acts">
                    <button className={item.installed ? "gh xs danger" : "pr sm"} disabled={!!addonBusy}
                      onClick={() => changeAddon(addonTab, item)}>
                      <i className={`ti ${item.installed ? "ti-trash" : "ti-download"}`} />
                      {item.installed ? "卸载" : "安装"}
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </Modal>
      )}

      {uninstall && (
        <ConfirmModal title={"卸载工具链 " + uninstall} icon="ti-trash" danger
          message={<>将运行 <span className="code">rustup toolchain uninstall {uninstall}</span>，删除该工具链及其组件 / target。此操作不可撤销。</>}
          confirmLabel={busy === "u" + uninstall ? "卸载中…" : "确认卸载"} busy={busy === "u" + uninstall}
          onConfirm={async () => { if (await act(() => invoke("rustup_uninstall", { name: uninstall }), "已卸载 " + uninstall, "u" + uninstall, "卸载工具链")) setUninstall(null); }}
          onClose={() => setUninstall(null)} />
      )}
    </>
  );
}
