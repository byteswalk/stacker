import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { enable as autostartEnable, disable as autostartDisable, isEnabled as autostartIsEnabled } from "@tauri-apps/plugin-autostart";
import { ConfirmModal, Modal, useToast } from "../ui";
import { Select } from "../Select";
import { SourceManagerModal } from "../SourceManagerModal";
import { getTheme, setTheme, type Theme } from "../theme";

type AppSettings = { minimize_to_tray: boolean; theme: Theme; proxy_host: string; proxy_port: number };
type SourceSummary = {
  server_version: string | null;
  builtin_count: number;
  local_count: number;
  binary_count: number;
};
type UpdateInfo = {
  current: string;
  latest: string;
  has_update: boolean;
  release_url?: string | null;
  installer_url?: string | null;
  portable_url?: string | null;
  published_at?: string | null;
  notes: string[];
};
type MirrorsUpdateCheck = {
  url: string;
  local_version: string | null;
  remote_version: string;
  has_update: boolean;
  tools: number;
};

export default function Settings() {
  const toast = useToast();
  const [noBackend, setNoBackend] = useState(false);
  const [sourceOpen, setSourceOpen] = useState(false);
  const [sourceSummary, setSourceSummary] = useState<SourceSummary | null>(null);
  const [tray, setTray] = useState(false);
  const [autostart, setAutostart] = useState(false);
  const [theme, setThemeState] = useState<Theme>(getTheme());
  const [proxyHost, setProxyHost] = useState("127.0.0.1");
  const [proxyPort, setProxyPort] = useState("7890");
  const [proxySaving, setProxySaving] = useState(false);
  const [appUpdBusy, setAppUpdBusy] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [sourceUpdate, setSourceUpdate] = useState<MirrorsUpdateCheck | null>(null);
  const [sourceUpdBusy, setSourceUpdBusy] = useState(false);

  function refreshSourceSummary() {
    invoke<SourceSummary>("source_catalog_status")
      .then(setSourceSummary)
      .catch(() => setNoBackend(true));
  }

  useEffect(() => {
    refreshSourceSummary();
    invoke<MirrorsUpdateCheck>("mirrors_check_update", { url: null })
      .then((r) => { if (r.has_update) setSourceUpdate(r); })
      .catch(() => {});
    invoke<AppSettings>("settings_get").then((s) => {
      setTray(s.minimize_to_tray);
      setProxyHost(s.proxy_host || "127.0.0.1");
      setProxyPort(String(s.proxy_port || 7890));
    }).catch(() => setNoBackend(true));
    autostartIsEnabled().then(setAutostart).catch(() => {});
  }, []);

  async function toggleTray(v: boolean) {
    try {
      await invoke("settings_set_tray", { enabled: v });
      setTray(v);
      toast(v ? "关闭窗口将最小化到托盘" : "已关闭最小化到托盘", "ok");
    } catch (e) {
      toast("设置失败：" + e, "err");
    }
  }

  async function toggleAutostart(v: boolean) {
    try {
      if (v) await autostartEnable();
      else await autostartDisable();
      setAutostart(v);
      toast(v ? "已设为开机自启" : "已取消开机自启", "ok");
    } catch (e) {
      toast("设置失败：" + e, "err");
    }
  }

  function changeTheme(t: Theme) {
    setTheme(t);
    setThemeState(t);
    invoke("settings_set_theme", { theme: t }).catch(() => {});
    toast("外观已切换", "ok");
  }

  async function saveProxyAddr() {
    const host = proxyHost.trim();
    const port = Number(proxyPort);
    if (!host) { toast("请输入代理主机地址", "info"); return; }
    if (!Number.isInteger(port) || port <= 0 || port > 65535) { toast("请输入有效的代理端口", "info"); return; }
    setProxySaving(true);
    try {
      await invoke("settings_set_proxy_addr", { host, port });
      setProxyHost(host);
      setProxyPort(String(port));
      toast("全局代理地址已保存", "ok");
    } catch (e) {
      toast("保存代理地址失败：" + e, "err");
    } finally {
      setProxySaving(false);
    }
  }

  async function checkAppUpdate() {
    setAppUpdBusy(true);
    try {
      const u = await invoke<UpdateInfo>("app_check_update");
      if (u.has_update) setUpdateInfo(u);
      else toast(`已是最新（v${u.current}）`, "ok");
    } catch (e) {
      toast(String(e), "info");
    } finally {
      setAppUpdBusy(false);
    }
  }

  async function openUrl(url?: string | null) {
    if (!url) { toast("当前发布信息没有提供该下载链接", "info"); return; }
    try {
      await invoke("app_open_url", { url });
    } catch (e) {
      toast("打开链接失败：" + e, "err");
    }
  }

  async function applySourceUpdate() {
    if (!sourceUpdate) return;
    setSourceUpdBusy(true);
    try {
      const s = await invoke<{ local_version: string | null; tools: number }>("mirrors_update", { url: sourceUpdate.url });
      setSourceUpdate(null);
      refreshSourceSummary();
      toast(`公共源清单已更新到 v${s.local_version}（${s.tools} 个分组）`, "ok");
    } catch (e) {
      toast("更新公共源清单失败：" + e, "err");
    } finally {
      setSourceUpdBusy(false);
    }
  }

  return (
    <>
      <div className="grouphd">
        <span className="gt"><i className="ti ti-database-cog" /> 源管理 <span className="cnt">分类维护 · 测速 · 导入导出</span></span>
      </div>
      {noBackend && <div className="banner gray"><i className="ti ti-plug-x lead" /><div className="bt">读取设置需要在 Stacker 桌面应用内运行。</div></div>}
      <div className="srcrow">
        <span className="av st"><i className="ti ti-database-cog" /></span>
        <div className="mt">
          <div className="t">源目录</div>
          <div className="s dim" title="集中管理运行时下载源、包仓库源、大文件下载镜像和本地自定义源。具体应用到工具配置仍在各生态页面完成。">
            管理下载源、仓库源和大文件镜像；应用配置仍在各生态页面完成。
          </div>
          <div className="s mono">
            内置 {sourceSummary?.builtin_count ?? 0}
            {sourceSummary?.server_version ? ` / 清单 v${sourceSummary.server_version}` : ""} · 自定义 {sourceSummary?.local_count ?? 0} · 大文件 {sourceSummary?.binary_count ?? 0}
          </div>
        </div>
        <button className="pr sm" disabled={noBackend} onClick={() => setSourceOpen(true)}><i className="ti ti-layout-sidebar-right-expand" /> 打开源管理</button>
      </div>
      <div className="callout"><i className="ti ti-info-circle" /><div>服务器清单用于更新内置源，拉取后会以服务器清单为准全量替换；本地自定义源由当前电脑维护，不会被服务器清单覆盖。</div></div>

      <div className="grouphd" style={{ marginTop: 18 }}><span className="gt"><i className="ti ti-adjustments" /> 通用与外观</span></div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-world-bolt" /></span>
        <div className="mt">
          <div className="t">全局代理地址</div>
          <div className="s dim" title="终端代理、Maven 代理和 Gradle 代理都会使用此地址；保存后，在对应页面点击应用时写入配置。">终端代理和构建工具代理使用此地址。</div>
        </div>
        <input className="ip" value={proxyHost} disabled={proxySaving || noBackend}
          onChange={(e) => setProxyHost(e.target.value)} placeholder="127.0.0.1" style={{ width: 156 }} />
        <input className="ip sm" value={proxyPort} disabled={proxySaving || noBackend}
          onChange={(e) => setProxyPort(e.target.value.replace(/[^\d]/g, ""))} placeholder="7890" />
        <button className="pr sm" disabled={proxySaving || noBackend} onClick={saveProxyAddr}>
          <i className={"ti " + (proxySaving ? "ti-loader spin" : "ti-device-floppy")} /> {proxySaving ? "保存中…" : "保存"}
        </button>
      </div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-device-desktop" /></span>
        <div className="mt"><div className="t">最小化到托盘</div><div className="s dim" title="开启后，关闭窗口会隐藏到系统托盘；可从托盘菜单显示窗口、切换终端代理或退出应用。">关闭窗口时隐藏到系统托盘。</div></div>
        <label className="sw sm2"><input type="checkbox" disabled={noBackend} checked={tray} onChange={(e) => toggleTray(e.target.checked)} /><span className="tk" /></label>
      </div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-player-play" /></span>
        <div className="mt"><div className="t">开机自启</div><div className="s dim" title="登录 Windows 后自动启动 Stacker，仅对当前用户生效。">登录 Windows 后自动启动 Stacker。</div></div>
        <label className="sw sm2"><input type="checkbox" disabled={noBackend} checked={autostart} onChange={(e) => toggleAutostart(e.target.checked)} /><span className="tk" /></label>
      </div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-moon" /></span>
        <div className="mt"><div className="t">外观</div><div className="s dim" title="选择深色、浅色或跟随系统主题。">深色 / 浅色 / 跟随系统</div></div>
        <Select value={theme} width={130} onChange={(v) => changeTheme(v as Theme)}
          options={[{ value: "dark", label: "深色" }, { value: "light", label: "浅色" }, { value: "system", label: "跟随系统" }]} />
      </div>

      <div className="grouphd" style={{ marginTop: 18 }}><span className="gt"><i className="ti ti-info-circle" /> 关于</span></div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-hexagon-letter-s" /></span>
        <div className="mt"><div className="t">Stacker 0.1.1 <span className="bd n">开源 · 无遥测</span></div>
          <div className="s dim" title="AI Coding Runtime Manager for Windows · github.com">AI Coding Runtime Manager for Windows · github.com</div></div>
        <button className="gh sm" disabled={appUpdBusy} onClick={checkAppUpdate}>
          <i className={"ti " + (appUpdBusy ? "ti-loader spin" : "ti-refresh")} /> {appUpdBusy ? "检查中…" : "检查更新"}</button>
      </div>

      {sourceOpen && <SourceManagerModal onClose={() => setSourceOpen(false)} onChanged={refreshSourceSummary} />}
      {sourceUpdate && !sourceOpen && (
        <ConfirmModal title="更新公共源清单" icon="ti-cloud-download" busy={sourceUpdBusy}
          message={<>发现新版公共源清单：<b style={{ color: "var(--tx)" }}>v{sourceUpdate.remote_version}</b>{sourceUpdate.local_version ? <>（当前 v{sourceUpdate.local_version}）</> : <>（本机尚未同步）</>}。<br />更新后会全量替换内置源，本地自定义源不会被覆盖。</>}
          confirmLabel="更新" onConfirm={applySourceUpdate} onClose={() => setSourceUpdate(null)} />
      )}
      {updateInfo && (
        <Modal title="发现新版本" icon="ti-cloud-download" onClose={() => setUpdateInfo(null)}
          footer={<>
            {updateInfo.installer_url && <button className="pr sm" onClick={() => openUrl(updateInfo.installer_url)}><i className="ti ti-download" /> 下载安装版</button>}
            {updateInfo.portable_url && <button className="gh sm" onClick={() => openUrl(updateInfo.portable_url)}><i className="ti ti-file-zip" /> 下载免安装版</button>}
            <button className="gh sm" onClick={() => openUrl(updateInfo.release_url)}><i className="ti ti-external-link" /> 查看发布页</button>
            <button className="gh sm" onClick={() => setUpdateInfo(null)}>关闭</button>
          </>}>
          <div className="banner blue" style={{ margin: 0 }}>
            <i className="ti ti-info-circle lead" />
            <div className="bt">当前版本 v{updateInfo.current}，最新版本 v{updateInfo.latest}。下载安装包后按提示完成升级。</div>
          </div>
          {updateInfo.notes.length > 0 && (
            <div className="field">
              <label>更新说明</label>
              <div className="histdiff">
                {updateInfo.notes.map((line, idx) => <div key={idx}>- {line}</div>)}
              </div>
            </div>
          )}
        </Modal>
      )}
    </>
  );
}
