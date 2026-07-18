import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { enable as autostartEnable, disable as autostartDisable, isEnabled as autostartIsEnabled } from "@tauri-apps/plugin-autostart";
import { ConfirmModal, Modal, useBusy, useToast } from "../ui";
import { Select } from "../Select";
import { SourceManagerModal } from "../SourceManagerModal";
import { getTheme, setTheme, type Theme } from "../theme";
import { formatBytes, useNotifications } from "../notifications";
import { useI18n, type Locale } from "../i18n";
import { disableRememberScanTargets } from "../features/space-analysis/targetStore";

type AppSettings = {
  minimize_to_tray: boolean;
  theme: Theme;
  proxy_host: string;
  proxy_port: number;
  log_level: "error" | "warn" | "info" | "debug";
  log_retention_days: number;
  large_file_threshold_bytes: number;
  remember_scan_targets: boolean;
};
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
type LogCleanupResult = { deletedFiles: number; releasedBytes: number; failedFiles: number };
type SpaceAnalysisSaveResult = "ok" | "settings-error" | "storage-error";

const GIB_BYTES = 1024 ** 3;
const MAX_LARGE_FILE_THRESHOLD_GB = 1024;

function normalizeLargeFileThresholdGb(value: string): number {
  return Math.min(MAX_LARGE_FILE_THRESHOLD_GB, Math.max(1, Math.round(Number(value) || 1)));
}

export default function Settings() {
  const { locale, setLocale } = useI18n();
  const toast = useToast();
  const busy = useBusy();
  const notices = useNotifications();
  const checkNotifications = notices.checkNow;
  const [noBackend, setNoBackend] = useState(false);
  const [sourceOpen, setSourceOpen] = useState(false);
  const [sourceSummary, setSourceSummary] = useState<SourceSummary | null>(null);
  const [tray, setTray] = useState(false);
  const [autostart, setAutostart] = useState(false);
  const [theme, setThemeState] = useState<Theme>(getTheme());
  const [logLevel, setLogLevel] = useState<AppSettings["log_level"]>("error");
  const [logRetentionDays, setLogRetentionDays] = useState("7");
  const [logRetentionSaving, setLogRetentionSaving] = useState(false);
  const [largeFileThresholdGb, setLargeFileThresholdGb] = useState("1");
  const [rememberScanTargets, setRememberScanTargets] = useState(true);
  const [spaceAnalysisSaving, setSpaceAnalysisSaving] = useState(false);
  const [clearLogConfirm, setClearLogConfirm] = useState(false);
  const [clearLogBusy, setClearLogBusy] = useState(false);
  const [proxyHost, setProxyHost] = useState("127.0.0.1");
  const [proxyPort, setProxyPort] = useState("7890");
  const [proxySaving, setProxySaving] = useState(false);
  const [appUpdBusy, setAppUpdBusy] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [sourceUpdate, setSourceUpdate] = useState<MirrorsUpdateCheck | null>(null);
  const [sourceUpdBusy, setSourceUpdBusy] = useState(false);
  const activeSourceUpdate = sourceUpdate ?? notices.sourceUpdate;
  const activeAppUpdate = updateInfo ?? notices.appUpdate;

  const refreshSourceSummary = useCallback(() => {
    invoke<SourceSummary>("source_catalog_status")
      .then(setSourceSummary)
      .catch(() => setNoBackend(true));
  }, []);

  useEffect(() => {
    refreshSourceSummary();
    invoke<AppSettings>("settings_get").then((s) => {
      setTray(s.minimize_to_tray);
      setProxyHost(s.proxy_host || "127.0.0.1");
      setProxyPort(String(s.proxy_port || 7890));
      setLogLevel(s.log_level || "error");
      setLogRetentionDays(String(s.log_retention_days || 7));
      setLargeFileThresholdGb(String(Math.round(s.large_file_threshold_bytes / GIB_BYTES) || 1));
      setRememberScanTargets(s.remember_scan_targets ?? true);
    }).catch(() => setNoBackend(true));
    autostartIsEnabled().then(setAutostart).catch(() => {});
    checkNotifications("settings").catch(() => {});
  }, [checkNotifications, refreshSourceSummary]);

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

  function changeLanguage(value: string) {
    const next = value as Locale;
    setLocale(next);
    toast(next === "zh-CN" ? "界面语言已切换为简体中文" : "Display language changed to English", "ok");
  }

  async function changeLogLevel(level: string) {
    try {
      const saved = await invoke<AppSettings["log_level"]>("settings_set_log_level", { level });
      setLogLevel(saved);
      toast(`日志级别已实时切换为 ${saved.toUpperCase()}`, "ok");
    } catch (e) {
      toast("日志级别设置失败：" + e, "err");
    }
  }

  async function openLogsDir() {
    try {
      await invoke("settings_open_logs_dir");
    } catch (e) {
      toast("打开日志目录失败：" + e, "err");
    }
  }

  async function openLogWindow() {
    try {
      await invoke("settings_open_log_window");
    } catch (e) {
      toast("打开实时日志失败：" + e, "err");
    }
  }

  async function saveLogRetention() {
    const days = Math.min(365, Math.max(1, Number(logRetentionDays) || 7));
    setLogRetentionSaving(true);
    try {
      const saved = await invoke<number>("settings_set_log_retention_days", { days });
      setLogRetentionDays(String(saved));
      toast(`日志保留时间已设置为 ${saved} 天`, "ok");
    } catch (e) {
      toast("保存日志保留时间失败：" + e, "err");
    } finally {
      setLogRetentionSaving(false);
    }
  }

  async function clearOldLogs() {
    setClearLogBusy(true);
    try {
      const result = await invoke<LogCleanupResult>("settings_clear_old_logs");
      setClearLogConfirm(false);
      const suffix = result.failedFiles > 0 ? `，${result.failedFiles} 个文件未能删除` : "";
      toast(`已清理 ${result.deletedFiles} 个历史日志，释放 ${formatBytes(result.releasedBytes)}${suffix}`, result.failedFiles > 0 ? "info" : "ok");
    } catch (e) {
      toast("清理日志失败：" + e, "err");
    } finally {
      setClearLogBusy(false);
    }
  }

  async function saveSpaceAnalysis(
    nextRememberScanTargets = rememberScanTargets,
  ): Promise<SpaceAnalysisSaveResult> {
    const thresholdGb = normalizeLargeFileThresholdGb(largeFileThresholdGb);
    setLargeFileThresholdGb(String(thresholdGb));
    setSpaceAnalysisSaving(true);
    const persistSettings = async () => {
      const saved = await invoke<AppSettings>("settings_set_space_analysis", {
        largeFileThresholdBytes: thresholdGb * GIB_BYTES,
        rememberScanTargets: nextRememberScanTargets,
      });
      setLargeFileThresholdGb(String(Math.round(saved.large_file_threshold_bytes / GIB_BYTES)));
      setRememberScanTargets(saved.remember_scan_targets);
    };
    try {
      if (!nextRememberScanTargets) {
        const result = await disableRememberScanTargets(persistSettings);
        if (!result.ok) {
          if (result.stage === "settings") {
            toast("保存空间分析设置失败：" + result.error, "err");
            return "settings-error";
          }
          toast("设置已保存，但无法清除记住的扫描目标。请检查系统存储权限后重试。", "err");
          return "storage-error";
        }
        toast("已关闭并清除记住的扫描目标", "ok");
        return "ok";
      }

      await persistSettings();
      toast("空间分析设置已保存", "ok");
      return "ok";
    } catch (e) {
      toast("保存空间分析设置失败：" + e, "err");
      return "settings-error";
    } finally {
      setSpaceAnalysisSaving(false);
    }
  }

  async function toggleRememberScanTargets(enabled: boolean) {
    const previous = rememberScanTargets;
    setRememberScanTargets(enabled);
    const result = await saveSpaceAnalysis(enabled);
    if (result === "settings-error") setRememberScanTargets(previous);
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
      toast("检查 Stacker 更新失败。请确认网络连接后重试。原因：" + e, "err");
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

  async function installAppUpdate() {
    if (!updateInfo?.installer_url) {
      toast("当前版本未提供 Windows 安装包", "info");
      return;
    }
    setAppUpdBusy(true);
    try {
      await busy({
        title: `更新 Stacker 至 v${updateInfo.latest}`,
        message: "正在下载并校验安装包。完成后 Stacker 将退出，由安装程序继续升级。",
        progressEvent: "app-update-progress",
      }, () => invoke("app_download_update", {
        url: updateInfo.installer_url,
        version: updateInfo.latest,
      }));
    } catch (e) {
      toast("更新失败：" + e, "err");
    } finally {
      setAppUpdBusy(false);
    }
  }

  async function applySourceUpdate() {
    const target = activeSourceUpdate;
    if (!target) return;
    setSourceUpdBusy(true);
    try {
      const s = await invoke<{ local_version: string | null; tools: number }>("mirrors_update", { url: target.url });
      setSourceUpdate(null);
      refreshSourceSummary();
      notices.checkNow("source-updated").catch(() => {});
      toast(`公共源清单已更新到 v${s.local_version}（${s.tools} 个分组）`, "ok");
    } catch (e) {
      toast("更新公共源清单失败：" + e, "err");
    } finally {
      setSourceUpdBusy(false);
    }
  }

  function updatePrefs(patch: Partial<typeof notices.prefs>) {
    notices.setPrefs({ ...notices.prefs, ...patch });
  }

  return (
    <>
      <div className="grouphd">
        <span className="gt">
          <i className="ti ti-database-cog" /> 源管理
          {activeSourceUpdate && <span className="bd r">有新版清单</span>}
          <span className="cnt">分类维护 · 测速 · 导入导出</span>
        </span>
      </div>
      {noBackend && <div className="banner gray"><i className="ti ti-plug-x lead" /><div className="bt">部分设置暂时无法读取。请重启 Stacker 后重试。</div></div>}
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
        {activeSourceUpdate && <button className="pr sm" disabled={sourceUpdBusy} onClick={applySourceUpdate}>
          <i className={"ti " + (sourceUpdBusy ? "ti-loader spin" : "ti-cloud-download")} /> 更新清单
        </button>}
        <button className="pr sm" disabled={noBackend} onClick={() => setSourceOpen(true)}><i className="ti ti-layout-sidebar-right-expand" /> 打开源管理</button>
      </div>
      {activeSourceUpdate && (
        <div className="callout">
          <i className="ti ti-cloud-download" />
          <div>发现新版公共源清单 v{activeSourceUpdate.remote_version}{activeSourceUpdate.local_version ? `（当前 v${activeSourceUpdate.local_version}）` : "（本机尚未同步）"}。更新后会全量替换内置源，本地自定义源不会被覆盖。</div>
        </div>
      )}
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
      <div className="srcrow">
        <span className="av st"><i className="ti ti-language" /></span>
        <div className="mt">
          <div className="t">界面语言</div>
          <div className="s dim" title="切换后立即应用到菜单、页面、弹窗和操作提示。">中文 / English</div>
        </div>
        <Select value={locale} width={150} onChange={changeLanguage}
          options={[{ value: "zh-CN", label: "简体中文" }, { value: "en-US", label: "English" }]} />
      </div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-file-description" /></span>
        <div className="mt">
          <div className="t">日志级别</div>
          <div className="s dim" title="日志按日期写入本机 Stacker 日志目录。切换后立即对当前进程生效；DEBUG 会记录更多诊断信息，问题排查结束后建议切回 ERROR。">
            级别切换立即生效；DEBUG 用于问题排查，日志按天归档。
          </div>
        </div>
        <Select value={logLevel} width={130} onChange={changeLogLevel}
          options={[
            { value: "error", label: "ERROR" },
            { value: "warn", label: "WARN" },
            { value: "info", label: "INFO" },
            { value: "debug", label: "DEBUG" },
          ]} />
        <button className="gh sm" onClick={openLogsDir} title="打开 Stacker 日志所在目录">
          <i className="ti ti-folder-open" /> 打开日志目录
        </button>
        <button className="gh sm" onClick={openLogWindow} title="在独立窗口查看当天日志；重复点击会聚焦已有窗口">
          <i className="ti ti-terminal-2" /> 实时日志
        </button>
      </div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-history-toggle" /></span>
        <div className="mt">
          <div className="t">日志保留</div>
          <div className="s dim" title="Stacker 启动时以及修改保留天数后，会自动删除超过保留期限的日志文件。">
            自动清理超过保留期限的日志；清理日志会保留今天的记录。
          </div>
        </div>
        <input className="ip sm" value={logRetentionDays} disabled={logRetentionSaving}
          onChange={(e) => setLogRetentionDays(e.target.value.replace(/[^\d]/g, ""))}
          onBlur={() => { if (!logRetentionDays) setLogRetentionDays("7"); }} />
        <span className="s dim">天</span>
        <button className="pr sm" disabled={logRetentionSaving} onClick={saveLogRetention}>
          <i className={"ti " + (logRetentionSaving ? "ti-loader spin" : "ti-device-floppy")} /> 保存
        </button>
        <button className="gh sm" onClick={() => setClearLogConfirm(true)}>
          <i className="ti ti-eraser" /> 清理日志
        </button>
      </div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-chart-treemap" /></span>
        <div className="mt">
          <div className="t">空间分析</div>
          <div className="s dim" title="大文件列表默认显示达到此阈值的文件；扫描目标只会在手动开始扫描后保存。">
            设置大文件阈值；记住的目标仅用于下次选择，不会自动扫描。
          </div>
        </div>
        <span className="s dim">大文件阈值</span>
        <input className="ip sm" type="number" min="1" max={MAX_LARGE_FILE_THRESHOLD_GB} step="1"
          aria-label="大文件阈值（GB）" value={largeFileThresholdGb} disabled={spaceAnalysisSaving || noBackend}
          onChange={(e) => setLargeFileThresholdGb(e.target.value)}
          onBlur={() => setLargeFileThresholdGb(String(normalizeLargeFileThresholdGb(largeFileThresholdGb)))} />
        <span className="s dim">GB</span>
        <button className="pr sm" disabled={spaceAnalysisSaving || noBackend} onClick={() => saveSpaceAnalysis()}>
          <i className={"ti " + (spaceAnalysisSaving ? "ti-loader spin" : "ti-device-floppy")} /> {spaceAnalysisSaving ? "保存中…" : "保存"}
        </button>
        <span className="s dim">记住上次扫描目标</span>
        <label className="sw sm2">
          <input type="checkbox" disabled={spaceAnalysisSaving || noBackend} checked={rememberScanTargets}
            aria-label="记住上次扫描目标" onChange={(e) => toggleRememberScanTargets(e.target.checked)} />
          <span className="tk" />
        </label>
      </div>

      <div className="grouphd" style={{ marginTop: 18 }}><span className="gt"><i className="ti ti-bell-cog" /> 提示管理</span></div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-bell" /></span>
        <div className="mt">
          <div className="t">后台检查提示</div>
          <div className="s dim">启动后和固定周期检查程序更新、源清单、生态版本、失效环境和清理阈值；仅显示红点，不自动弹窗。</div>
        </div>
        <span className="s dim">{notices.checking ? "检查中…" : notices.lastChecked ? `上次检查 ${new Date(notices.lastChecked).toLocaleTimeString()}` : "尚未检查"}</span>
        <button className="gh sm" disabled={notices.checking} onClick={() => notices.checkNow("manual")}>
          <i className={"ti " + (notices.checking ? "ti-loader spin" : "ti-refresh")} /> 立即检查
        </button>
        <label className="sw sm2"><input type="checkbox" checked={notices.prefs.enabled} onChange={(e) => updatePrefs({ enabled: e.target.checked })} /><span className="tk" /></label>
      </div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-clock" /></span>
        <div className="mt"><div className="t">检查周期</div><div className="s dim">默认 30 分钟；周期越短，网络请求越频繁。</div></div>
        <Select value={String(notices.prefs.intervalMinutes)} width={130} onChange={(v) => updatePrefs({ intervalMinutes: Number(v) })}
          options={[15, 30, 60, 120].map((m) => ({ value: String(m), label: `${m} 分钟` }))} />
      </div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-list-check" /></span>
        <div className="mt"><div className="t">检查项目</div><div className="s dim">可按使用习惯关闭程序更新、源清单、生态版本和失效环境提示。</div></div>
        <label className="ck"><input type="checkbox" checked={notices.prefs.appUpdate} onChange={(e) => updatePrefs({ appUpdate: e.target.checked })} /> 程序更新</label>
        <label className="ck"><input type="checkbox" checked={notices.prefs.sourceUpdate} onChange={(e) => updatePrefs({ sourceUpdate: e.target.checked })} /> 源清单</label>
        <label className="ck"><input type="checkbox" checked={notices.prefs.ecosystemUpdate} onChange={(e) => updatePrefs({ ecosystemUpdate: e.target.checked })} /> 生态版本</label>
        <label className="ck"><input type="checkbox" checked={notices.prefs.environmentIssue} onChange={(e) => updatePrefs({ environmentIssue: e.target.checked })} /> 失效环境</label>
      </div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-eraser" /></span>
        <div className="mt">
          <div className="t">磁盘清理提醒</div>
          <div className="s dim">可安全清理项超过阈值时，在「磁盘清理」菜单显示红点。</div>
          {notices.cleanupBytes > 0 && <div className="s mono">当前后台估算：{formatBytes(notices.cleanupBytes)}</div>}
        </div>
        <input className="ip sm" value={String(notices.prefs.cleanupThresholdGb)}
          onChange={(e) => updatePrefs({ cleanupThresholdGb: Number(e.target.value.replace(/[^\d]/g, "")) || 1 })} />
        <span className="s dim">GB</span>
        <label className="sw sm2"><input type="checkbox" checked={notices.prefs.cleanup} onChange={(e) => updatePrefs({ cleanup: e.target.checked })} /><span className="tk" /></label>
      </div>
      <div className="grouphd" style={{ marginTop: 18 }}><span className="gt"><i className="ti ti-info-circle" /> 关于</span></div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-hexagon-letter-s" /></span>
        <div className="mt"><div className="t">Stacker 0.2.0 <span className="bd n">开源 · 无遥测</span>{activeAppUpdate && <span className="bd r">发现 v{activeAppUpdate.latest}</span>}</div>
          <div className="s dim" title="Windows 开发环境与工作智能体工具管理器 · https://github.com/byteswalk/stacker">Windows 开发环境与工作智能体工具管理器 · github.com/byteswalk/stacker</div></div>
        <button className="gh sm" onClick={() => openUrl("https://github.com/byteswalk/stacker")}>
          <i className="ti ti-brand-github" /> GitHub</button>
        <button className="gh sm" disabled={appUpdBusy} onClick={checkAppUpdate}>
          <i className={"ti " + (appUpdBusy ? "ti-loader spin" : "ti-refresh")} /> {appUpdBusy ? "检查中…" : "检查更新"}</button>
      </div>

      {clearLogConfirm && <ConfirmModal title="清理历史日志" icon="ti-eraser" danger busy={clearLogBusy}
        message="将删除日志目录内除今天之外的全部日志文件。今天正在使用的日志不会受到影响。"
        confirmLabel="清理日志" onConfirm={clearOldLogs} onClose={() => setClearLogConfirm(false)} />}
      {sourceOpen && <SourceManagerModal onClose={() => setSourceOpen(false)} onChanged={() => { refreshSourceSummary(); notices.checkNow("source-changed").catch(() => {}); }} />}
      {updateInfo && (
        <Modal title="发现新版本" icon="ti-cloud-download" onClose={() => setUpdateInfo(null)}
          footer={<>
            {updateInfo.installer_url && <button className="pr sm" disabled={appUpdBusy} onClick={installAppUpdate}><i className="ti ti-download" /> 立即更新</button>}
            {updateInfo.portable_url && <button className="gh sm" onClick={() => openUrl(updateInfo.portable_url)}><i className="ti ti-file-zip" /> 下载免安装版</button>}
            <button className="gh sm" onClick={() => openUrl(updateInfo.release_url)}><i className="ti ti-external-link" /> 查看发布页</button>
            <button className="gh sm" onClick={() => setUpdateInfo(null)}>关闭</button>
          </>}>
          <div className="banner blue" style={{ margin: 0 }}>
            <i className="ti ti-info-circle lead" />
            <div className="bt">当前版本 v{updateInfo.current}，最新版本 v{updateInfo.latest}。Stacker 将在应用内下载安装包并启动升级。</div>
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
