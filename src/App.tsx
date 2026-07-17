import { useEffect, useRef, useState } from "react";
import { invoke } from "./invoke";
import { collectFrontendSettings, restoreFrontendSettings, type FrontendSettings } from "./frontendSettings";
import { open, save } from "@tauri-apps/plugin-dialog";
import { ToastProvider, ToastHost, useToast, Modal, ConfirmModal, BusyProvider, BusyHost } from "./ui";
import { Select } from "./Select";
import Overview from "./pages/Overview";
import Vibe from "./pages/Vibe";
import Git from "./pages/Git";
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
import { useI18n, type MessageKey } from "./i18n";
import { NotificationProvider, useNotifications, formatBytes } from "./notifications";

export type Page =
  | "overview" | "vibe" | "git" | "python" | "node" | "java" | "maven" | "gradle" | "go" | "rust"
  | "proxy" | "cleanup" | "history" | "settings";

type NavItem = { id: Page; icon: string; labelKey: MessageKey };

// 主导航：概览 + 8 生态 ─（分隔）─ 终端代理 / 磁盘清理
const NAV_TOP: NavItem[] = [
  { id: "overview", icon: "ti-layout-dashboard", labelKey: "nav.overview" },
  { id: "vibe", icon: "ti-sparkles", labelKey: "nav.vibe" },
  { id: "git", icon: "ti-brand-git", labelKey: "nav.git" },
  { id: "python", icon: "ti-brand-python", labelKey: "nav.python" },
  { id: "node", icon: "ti-brand-nodejs", labelKey: "nav.node" },
  { id: "java", icon: "ti-coffee", labelKey: "nav.java" },
  { id: "maven", icon: "ti-feather", labelKey: "nav.maven" },
  { id: "gradle", icon: "ti-box", labelKey: "nav.gradle" },
  { id: "go", icon: "ti-brand-golang", labelKey: "nav.go" },
  { id: "rust", icon: "ti-brand-rust", labelKey: "nav.rust" },
];
const NAV_TOOLS: NavItem[] = [
  { id: "proxy", icon: "ti-world-bolt", labelKey: "nav.proxy" },
  { id: "cleanup", icon: "ti-eraser", labelKey: "nav.cleanup" },
];
const NAV_FOOT: NavItem[] = [
  { id: "history", icon: "ti-history", labelKey: "nav.history" },
  { id: "settings", icon: "ti-settings", labelKey: "nav.settings" },
];
const ALL = [...NAV_TOP, ...NAV_TOOLS, ...NAV_FOOT];

function NavBtn({ item, page, set }: { item: NavItem; page: Page; set: (p: Page) => void }) {
  const { t } = useI18n();
  const notices = useNotifications();
  const noticeCount = item.id === "settings"
    ? notices.settingsCount
    : item.id === "cleanup"
      ? notices.cleanupCount
      : notices.pageNoticeCounts[item.id] ?? 0;
  const noticeTitle = noticeTip(item.id, notices);
  return (
    <button className={"ni" + (page === item.id ? " on" : "")} onClick={() => set(item.id)}>
      <i className={"ti " + item.icon} /> {t(item.labelKey)}
      {noticeCount > 0 && <span className="navdot" title={noticeTitle}>{noticeCount > 9 ? "9+" : noticeCount}</span>}
    </button>
  );
}

function noticeTip(page: Page, notices: ReturnType<typeof useNotifications>) {
  const rows: string[] = [];
  if (page === "settings") {
    if (notices.appUpdate) rows.push(`程序更新：${notices.appUpdate.current} -> ${notices.appUpdate.latest}`);
    if (notices.sourceUpdate) rows.push(`源清单更新：${notices.sourceUpdate.local_version ?? "未同步"} -> ${notices.sourceUpdate.remote_version}`);
  } else if (page === "cleanup") {
    if (notices.cleanupCount > 0) rows.push(`磁盘清理：可安全清理约 ${formatBytes(notices.cleanupBytes)}`);
  } else {
    notices.ecosystemUpdates.filter((item) => item.id === page)
      .forEach((item) => rows.push(`${item.name} 有新版本：${item.current} -> ${item.latest}`));
    notices.aiToolUpdates.filter((item) => item.page === page)
      .forEach((item) => rows.push(`${item.name} 有新版本：${item.current} -> ${item.latest}`));
    notices.environmentIssues.filter((item) => item.page === page)
      .forEach((item) => rows.push(`环境异常：${item.title}`));
  }
  return rows.length ? rows.join("\n") : "暂无待处理明细";
}

function Stub({ item }: { item: NavItem }) {
  const { t } = useI18n();
  return (
    <div className="stub">
      <div className="si"><i className={"ti " + item.icon} /></div>
      <h2>{t(item.labelKey)}</h2>
      <p>{t("state.comingSoonDesc")}</p>
    </div>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <BusyProvider>
        <NotificationProvider>
          <Shell />
        </NotificationProvider>
      </BusyProvider>
    </ToastProvider>
  );
}

type SavedProfile = {
  name: string;
  sources: { tool: string; mirror: string }[];
  proxy: boolean;
  created: string;
  frontend_settings?: FrontendSettings;
};

function Shell() {
  const { t } = useI18n();
  const toast = useToast();
  const notices = useNotifications();
  const [page, setPage] = useState<Page>("overview");
  const [profile, setProfile] = useState("");
  const [applying, setApplying] = useState(false);
  const [saved, setSaved] = useState<SavedProfile[]>([]);
  const [saveOpen, setSaveOpen] = useState(false);
  const [saveName, setSaveName] = useState("");
  const [saving, setSaving] = useState(false);
  const [configEpoch, setConfigEpoch] = useState(0);
  const [deleteProfile, setDeleteProfile] = useState<string | null>(null);
  const [osWarn, setOsWarn] = useState<{ name: string; build: number } | null>(null);
  const [osDismiss, setOsDismiss] = useState(false);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const cur = ALL.find((n) => n.id === page)!;
  const currentNoticeCount = page === "settings"
    ? notices.settingsCount
    : page === "cleanup"
      ? notices.cleanupCount
      : notices.pageNoticeCounts[page] ?? 0;
  const currentNoticeTitle = noticeTip(page, notices);

  function refreshProfiles() {
    invoke<SavedProfile[]>("profile_list")
      .then((list) => {
        setSaved(list);
        setProfile((cur) => list.some((p) => p.name === cur) ? cur : (list[0]?.name ?? ""));
      })
      .catch(() => {});
  }
  useEffect(() => {
    refreshProfiles();
    invoke<{ name: string; build: number; supported: boolean }>("os_info")
      .then((o) => { if (!o.supported) setOsWarn({ name: o.name, build: o.build }); }).catch(() => {});
  }, []);
  useEffect(() => {
    if (contentRef.current) contentRef.current.scrollTop = 0;
  }, [page]);

  async function applyProfile() {
    if (!profile) { toast("请先保存或导入配置方案", "info"); return; }
    setApplying(true);
    try {
      const result = await invoke<{ changed: number; frontend_settings: FrontendSettings }>("profile_apply", { name: profile });
      restoreFrontendSettings(result.frontend_settings);
      setConfigEpoch((value) => value + 1);
      toast(`已套用方案「${profile}」· 改动 ${result.changed} 项`, "ok");
    } catch (e) { toast("应用失败：" + e, "err"); } finally { setApplying(false); }
  }

  async function doSave() {
    const name = saveName.trim();
    if (!name) { toast("请输入方案名", "info"); return; }
    setSaving(true);
    try {
      await invoke<SavedProfile>("profile_save", { name, frontendSettings: collectFrontendSettings() });
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
      if (profile === name) setProfile("");
      setDeleteProfile(null);
      toast(`已删除方案「${name}」`, "ok");
    } catch (e) { toast("删除失败：" + e, "err"); }
  }

  async function exportProfiles() {
    try {
      const path = await save({ defaultPath: "stacker-config.json", filters: [{ name: "Stacker 配置", extensions: ["json"] }] });
      if (!path) return;
      await invoke("bundle_export", { path, frontendSettings: collectFrontendSettings() });
      toast("配置方案已导出；凭据不会写入导出文件", "ok");
    } catch (e) {
      toast("导出失败：" + e, "err");
    }
  }

  async function importProfiles() {
    try {
      const path = await open({ multiple: false, directory: false, filters: [{ name: "Stacker 配置", extensions: ["json"] }] });
      if (!path || typeof path !== "string") return;
      const r = await invoke<{ profiles: number; customs: number; frontend_settings: FrontendSettings }>("bundle_import", { path });
      restoreFrontendSettings(r.frontend_settings);
      setConfigEpoch((value) => value + 1);
      refreshProfiles();
      toast(`已导入：方案 ${r.profiles} 个 · 自定义源 ${r.customs} 个（密码需重填）`, "ok");
    } catch (e) {
      toast("导入失败：" + e, "err");
    }
  }

  return (
    <div className="a">
      <aside className="side">
        <div className="brand">
          <span className="logo" aria-hidden="true">
            <svg className="logo-mark" viewBox="0 0 32 32" focusable="false">
              <path className="logo-layer-1" d="M16 4 28 10 16 16 4 10Z" />
              <path className="logo-layer-2" d="M16 10 28 16 16 22 4 16Z" />
              <path className="logo-layer-3" d="M16 16 28 22 16 28 4 22Z" />
            </svg>
          </span>
          Stacker
        </div>
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
              {t(cur.labelKey)}
              {currentNoticeCount > 0 && <span className="navdot title-dot" title={currentNoticeTitle}>{currentNoticeCount > 9 ? "9+" : currentNoticeCount}</span>}
            </span>
          </div>
          {page === "overview" && (
            <div className="hdright">
              <div className="profile">
                <i className="ti ti-bookmark" style={{ fontSize: 14 }} />
                <Select className="psel" value={profile} disabled={applying} width={196} onChange={setProfile}
                  options={saved.length > 0 ? saved.map((p) => ({ value: p.name, label: p.name })) : [{ value: "", label: "暂无方案", disabled: true }]} />
                <button className="pbtn" title="应用方案" disabled={applying || !profile} onClick={applyProfile}><i className={"ti " + (applying ? "ti-loader" : "ti-check")} /></button>
                <button className="pbtn" title="保存当前源选择与代理状态" disabled={applying} onClick={() => { setSaveName(""); setSaveOpen(true); }}><i className="ti ti-device-floppy" /></button>
                <button className="pbtn" title="导入配置方案" disabled={applying} onClick={importProfiles}><i className="ti ti-download" /></button>
                <button className="pbtn" title="导出配置方案" disabled={applying} onClick={exportProfiles}><i className="ti ti-upload" /></button>
              </div>
            </div>
          )}
        </div>
        <div className="route-progress" aria-hidden="true" />

        <div className="content" ref={contentRef}>
          {osWarn && !osDismiss && (
            <div className="banner amber" style={{ marginBottom: 12, alignItems: "center" }}>
              <i className="ti ti-alert-triangle lead" />
              <div className="bt"><b>系统版本可能不受支持。</b> Stacker 需要 Windows 10 或 Windows 11；当前为 {osWarn.name}（build {osWarn.build}）。较低版本可能无法正常显示界面或运行部分功能。</div>
              <button className="gh xs" onClick={() => setOsDismiss(true)}>关闭提示</button>
            </div>
          )}
          {page === "overview" ? <Overview key={configEpoch} goto={setPage} />
            : page === "vibe" ? <Vibe key={configEpoch} />
            : page === "git" ? <Git key={configEpoch} />
            : page === "node" ? <Node key={configEpoch} />
            : page === "proxy" ? <Proxy key={configEpoch} />
            : page === "history" ? <History key={configEpoch} />
            : page === "java" ? <Java key={configEpoch} />
            : page === "python" ? <Python key={configEpoch} />
            : page === "maven" ? <Maven key={configEpoch} />
            : page === "gradle" ? <Gradle key={configEpoch} />
            : page === "rust" ? <Rust key={configEpoch} />
            : page === "go" ? <Go key={configEpoch} />
            : page === "cleanup" ? <Cleanup key={configEpoch} />
            : page === "settings" ? <Settings key={configEpoch} />
            : <Stub item={cur} />}
        </div>
      </div>

      {saveOpen && (
        <Modal title="保存为方案" icon="ti-device-floppy"
          sub="保存当前源选择与代理状态，便于在不同网络环境间快速切换。"
          onClose={() => !saving && setSaveOpen(false)}
          footer={<>
            <button className="gh sm" disabled={saving} onClick={() => setSaveOpen(false)}>取消</button>
            <button className="pr sm" disabled={saving || !saveName.trim()} onClick={doSave}>
              <i className={"ti " + (saving ? "ti-loader" : "ti-device-floppy")} /> {saving ? "保存中…" : "保存"}</button>
          </>}>
          <div className="field">
            <label>方案名</label>
            <input className="ip full" autoFocus value={saveName} placeholder="如：公司内网 / 公开网络"
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
                    <span style={{ fontSize: 11, color: "var(--mut)" }}>{p.proxy ? "代理已开启" : "代理未开启"} · {p.created}</span>
                    <button className="gh sm" disabled={saving} title="删除此方案" onClick={() => setDeleteProfile(p.name)}><i className="ti ti-trash" /></button>
                  </div>
                ))}
              </div>
            </div>
          )}
        </Modal>
      )}

      {deleteProfile && (
        <ConfirmModal title="删除配置方案" icon="ti-trash" danger
          message={<>将删除配置方案「{deleteProfile}」。已应用到各工具的配置不会被更改。</>}
          confirmLabel="确认删除" onConfirm={() => delProfile(deleteProfile)} onClose={() => setDeleteProfile(null)} />
      )}

      <ToastHost />
      <BusyHost />
    </div>
  );
}
