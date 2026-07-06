import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useToast, Modal, useBusy, Loading } from "../ui";

type SdkVersion = { kind: string; version: string; vendor: string; path: string; current: boolean; arch?: string };
const archLabel = (a?: string) => (a === "x64" ? "64 位" : a === "x86" ? "32 位" : a === "ARM64" ? "ARM64" : "");
type SdkGroup = { kind: string; label: string; current_desc: string; versions: SdkVersion[] };
type DriveInfo = { letter: string; fixed: boolean };
type ScanResult = { java: SdkVersion[]; python: SdkVersion[]; node: SdkVersion[]; go: SdkVersion[] };
type JavaEff = { cmd_version: string | null; cmd_major: string | null; home_path: string | null; home_version: string | null; home_major: string | null; split: boolean };

export default function Java() {
  const toast = useToast();
  const runBusy = useBusy();
  const [grp, setGrp] = useState<SdkGroup | null>(null);
  const [eff, setEff] = useState<JavaEff | null>(null);
  const [loadErr, setLoadErr] = useState(false);
  const [scanned, setScanned] = useState<SdkVersion[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [excludeIde, setExcludeIde] = useState(true); // 默认不扫描 IDE 自带 JDK
  const [sysConfigured, setSysConfigured] = useState(false);
  const [dlg, setDlg] = useState<SdkVersion | null>(null);
  const [scope, setScope] = useState<"user" | "system">("user");
  const [busy, setBusy] = useState(false);
  const [downloadOpen, setDownloadOpen] = useState(false);
  const [dlSetDef, setDlSetDef] = useState(true);
  const [dlScope, setDlScope] = useState<"user" | "system">("user");
  const [dlVer, setDlVer] = useState("21");
  const [vendor, setVendor] = useState<"temurin" | "dragonwell" | "zulu">("temurin");
  const [arch, setArch] = useState<"x64" | "x32">("x64");
  const [appDir, setAppDir] = useState("");
  const [dlDest, setDlDest] = useState("D:\\Environments\\temurin-21-x64");

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
  function pickVendor(vd: "temurin" | "dragonwell" | "zulu") {
    const a = supportsX32(dlVer, vd) ? arch : "x64";
    setVendor(vd); setArch(a); setDlDest(destFor(dlVer, vd, a));
  }
  function pickArch(a: "x64" | "x32") { setArch(a); setDlDest(destFor(dlVer, vendor, a)); }
  async function browseDest() {
    const dir = await open({ directory: true, defaultPath: appDir || undefined });
    if (typeof dir === "string") setDlDest(`${dir}\\${vendor}-${dlVer}-${archSuffix(arch)}`);
  }

  async function load() {
    const groups = await invoke<SdkGroup[]>("env_state");
    setGrp(groups.find((g) => g.kind === "java") ?? null);
    invoke<Record<string, boolean>>("env_system_info").then((m) => setSysConfigured(!!m.java)).catch(() => {});
    invoke<JavaEff>("env_java_effective").then(setEff).catch(() => setEff(null));
  }
  useEffect(() => { load().catch(() => setLoadErr(true)); }, []);

  const versions = scanned ?? grp?.versions ?? [];
  const current = versions.find((v) => v.current);

  const cancelledRef = useRef(false);
  async function scan() {
    cancelledRef.current = false;
    setScanning(true);
    const drives = await invoke<DriveInfo[]>("list_drives").catch(() => [] as DriveInfo[]);
    const roots = drives.filter((d) => d.fixed).map((d) => d.letter + "\\");
    try {
      const r = await runBusy({
        title: "扫描磁盘上的 JDK",
        message: "正在遍历各固定磁盘查找已安装的 JDK，请稍候。完成会自动关闭；也可随时「取消扫描」。",
        progressEvent: "env-scan-progress",
        cancel: { label: "取消扫描", onCancel: cancelScan },
      }, () => invoke<ScanResult>("env_scan", { roots, excludeIdeJdk: excludeIde }));
      if (cancelledRef.current) return; // 已取消：丢弃部分结果，原列表不动
      setScanned(r.java); await load();
      toast(`扫描完成，发现 ${r.java.length} 个 JDK`, "ok");
    } catch (e) { toast("扫描失败：" + e, "err"); }
    finally { setScanning(false); }
  }
  async function cancelScan() { cancelledRef.current = true; await invoke("env_cancel").catch(() => {}); }

  // 命令行与 JAVA_HOME 版本不一致 → 把默认对齐到 JAVA_HOME（默认系统级，压过系统 PATH 里的 javapath）
  function fixSplit() {
    if (!eff?.home_path) return;
    setScope("system");
    setDlg({ kind: "java", version: eff.home_version ?? eff.home_major ?? "?", vendor: "", path: eff.home_path, current: false });
  }

  async function applyDefault() {
    if (!dlg) return;
    setBusy(true);
    try {
      const cmd = scope === "system" ? "env_set_default_system" : "env_set_default";
      await invoke(cmd, { kind: "java", path: dlg.path, siblings: versions.map((v) => v.path) });
      toast("已设为默认 JDK" + (scope === "system" ? "（系统级）" : "（用户级）"), "ok");
      const picked = dlg; setDlg(null);
      await load();
      setScanned((s) => s ? s.map((v) => ({ ...v, current: v.path === picked.path })) : s);
    } catch (e) { toast("切换失败：" + e, "err"); } finally { setBusy(false); }
  }

  async function downloadJdk() {
    const cmd = vendor === "dragonwell" ? "dragonwell_resolve" : vendor === "zulu" ? "zulu_resolve" : "jdk_resolve";
    const args = vendor === "dragonwell" ? { major: dlVer } : { major: dlVer, arch };
    const label = vendor === "dragonwell" ? "Dragonwell（阿里）" : vendor === "zulu" ? "Zulu（Azul）" : "Temurin（清华）";
    setDownloadOpen(false);
    try {
      await runBusy({
        title: `下载 ${label} JDK ${dlVer}`,
        message: `安装到 ${dlDest}`,
        progressEvent: "install-progress",
        cancel: { label: "取消下载", onCancel: () => { invoke("op_cancel").catch(() => {}); } },
      }, async () => {
        const asset = await invoke<{ version: string; url: string; filename: string }>(cmd, args);
        await invoke("installer_download", { url: asset.url, destDir: dlDest, stripTop: true });
        if (dlSetDef) {
          const setDefaultCmd = dlScope === "system" ? "env_set_default_system" : "env_set_default";
          await invoke(setDefaultCmd, { kind: "java", path: dlDest, siblings: versions.map((v) => v.path) });
        }
        toast(`JDK ${asset.version} 已下载解压到 ${dlDest}${dlSetDef ? `，已设为默认（${dlScope === "system" ? "系统级" : "用户级"}）` : ""}`, "ok");
      });
      scan(); // 自动扫描把新装的 JDK 纳入列表
    } catch (e) { toast("下载失败：" + e, "err"); }
  }

  if (loadErr) return <div className="stub"><div className="si"><i className="ti ti-plug-x" /></div><h2>读取环境失败</h2><p>请在 Tauri 应用内运行（浏览器预览没有后端）。</p></div>;
  if (!grp) return <Loading text="正在读取 JDK…" />;

  return (
    <>
      <div className="grouphd">
        <span className="gt"><i className="ti ti-coffee" /> JDK 版本 <span className="cnt">{versions.length} 个</span></span>
        <div className="ghr">
          <label className="ck" style={{ fontSize: 11.5, marginRight: 2 }}
            title="不扫描 IDE / 工具自带的 JDK（JetBrains jbr、Android Studio、MyEclipse 等）——它们随 IDE 走，通常不作为独立 JDK 使用">
            <input type="checkbox" checked={excludeIde} disabled={scanning} onChange={(e) => setExcludeIde(e.target.checked)} /> 排除 IDE 自带
          </label>
          {!scanning
            ? <button className="gh xs" onClick={scan}><i className="ti ti-refresh" /> 扫描磁盘</button>
            : <button className="gh xs" onClick={cancelScan}>取消扫描</button>}
          <button className="pr sm" onClick={() => setDownloadOpen(true)}><i className="ti ti-download" /> 下载 JDK</button>
        </div>
      </div>

      <div className="effbox">
        <div className="eh"><i className="ti ti-target" /> 生效情况 <span className="sub">按最新 PATH / 注册表实测</span></div>
        <div className="effrow"><span className="ek"><i className="ti ti-terminal-2" /> 命令 java</span>
          <span className="ev">{eff?.cmd_version ? <>JDK <b>{eff.cmd_version}</b></> : current ? <>JDK <b>{current.version}</b></> : "未检测到 java 命令"}</span>
          {eff?.split ? <span className="bd r">被 PATH 抢占</span> : eff?.cmd_version ? <span className="bd g">生效中</span> : null}</div>
        <div className="effrow"><span className="ek"><i className="ti ti-variable" /> JAVA_HOME</span>
          <span className="ev">{eff?.home_version && <>JDK <b>{eff.home_version}</b> · </>}<span className="mono">{eff?.home_path ?? current?.path ?? "—"}</span></span>
          {sysConfigured && <span className="bd w">含系统级</span>}</div>
        {eff?.split && (
          <div className="banner" style={{ margin: "10px 0 0", boxShadow: "inset 3px 0 0 var(--amber)", borderColor: "rgba(228,180,80,.3)" }}>
            <i className="ti ti-alert-triangle lead" style={{ color: "var(--amber)" }} />
            <div className="bt">命令行实际用 <b>JDK {eff.cmd_major}</b>，但 JAVA_HOME 指向 <b>JDK {eff.home_major}</b>（Maven / IDE 走 JAVA_HOME）。多因系统 PATH 里的 Oracle <span className="code">javapath</span> 抢在前面。
              <button className="pr sm" style={{ marginTop: 8 }} onClick={fixSplit}><i className="ti ti-arrows-join" /> 对齐到 JAVA_HOME（JDK {eff.home_major}）</button></div>
          </div>
        )}
      </div>

      {versions.length === 0 ? (
        <div className="stub"><div className="si"><i className="ti ti-coffee-off" /></div><h2>未检测到 JDK</h2>
          <p>点「扫描磁盘」发现已装 JDK，或「下载 JDK」由 Stacker 直接装一个（走国内镜像）。</p></div>
      ) : versions.map((v) => (
        <div className={"vrow" + (v.current ? " cur" : "")} key={v.path}>
          <span className="ver">{v.version}</span>
          {v.arch && <span className="bd n" style={{ marginRight: 6 }}>{archLabel(v.arch)}</span>}
          <span className="meta">{v.vendor} · {v.path}</span>
          <div className="acts">
            {v.current && <span className="live"><i className="ti ti-circle-check" /> 生效中</span>}
            {v.current
              ? <button className="gh xs" title="重新写入 JAVA_HOME / PATH 并刷新（命令被 PATH 抢占时可修复）" onClick={() => { setScope("user"); setDlg(v); }}><i className="ti ti-refresh" /> 重新应用</button>
              : <button className="pr sm" onClick={() => { setScope("user"); setDlg(v); }}>设为默认</button>}
          </div>
        </div>
      ))}

      <div className="callout"><i className="ti ti-info-circle" /><div><b>JDK 没有"包源"。</b> Java 依赖走 Maven/Gradle，换源在它们各自页里。本页只管 JDK 版本与 JAVA_HOME/PATH。</div></div>

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

      {downloadOpen && (
        <Modal wide title="下载 JDK" icon="ti-download" onClose={() => setDownloadOpen(false)}
          footer={<>
            <button className="gh sm" onClick={() => setDownloadOpen(false)}>取消</button>
            <button className="pr sm" disabled={!dlDest.trim()} onClick={downloadJdk}><i className="ti ti-download" /> 下载并安装</button>
          </>}>
          <div className="field"><label>发行版</label>
            <div className="seg" style={{ alignSelf: "flex-start", flexWrap: "wrap" }}>
              <button className={vendor === "temurin" ? "on" : ""} onClick={() => pickVendor("temurin")}>Temurin（Adoptium）</button>
              <button className={vendor === "zulu" ? "on" : ""} onClick={() => pickVendor("zulu")}>Zulu（Azul）</button>
              <button className={vendor === "dragonwell" ? "on" : ""} onClick={() => pickVendor("dragonwell")}>Dragonwell（阿里 · 标准版）</button></div></div>
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
            <div className="hint"><i className="ti ti-alert-circle" style={{ color: "#e4b450" }} /> 默认在工具目录的 <span className="code">jdk</span> 子目录下，可点「浏览」改到任意盘。下载后自动扫描纳入列表。</div></div>
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
            vendor === "dragonwell" ? "将从 Dragonwell 官方国内镜像获取最新 Windows x64 安装包。"
            : vendor === "zulu" ? "将从 Azul 官方服务获取最新 GA 版本；当前未配置国内镜像。"
            : "将从清华 TUNA Adoptium 镜像获取所选大版本的最新 GA 补丁版。"}</div></div>
        </Modal>
      )}
    </>
  );
}
