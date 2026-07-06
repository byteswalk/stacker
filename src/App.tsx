import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ToastProvider, ToastHost, useToast, Modal, BusyProvider, BusyHost } from "./ui";
import { Select } from "./Select";
import Overview from "./pages/Overview";
import Proxy from "./pages/Proxy";
import History from "./pages/History";
import Java from "./pages/Java";
import Python from "./pages/Python";
import Maven from "./pages/Maven";
import Gradle from "./pages/Gradle";
import Rust from "./pages/Rust";
import Go from "./pages/Go";
import Cleanup from "./pages/Cleanup";
import Node from "./pages/Node";
import Settings from "./pages/Settings";

export type Page =
  | "overview" | "python" | "node" | "java" | "maven" | "gradle" | "go" | "rust"
  | "proxy" | "cleanup" | "history" | "settings";

type NavItem = { id: Page; icon: string; label: string };

// 主导航：概览 + 8 生态 ─（分隔）─ 终端代理 / 磁盘清理
const NAV_TOP: NavItem[] = [
  { id: "overview", icon: "ti-layout-dashboard", label: "概览" },
  { id: "python", icon: "ti-brand-python", label: "Python" },
  { id: "node", icon: "ti-brand-nodejs", label: "Node" },
  { id: "java", icon: "ti-coffee", label: "Java" },
  { id: "maven", icon: "ti-feather", label: "Maven" },
  { id: "gradle", icon: "ti-box", label: "Gradle" },
  { id: "go", icon: "ti-brand-golang", label: "Go" },
  { id: "rust", icon: "ti-brand-rust", label: "Rust" },
];
const NAV_TOOLS: NavItem[] = [
  { id: "proxy", icon: "ti-world-bolt", label: "终端代理" },
  { id: "cleanup", icon: "ti-eraser", label: "磁盘清理" },
];
const NAV_FOOT: NavItem[] = [
  { id: "history", icon: "ti-history", label: "历史" },
  { id: "settings", icon: "ti-settings", label: "设置" },
];
const ALL = [...NAV_TOP, ...NAV_TOOLS, ...NAV_FOOT];

function NavBtn({ item, page, set }: { item: NavItem; page: Page; set: (p: Page) => void }) {
  return (
    <button className={"ni" + (page === item.id ? " on" : "")} onClick={() => set(item.id)}>
      <i className={"ti " + item.icon} /> {item.label}
    </button>
  );
}

function Stub({ item }: { item: NavItem }) {
  return (
    <div className="stub">
      <div className="si"><i className={"ti " + item.icon} /></div>
      <h2>{item.label}</h2>
      <p>此页正在按原型实现中。设计稿见 <code style={{ fontFamily: "var(--font-mono)", color: "var(--mut)" }}>design/proto/{item.id}.html</code>，后端 handler 接入后填充。</p>
    </div>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <BusyProvider>
        <Shell />
      </BusyProvider>
    </ToastProvider>
  );
}

type SavedProfile = { name: string; sources: { tool: string; mirror: string }[]; proxy: boolean; created: string };

const BUILTIN = ["国内源（全部国内镜像）", "官方源（全部官方）"];

function Shell() {
  const toast = useToast();
  const [page, setPage] = useState<Page>("overview");
  const [profile, setProfile] = useState("默认");
  const [applying, setApplying] = useState(false);
  const [saved, setSaved] = useState<SavedProfile[]>([]);
  const [saveOpen, setSaveOpen] = useState(false);
  const [saveName, setSaveName] = useState("");
  const [saving, setSaving] = useState(false);
  const [osWarn, setOsWarn] = useState<{ name: string; build: number } | null>(null);
  const [osDismiss, setOsDismiss] = useState(false);
  const cur = ALL.find((n) => n.id === page)!;

  function refreshProfiles() {
    invoke<SavedProfile[]>("profile_list").then(setSaved).catch(() => {});
  }
  useEffect(() => {
    refreshProfiles();
    invoke<{ name: string; build: number; supported: boolean }>("os_info")
      .then((o) => { if (!o.supported) setOsWarn({ name: o.name, build: o.build }); }).catch(() => {});
  }, []);

  // 内置模板：国内源 = 全切首个国内镜像 + 关代理；官方源 = 全切官方 + 开代理
  async function applyBuiltin(toMirror: boolean, label: string) {
    const tools = await invoke<{ id: string; installed: boolean; current: string | null; mirrors: { id: string }[] }[]>("list_sources");
    let n = 0;
    for (const t of tools) {
      if (t.id === "python-runtime") continue;
      if (!t.installed) continue;
      const target = toMirror ? t.mirrors.find((m) => m.id !== "official")?.id : "official";
      if (target && t.current !== target) { await invoke("apply_source", { toolId: t.id, mirrorId: target }); n++; }
    }
    const ps = await invoke<{ enabled: boolean; host: string; port: number; detected_port: number | null }>("proxy_status");
    if (toMirror && ps.enabled) await invoke("proxy_disable", { alsoJvm: false });
    if (!toMirror && !ps.enabled) await invoke("proxy_enable", { host: ps.host || "127.0.0.1", port: ps.port || ps.detected_port || 7890, alsoJvm: false, manual: [] });
    toast(`已应用「${label}」· 换源 ${n} 项`, "ok");
  }

  async function applyProfile() {
    if (profile === "默认") { toast("「默认」为占位模板，未改动配置", "info"); return; }
    setApplying(true);
    try {
      if (BUILTIN.includes(profile)) {
        await applyBuiltin(profile.startsWith("国内源"), profile.split("（")[0]);
      } else {
        const n = await invoke<number>("profile_apply", { name: profile });
        toast(`已套用方案「${profile}」· 改动 ${n} 项`, "ok");
      }
    } catch (e) { toast("应用失败：" + e, "err"); } finally { setApplying(false); }
  }

  async function doSave() {
    const name = saveName.trim();
    if (!name) { toast("请输入方案名", "info"); return; }
    setSaving(true);
    try {
      await invoke<SavedProfile>("profile_save", { name });
      refreshProfiles();
      setProfile(name);
      setSaveOpen(false);
      setSaveName("");
      toast(`已保存方案「${name}」`, "ok");
    } catch (e) { toast("保存失败：" + e, "err"); } finally { setSaving(false); }
  }

  async function delProfile(name: string) {
    try {
      await invoke("profile_delete", { name });
      refreshProfiles();
      if (profile === name) setProfile("默认");
      toast(`已删除方案「${name}」`, "ok");
    } catch (e) { toast("删除失败：" + e, "err"); }
  }

  return (
    <div className="a">
      <aside className="side">
        <div className="brand"><span className="logo"><i className="ti ti-hexagon-letter-s" /></span> Stacker</div>
        <nav>
          {NAV_TOP.map((n) => <NavBtn key={n.id} item={n} page={page} set={setPage} />)}
          <div className="navsep" />
          {NAV_TOOLS.map((n) => <NavBtn key={n.id} item={n} page={page} set={setPage} />)}
        </nav>
        <div className="sidefoot">
          {NAV_FOOT.map((n) => <NavBtn key={n.id} item={n} page={page} set={setPage} />)}
        </div>
      </aside>

      <div className="main">
        <div className="hd">
          <div className="htitle">
            <span className="ttl">
              {page !== "overview" && <span className="eco av st"><i className={"ti " + cur.icon} /></span>}
              {cur.label}
            </span>
          </div>
          {page === "overview" && (
            <div className="hdright">
              <div className="profile">
                <i className="ti ti-bookmark" style={{ fontSize: 14 }} />
                <Select className="psel" value={profile} disabled={applying} width={196} onChange={setProfile}
                  groups={[
                    { options: [{ value: "默认", label: "默认" }] },
                    { label: "内置模板", options: [
                      { value: "国内源（全部国内镜像）", label: "国内源（全部国内镜像）" },
                      { value: "官方源（全部官方）", label: "官方源（全部官方）" },
                    ] },
                    ...(saved.length > 0 ? [{ label: "我的方案", options: saved.map((p) => ({ value: p.name, label: p.name })) }] : []),
                  ]} />
                <button className="pbtn" title="应用方案" disabled={applying} onClick={applyProfile}><i className={"ti " + (applying ? "ti-loader" : "ti-check")} /></button>
                <button className="pbtn" title="把当前各工具的源 + 代理开关存为命名方案" disabled={applying} onClick={() => { setSaveName(""); setSaveOpen(true); }}><i className="ti ti-device-floppy" /></button>
              </div>
            </div>
          )}
        </div>

        <div className="content">
          {osWarn && !osDismiss && (
            <div className="banner amber" style={{ marginBottom: 12, alignItems: "center" }}>
              <i className="ti ti-alert-triangle lead" />
              <div className="bt"><b>系统版本可能不受支持</b> —— Stacker 需要 Windows 10 / 11；当前 {osWarn.name}（build {osWarn.build}）。低版本下界面（WebView2）与部分功能可能无法正常工作。</div>
              <button className="gh xs" onClick={() => setOsDismiss(true)}>知道了</button>
            </div>
          )}
          {page === "overview" ? <Overview goto={setPage} />
            : page === "node" ? <Node />
            : page === "proxy" ? <Proxy />
            : page === "history" ? <History />
            : page === "java" ? <Java />
            : page === "python" ? <Python />
            : page === "maven" ? <Maven />
            : page === "gradle" ? <Gradle />
            : page === "rust" ? <Rust />
            : page === "go" ? <Go />
            : page === "cleanup" ? <Cleanup />
            : page === "settings" ? <Settings />
            : <Stub item={cur} />}
        </div>
      </div>

      {saveOpen && (
        <Modal title="保存为方案" icon="ti-device-floppy"
          sub="抓取当前各工具的源选择与代理开关，存成命名方案，之后可一键套用。"
          onClose={() => !saving && setSaveOpen(false)}
          footer={<>
            <button className="gh sm" disabled={saving} onClick={() => setSaveOpen(false)}>取消</button>
            <button className="pr sm" disabled={saving || !saveName.trim()} onClick={doSave}>
              <i className={"ti " + (saving ? "ti-loader" : "ti-device-floppy")} /> {saving ? "保存中…" : "保存"}</button>
          </>}>
          <div className="field">
            <label>方案名</label>
            <input className="ip full" autoFocus value={saveName} placeholder="如：公司内网 / 出差官方源"
              onChange={(e) => setSaveName(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") doSave(); }} />
            {saved.some((p) => p.name === saveName.trim()) && saveName.trim() &&
              <div style={{ fontSize: 12, color: "var(--amber)", marginTop: 6 }}>已存在同名方案，保存将覆盖。</div>}
          </div>
          {saved.length > 0 && (
            <div className="field">
              <label>已有方案</label>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {saved.map((p) => (
                  <div key={p.name} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13 }}>
                    <i className="ti ti-bookmark" style={{ color: "var(--mut)" }} />
                    <span style={{ flex: 1 }}>{p.name}</span>
                    <span style={{ fontSize: 11, color: "var(--mut)" }}>{p.proxy ? "代理开" : "代理关"} · {p.created}</span>
                    <button className="gh sm" disabled={saving} title="删除此方案" onClick={() => delProfile(p.name)}><i className="ti ti-trash" /></button>
                  </div>
                ))}
              </div>
            </div>
          )}
        </Modal>
      )}

      <ToastHost />
      <BusyHost />
    </div>
  );
}
