import { useEffect, useRef, useState } from "react";
import { invoke } from "../invoke";
import { open } from "@tauri-apps/plugin-dialog";
import { useToast, Modal, useBusy, Loading, ErrorState, operationWasCancelled } from "../ui";
import { TerminalBar } from "../TerminalBar";
import { EcoActions, type Shells, summaryLine } from "../EcoActions";
import { useNotifications } from "../notifications";

type SdkVersion = { kind: string; version: string; vendor: string; path: string; current: boolean; arch?: string; origin?: "managed" | "external" | "tool-bundled" | "project" | "unknown"; can_delete?: boolean };
const archLabel = (a?: string) => (a === "x64" ? "64 位" : a === "x86" ? "32 位" : a === "ARM64" ? "ARM64" : "");
type SdkGroup = { kind: string; label: string; current_desc: string; versions: SdkVersion[] };
type DriveInfo = { letter: string; fixed: boolean };
type ScanResult = { java: SdkVersion[]; python: SdkVersion[]; node: SdkVersion[]; go: SdkVersion[] };
type JavaEff = { cmd_version: string | null; cmd_major: string | null; home_path: string | null; home_version: string | null; home_major: string | null; split: boolean };
type HostPing = { host: string; ms: number | null };

const JAVA_VENDOR_STORAGE = "stacker.java.vendor";
const JAVA_VENDORS = {
  temurin: { label: "Temurin（Adoptium）", source: "清华 Temurin", host: "mirrors.tuna.tsinghua.edu.cn" },
  zulu: { label: "Zulu（Azul）", source: "Azul Zulu", host: "cdn.azul.com" },
  dragonwell: { label: "Dragonwell（阿里 · 标准版）", source: "阿里 Dragonwell", host: "dragonwell.oss-cn-shanghai.aliyuncs.com" },
} as const;
type JavaVendor = keyof typeof JAVA_VENDORS;

function cmpJavaVer(a: string, b: string) {
  const nums = (v: string) => (v.match(/\d+/g) ?? []).map((n) => Number(n) || 0);
  const pa = nums(a);
  const pb = nums(b);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d) return d;
  }
  return 0;
}

function normalizeJavaVersion(version: string) {
  return version.replace(/^jdk-?/i, "").replace(/[_+].*$/, "");
}

export default function Java() {
  const toast = useToast();
  const runBusy = useBusy();
  const notices = useNotifications();
  const [grp, setGrp] = useState<SdkGroup | null>(null);
  const [eff, setEff] = useState<JavaEff | null>(null);
  const [loadErr, setLoadErr] = useState(false);
  const [scanned, setScanned] = useState<SdkVersion[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [excludeIde, setExcludeIde] = useState(true); // 默认不扫描 IDE 自带 JDK
  const [sysConfigured, setSysConfigured] = useState(false);
  const [dlg, setDlg] = useState<SdkVersion | null>(null);
  const [removeDlg, setRemoveDlg] = useState<SdkVersion | null>(null);
  const [scope, setScope] = useState<"user" | "system">("user");
  const [busy, setBusy] = useState(false);
  const [downloadOpen, setDownloadOpen] = useState(false);
  const [dlSetDef, setDlSetDef] = useState(true);
  const [dlScope, setDlScope] = useState<"user" | "system">("user");
  const [dlVer, setDlVer] = useState("21");
  const [vendor, setVendor] = useState<JavaVendor>(() => {
    const saved = localStorage.getItem(JAVA_VENDOR_STORAGE);
    return saved && saved in JAVA_VENDORS ? saved as JavaVendor : "temurin";
  });
  const [vendorPings, setVendorPings] = useState<Partial<Record<JavaVendor, number | null>>>({});
  const [vendorTesting, setVendorTesting] = useState(false);
  const [arch, setArch] = useState<"x64" | "x32">("x64");
  const [appDir, setAppDir] = useState("");
  const [dlDest, setDlDest] = useState("D:\\Environments\\temurin-21-x64");
  const [shells, setShells] = useState<Shells>({ powershell: true, gitbash: false, cmd: true });

  // 仅 8/11/17 有 32 位构建；Dragonwell 只有 x64；Temurin/Zulu 有 32 位
  const X32_VERS = ["8", "11", "17"];
  const supportsX32 = (v: string, vd: string) => (vd === "temurin" || vd === "zulu") && X32_VERS.includes(v);
  const archSuffix = (a: string) => (a === "x32" ? "x86" : "x64");
  // 默认安装到「工具目录\jdk\<发行版>-<版本>-<位数>」，位数分目录避免同名覆盖
  const destFor = (v: string, vd: string = vendor, a: string = arch) =>
    (appDir ? `${appDir}\\jdk\\${vd}-${v}-${archSuffix(a)}` : `D:\\Environments\\${vd}-${v}-${archSuffix(a)}`);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { invoke<string>("app_dir").then((d) => { setAppDir(d); setDlDest((d ? `${d}\\jdk\\` : "D:\\Environments\\") + `temurin-${dlVer}-x64`); }).catch(() => {}); }, []);
  function pickVer(v: string) {
    const a = supportsX32(v, vendor) ? arch : "x64";
    setDlVer(v); setArch(a); setDlDest(destFor(v, vendor, a));
  }
  function pickVendor(vd: JavaVendor) {
    const a = supportsX32(dlVer, vd) ? arch : "x64";
    setVendor(vd); localStorage.setItem(JAVA_VENDOR_STORAGE, vd); setArch(a); setDlDest(destFor(dlVer, vd, a));
  }
  function pickArch(a: "x64" | "x32") { setArch(a); setDlDest(destFor(dlVer, vendor, a)); }
  async function browseDest() {
    const dir = await open({ directory: true, defaultPath: appDir || undefined });
    if (typeof dir === "string") setDlDest(`${dir}\\${vendor}-${dlVer}-${archSuffix(arch)}`);
  }

  async function testVendors() {
    if (vendorTesting) return;
    setVendorTesting(true);
    try {
      const fastest = await runBusy({
        title: "测试 Java 下载源",
        message: "正在比较 Temurin、Zulu 与 Dragonwell 下载端点的连接延迟；单项超过 1.5 秒未连接将记为超时。",
      }, async () => {
        const rows = await invoke<HostPing[]>("speedtest_hosts", { hosts: Object.values(JAVA_VENDORS).map((item) => item.host) });
        const latency = new Map(rows.map((row) => [row.host, row.ms]));
        const next = Object.fromEntries(Object.entries(JAVA_VENDORS).map(([id, item]) => [id, latency.get(item.host) ?? null])) as Partial<Record<JavaVendor, number | null>>;
        setVendorPings(next);
        return (Object.keys(JAVA_VENDORS) as JavaVendor[])
          .filter((id) => typeof next[id] === "number")
          .sort((a, b) => (next[a] as number) - (next[b] as number))[0];
      });
      if (!fastest) {
        toast("未检测到可连接的 Java 下载源，已保留当前选择", "info");
        return;
      }
      pickVendor(fastest);
      toast(`测速完成，已选择 ${JAVA_VENDORS[fastest].source}`, "ok");
    } catch (error) {
      toast("Java 下载源测速失败：" + error, "err");
    } finally {
      setVendorTesting(false);
    }
  }

  function vendorButtonLabel(id: JavaVendor) {
    const ms = vendorPings[id];
    return `${JAVA_VENDORS[id].label}${typeof ms === "number" ? ` · ${ms}ms` : ms === null && id in vendorPings ? " · 超时" : ""}`;
  }

  async function load() {
    const groups = await invoke<SdkGroup[]>("env_state");
    setGrp(groups.find((g) => g.kind === "java") ?? null);
    invoke<Record<string, boolean>>("env_system_info").then((m) => setSysConfigured(!!m.java)).catch(() => {});
    invoke<JavaEff>("env_java_effective").then(setEff).catch(() => setEff(null));
    invoke<Shells>("shells_available").then(setShells).catch(() => {});
  }
  useEffect(() => { load().catch(() => setLoadErr(true)); }, []);

  const versions = scanned ?? grp?.versions ?? [];
  const current = versions.find((v) => v.current);
  const rawUpdateHint = notices.ecosystemUpdates.find((item) => item.id === "java");
  const updateHint =
    rawUpdateHint && current && cmpJavaVer(normalizeJavaVersion(current.version), rawUpdateHint.latest) >= 0
      ? undefined
      : rawUpdateHint;
  const updateTitle = updateHint ? `发现新版本：当前 ${updateHint.current}，最新 ${updateHint.latest}` : undefined;

  const cancelledRef = useRef(false);
  async function scan() {
    cancelledRef.current = false;
    setScanning(true);
    const drives = await invoke<DriveInfo[]>("list_drives").catch(() => [] as DriveInfo[]);
    const roots = drives.filter((d) => d.fixed).map((d) => d.letter + "\\");
    try {
      const r = await runBusy({
        title: "扫描磁盘上的 JDK",
        message: "正在遍历本机固定磁盘并识别 JDK。可随时取消扫描，已有列表不会被覆盖。",
        progressEvent: "env-scan-progress",
        cancel: { label: "取消扫描", onCancel: cancelScan },
      }, async () => {
        const result = await invoke<ScanResult>("env_scan", { roots, excludeIdeJdk: excludeIde, excludeToolBundled: excludeIde, kinds: ["java"] });
        if (!cancelledRef.current) await load();
        return result;
      });
      if (cancelledRef.current) return; // 已取消：丢弃部分结果，原列表不动
      setScanned(r.java);
      toast(`扫描完成，发现 ${r.java.length} 个 JDK`, "ok");
    } catch (e) { toast("扫描失败：" + e, "err"); }
    finally { setScanning(false); }
  }
  async function cancelScan() { cancelledRef.current = true; await invoke("env_cancel").catch(() => {}); }

  async function refreshState() {
    try {
      await runBusy({ title: "刷新 Java 状态", message: "正在重新检测 java 命令、JAVA_HOME 和已登记的 JDK 目录。" }, async () => {
        setScanned(null);
        await load();
      });
      toast("Java 状态已刷新", "ok");
    } catch (error) {
      toast("刷新 Java 状态失败：" + error, "err");
    }
  }

  async function removeManagedVersion() {
    if (!removeDlg) return;
    const target = removeDlg;
    setBusy(true);
    try {
      await runBusy({
        title: `删除 JDK ${target.version}`,
        message: target.current
          ? "正在删除安装目录并清除指向该版本的 JAVA_HOME 与 PATH；如包含系统级配置，Windows 将请求管理员授权。"
          : "正在删除由 Stacker 安装的 JDK 目录。",
      }, async () => {
        await invoke("env_remove_managed", { kind: "java", path: target.path });
        setScanned(null);
        await load();
      });
      setRemoveDlg(null);
      toast(`JDK ${target.version} 已删除`, "ok");
      notices.checkNow("java-remove").catch(() => undefined);
    } catch (error) {
      toast(`删除 JDK ${target.version} 失败：${error}`, "err");
    } finally {
      setBusy(false);
    }
  }

  // 命令行与 JAVA_HOME 版本不一致 → 把默认对齐到 JAVA_HOME（默认系统级，压过系统 PATH 里的 javapath）
  function fixSplit() {
    if (!eff?.home_path) return;
    setScope("system");
    setDlg({ kind: "java", version: eff.home_version ?? eff.home_major ?? "?", vendor: "", path: eff.home_path, current: false });
  }

  async function applyDefault() {
    if (!dlg) return;
    const picked = dlg;
    setBusy(true);
    try {
      const cmd = scope === "system" ? "env_set_default_system" : "env_set_default";
      await runBusy({
        title: "设置默认 JDK",
        message: `正在写入${scope === "system" ? "系统级" : "当前用户"} JAVA_HOME 与 PATH，并验证新配置。`,
      }, async () => {
        await invoke(cmd, { kind: "java", path: picked.path, siblings: versions.map((v) => v.path) });
        await load();
      });
      setScanned((s) => s ? s.map((v) => ({ ...v, current: v.path === picked.path })) : s);
      setDlg(null);
      toast("已设为默认 JDK" + (scope === "system" ? "（系统级）" : "（用户级）"), "ok");
      void notices.checkNow("java-default").catch(() => undefined);
    } catch (e) { toast("切换失败：" + e, "err"); } finally { setBusy(false); }
  }

  async function downloadJdk() {
    const cmd = vendor === "dragonwell" ? "dragonwell_resolve" : vendor === "zulu" ? "zulu_resolve" : "jdk_resolve";
    const args = vendor === "dragonwell" ? { major: dlVer } : { major: dlVer, arch };
    const label = JAVA_VENDORS[vendor].source;
    setDownloadOpen(false);
    try {
      const asset = await runBusy({
        title: `下载 ${label} JDK ${dlVer}`,
        message: `安装到 ${dlDest}`,
        progressEvent: "install-progress",
        cancel: { label: "取消下载", onCancel: () => { invoke("op_cancel").catch(() => {}); } },
      }, async () => {
        const asset = await invoke<{ version: string; url: string; filename: string }>(cmd, args);
        await invoke("installer_download", { url: asset.url, destDir: dlDest, stripTop: true });
        await invoke("env_register_install", { kind: "java", path: dlDest });
        if (dlSetDef) {
          const setDefaultCmd = dlScope === "system" ? "env_set_default_system" : "env_set_default";
          await invoke(setDefaultCmd, { kind: "java", path: dlDest, siblings: versions.map((v) => v.path) });
        }
        await load();
        return asset;
      });
      setScanned((previous) => {
        const rows = previous ?? versions;
        const installed: SdkVersion = {
          kind: "java",
          version: asset.version,
          vendor: label,
          path: dlDest,
          current: dlSetDef,
          arch: arch === "x32" ? "x86" : "x64",
        };
        return [
          ...rows
            .filter((item) => item.path.toLowerCase() !== dlDest.toLowerCase())
            .map((item) => dlSetDef ? { ...item, current: false } : item),
          installed,
        ];
      });
      toast(`JDK ${asset.version} 已下载解压到 ${dlDest}${dlSetDef ? `，已设为默认（${dlScope === "system" ? "系统级" : "用户级"}）` : ""}`, "ok");
      void notices.checkNow("java-install").catch(() => undefined);
    } catch (e) { toast(operationWasCancelled(e) ? "已取消安装 Java 运行时" : "安装 Java 运行时失败：" + e, operationWasCancelled(e) ? "info" : "err"); }
  }

  if (loadErr) return <ErrorState title="暂时无法读取 JDK 环境" description="请确认 JDK 安装目录与环境变量可访问，然后重试。" onRetry={async () => { await load(); setLoadErr(false); }} />;
  const grpLoading = !grp;

  const javaSummary = [
    "## Java 环境摘要",
    "",
    summaryLine("java 命令", eff?.cmd_version ? `JDK ${eff.cmd_version}` : "未检测到"),
    summaryLine("JAVA_HOME", eff?.home_path ?? current?.path ?? "未配置"),
    summaryLine("JAVA_HOME 版本", eff?.home_version ?? "未检测到"),
    summaryLine("已扫描 JDK", versions.map((v) => `${v.version} (${v.vendor || "Unknown"})`).join("；") || "无"),
    summaryLine("配置范围", sysConfigured ? "包含系统级配置" : "当前用户配置"),
    "",
    "## 给 AI 的使用说明",
    "- 构建 Java 项目前，先执行 java -version、javac -version 并检查 JAVA_HOME。",
    "- Maven / Gradle 可能优先读取 JAVA_HOME；如命令行版本与 JAVA_HOME 不一致，应先修正默认 JDK。",
  ].join("\n");

  return (
    <>
      {grpLoading ? (
        <Loading text="正在检测 java 命令、JAVA_HOME 与已安装 JDK…" />
      ) : (
        <TerminalBar
          avail={shells}
          ecosystem="java"
          tip="Java 命令通过 JAVA_HOME 与 PATH 生效。可打开终端验证当前版本，或复制摘要给 AI。"
          action={<EcoActions ecosystem="java" shells={shells} summary={javaSummary} />}
        />
      )}

      <div className="grouphd">
        <span className="gt">
          <i className="ti ti-coffee" /> 运行时版本 <span className="cnt">{grpLoading ? "检测中" : `${versions.length} 个`}</span>
          {updateHint && <span className="bd r update-badge" title={updateTitle}>发现新版本</span>}
        </span>
        <div className="ghr">
          <label className="ck" style={{ fontSize: 11.5, marginRight: 2 }}
            title="不扫描 IDE 或工具自带的 JDK（JetBrains Runtime、Android Studio、MyEclipse 等）。这些运行时由对应 IDE 管理，通常不作为独立 JDK 使用。">
            <input type="checkbox" checked={excludeIde} disabled={scanning} onChange={(e) => setExcludeIde(e.target.checked)} /> 排除工具自带
          </label>
          <button className="gh xs" disabled={scanning} onClick={refreshState}><i className="ti ti-refresh" /> 刷新状态</button>
          {!scanning
            ? <button className="gh xs" onClick={scan}><i className="ti ti-scan" /> 扫描本机</button>
            : <button className="gh xs" onClick={cancelScan}>取消扫描</button>}
          <button className="pr sm" onClick={() => setDownloadOpen(true)}><i className="ti ti-download" /> 安装新版本</button>
        </div>
      </div>

      <div className="effbox">
        <div className="eh"><i className="ti ti-target" /> 生效情况 <span className="sub">按最新 PATH / 注册表实测</span></div>
        <div className="effrow"><span className="ek"><i className="ti ti-terminal-2" /> 命令 java</span>
          <span className="ev">{grpLoading ? "检测中…" : eff?.cmd_version ? <>JDK <b>{eff.cmd_version}</b></> : current ? <>JDK <b>{current.version}</b></> : "未检测到 java 命令"}</span>
          {eff?.split ? <span className="bd r">被 PATH 抢占</span> : eff?.cmd_version ? <span className="bd g">生效中</span> : null}</div>
        <div className="effrow"><span className="ek"><i className="ti ti-variable" /> JAVA_HOME</span>
          <span className="ev">{eff?.home_version && <>JDK <b>{eff.home_version}</b> · </>}<span className="mono">{grpLoading ? "检测中…" : eff?.home_path ?? current?.path ?? "—"}</span></span>
          {sysConfigured && <span className="bd w">含系统级</span>}</div>
        {eff?.split && (
          <div className="banner" style={{ margin: "10px 0 0", boxShadow: "inset 3px 0 0 var(--amber)", borderColor: "rgba(228,180,80,.3)" }}>
            <i className="ti ti-alert-triangle lead" style={{ color: "var(--amber)" }} />
            <div className="bt">命令行实际用 <b>JDK {eff.cmd_major}</b>，但 JAVA_HOME 指向 <b>JDK {eff.home_major}</b>（Maven / IDE 走 JAVA_HOME）。多因系统 PATH 里的 Oracle <span className="code">javapath</span> 抢在前面。
              <button className="pr sm" style={{ marginTop: 8 }} onClick={fixSplit}><i className="ti ti-arrows-join" /> 对齐到 JAVA_HOME（JDK {eff.home_major}）</button></div>
          </div>
        )}
      </div>

      {grpLoading ? (
        <Loading text="正在读取 JDK 列表…" />
      ) : versions.length === 0 ? (
        <div className="stub"><div className="si"><i className="ti ti-coffee-off" /></div><h2>未检测到 JDK</h2>
          <p>可扫描磁盘识别已有 JDK，也可直接下载并安装所需版本。</p></div>
      ) : versions.map((v) => (
        <div className={"vrow" + (v.current ? " cur" : "")} key={v.path}>
          <span className="ver">{v.version}</span>
          {v.arch && <span className="bd n" style={{ marginRight: 6 }}>{archLabel(v.arch)}</span>}
          <span className="meta">{v.vendor} · {v.path}</span>
          <div className="acts">
            {v.origin === "managed" && <span className="bd g">Stacker 安装</span>}
            {v.origin === "tool-bundled" && <span className="bd n">工具自带</span>}
            {v.current && <span className="live"><i className="ti ti-circle-check" /> 生效中</span>}
            {v.current
              ? <button className="gh xs" title="重新写入 JAVA_HOME / PATH 并刷新（命令被 PATH 抢占时可修复）" onClick={() => { setScope("user"); setDlg(v); }}><i className="ti ti-refresh" /> 重新应用</button>
              : <button className="pr sm" onClick={() => { setScope("user"); setDlg(v); }}>设为默认</button>}
            {v.can_delete && <button className="gh xs danger" title="删除此版本" onClick={() => setRemoveDlg(v)}><i className="ti ti-trash" /></button>}
          </div>
        </div>
      ))}

      <div className="callout"><i className="ti ti-info-circle" /><div>本页管理 Java 运行时以及 <span className="code">JAVA_HOME</span> / <span className="code">PATH</span>。Java 依赖仓库请在 Maven 或 Gradle 页面配置。</div></div>

      {dlg && (
        <Modal wide title={`把默认 JDK 切到 ${dlg.version} · ${dlg.vendor}`} icon="ti-coffee" onClose={() => setDlg(null)}
          sub={<b style={{ color: "var(--tx)" }}>JAVA_HOME 和 PATH 始终一起改、同级同步</b>}
          footer={<>
            <button className="gh sm" onClick={() => setDlg(null)} disabled={busy}>取消</button>
            <button className="pr" style={{ background: "#d97a1f" }} onClick={applyDefault} disabled={busy}>
              <i className="ti ti-shield-half" /> {busy ? "应用中…" : scope === "system" ? "应用（将触发 UAC 提权）" : "应用"}</button>
          </>}>
          <div className="field"><label>作用范围</label>
            <div className={"opt" + (scope === "user" ? " sel" : "")} onClick={() => setScope("user")}><span className="rd" />
              <div><div className="ot">仅当前用户 <span className="bd n" style={{ fontSize: 10 }}>免管理员</span></div>
                <div className="od">写 HKCU：用户级 JAVA_HOME + 用户 PATH。无需 UAC 提权。</div></div></div>
            <div className={"opt" + (scope === "system" ? " sel" : "")} onClick={() => setScope("system")}><span className="rd" />
              <div><div className="ot"><i className="ti ti-shield-lock" style={{ color: "#f5a45a" }} /> 系统全局 <span className="bd w" style={{ fontSize: 10 }}>需管理员 · 需 UAC 提权</span></div>
                <div className="od">写 HKLM：所有用户生效。命令被系统 PATH 覆盖时选它。</div></div></div>
          </div>
          <div className="banner gray" style={{ margin: 0 }}><i className="ti ti-history lead" /><div className="bt">改动前自动备份 JAVA_HOME 与 PATH，可在「历史」还原。</div></div>
        </Modal>
      )}

      {removeDlg && (
        <Modal title={`删除 JDK ${removeDlg.version}`} icon="ti-trash" onClose={() => !busy && setRemoveDlg(null)}
          footer={<>
            <button className="gh sm" disabled={busy} onClick={() => setRemoveDlg(null)}>取消</button>
            <button className="danger sm" disabled={busy} onClick={removeManagedVersion}><i className="ti ti-trash" /> {busy ? "删除中…" : "确认删除"}</button>
          </>}>
          <div className="banner red" style={{ margin: 0 }}>
            <i className="ti ti-alert-triangle lead" />
            <div className="bt">将永久删除 <span className="mono">{removeDlg.path}</span>。{removeDlg.current ? "该版本当前生效，JAVA_HOME 与 PATH 中的相关配置也会一并清除。" : "此操作不可撤销。"}</div>
          </div>
        </Modal>
      )}

      {downloadOpen && (
        <Modal wide title="安装 Java 运行时" icon="ti-download" onClose={() => setDownloadOpen(false)}
          footer={<>
            <button className="gh sm" onClick={() => setDownloadOpen(false)}>取消</button>
            <button className="pr sm" disabled={!dlDest.trim()} onClick={downloadJdk}><i className="ti ti-download" /> 下载并安装</button>
          </>}>
          <div className="field"><label>发行版</label>
            <div className="row" style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              <div className="seg" style={{ alignSelf: "flex-start", flexWrap: "wrap" }}>
                {(Object.keys(JAVA_VENDORS) as JavaVendor[]).map((id) => (
                  <button key={id} className={vendor === id ? "on" : ""} title={`${JAVA_VENDORS[id].source} · ${JAVA_VENDORS[id].host}`} onClick={() => pickVendor(id)}>{vendorButtonLabel(id)}</button>
                ))}
              </div>
              <button className="gh sm" disabled={vendorTesting} onClick={testVendors}><i className={"ti " + (vendorTesting ? "ti-loader spin" : "ti-bolt")} /> {vendorTesting ? "测速中…" : "测速"}</button>
            </div>
          </div>
          <div className="field"><label>版本</label>
            <div className="seg" style={{ alignSelf: "flex-start", flexWrap: "wrap" }}>
              {["25", "21", "17", "11", "8"].map((v) => <button key={v} className={dlVer === v ? "on" : ""} onClick={() => pickVer(v)}>JDK {v}</button>)}</div></div>
          {supportsX32(dlVer, vendor) && (
            <div className="field"><label>位数</label>
              <div className="seg" style={{ alignSelf: "flex-start" }}>
                <button className={arch === "x64" ? "on" : ""} onClick={() => pickArch("x64")}>64 位</button>
                <button className={arch === "x32" ? "on" : ""} onClick={() => pickArch("x32")}>32 位</button></div>
              <div className="hint">仅 JDK 8 / 11 / 17 提供 32 位；21 / 25 与 Dragonwell 只有 64 位。</div></div>
          )}
          <div className="field"><label>安装目录</label>
            <div className="row" style={{ gap: 8, display: "flex" }}>
              <input className="ip full" style={{ flex: 1 }} value={dlDest} onChange={(e) => setDlDest(e.target.value)} />
              <button className="gh sm" onClick={browseDest}><i className="ti ti-folder" /> 浏览…</button>
            </div>
            <div className="hint"><i className="ti ti-info-circle" style={{ color: "#6aa3f5" }} /> 默认保存到 Stacker 工具目录的 <span className="code">jdk</span> 子目录；可选择其他磁盘位置。</div></div>
          <label className="ck" style={{ marginTop: -2 }}><input type="checkbox" checked={dlSetDef} onChange={(e) => setDlSetDef(e.target.checked)} /> 安装后设为默认</label>
          {dlSetDef && (
            <div className="field"><label>作用范围</label>
              <div className={"opt" + (dlScope === "user" ? " sel" : "")} onClick={() => setDlScope("user")}><span className="rd" />
                <div><div className="ot">仅当前用户 <span className="bd n" style={{ fontSize: 10 }}>免管理员</span></div>
                  <div className="od">写入当前用户的 JAVA_HOME 和 PATH，适合个人开发环境。</div></div></div>
              <div className={"opt" + (dlScope === "system" ? " sel" : "")} onClick={() => setDlScope("system")}><span className="rd" />
                <div><div className="ot"><i className="ti ti-shield-lock" style={{ color: "#f5a45a" }} /> 系统全局 <span className="bd w" style={{ fontSize: 10 }}>需管理员 · 需 UAC 提权</span></div>
                  <div className="od">写入系统级 JAVA_HOME 和 PATH，所有用户与系统服务可见。</div></div></div>
            </div>
          )}
          <div className="banner gray" style={{ margin: 0 }}><i className="ti ti-info-circle lead" /><div className="bt">{
            vendor === "dragonwell" ? "将从 Dragonwell 官方下载源获取最新 Windows x64 安装包。"
            : vendor === "zulu" ? "将从 Azul 官方服务获取最新 GA 版本。"
            : "将从清华 TUNA Adoptium 镜像获取所选大版本的最新 GA 补丁版。"}</div></div>
        </Modal>
      )}
    </>
  );
}
