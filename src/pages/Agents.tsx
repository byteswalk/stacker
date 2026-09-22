import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "../invoke";
import { ConfirmModal, Modal, useBusyRead, useToast } from "../ui";
import { useNotifications } from "../notifications";
import { translateText, useI18n } from "../i18n";
import { Select } from "../Select";

import {
  loadCatalog,
  refreshOneTool,
  runVibeCheck,
  subscribeVibe,
  surfaceDetected,
  vibeSnapshot,
  type VibeSurface,
  type VibeTool,
} from "../features/agents/catalogStore";

import { UpdatePlanModal } from "../features/agents/UpdatePlanModal";
import { tileLine } from "../features/agents/tileState";
import { surfaceTaskText } from "../features/agents/surfaceTask";
import {
  cancelAgentTask,
  openTaskFor,
  startAgentTask,
  subscribeTasks,
  taskSnapshot,
  type AgentTaskAction,
} from "../features/agent-tasks/taskStore";

export type { VibeSurface, VibeTool } from "../features/agents/catalogStore";

type AgentView = "tiles" | "cards";
const VIEW_KEY = "stacker.agents.view";

/** Thumbnails by default: nineteen full cards do not fit a screen. Remembered per viewer. */
function storedView(): AgentView {
  try { return localStorage.getItem(VIEW_KEY) === "cards" ? "cards" : "tiles"; } catch { return "tiles"; }
}

export function UnavailableSurface({ target, surface }: { target: "cli" | "desktop"; surface: VibeSurface }) {
  return (
    <div className="vtool-surface unavailable">
      <span className={"surface-kind " + target}>{target === "cli" ? "CLI" : "桌面端"}</span>
      <div className="surface-main">
        <div className="surface-title">
          <i className="ti ti-circle-off" aria-hidden="true" />
          <span>{surface.label || (target === "cli" ? "CLI" : "桌面端")}</span>
          <span className="bd n">暂不可用</span>
        </div>
        <div className="surface-desc">{surface.description || "Stacker 暂未接入此使用方式。"}</div>
        <div className="surface-meta">支持情况请参阅该智能体的官方文档。</div>
      </div>
    </div>
  );
}


function surfaceBadge(surface: VibeSurface) {
  if (surface.status === "pending") return <span className="bd n">待检测</span>;
  if (surface.status === "update") return <span className="bd w">可更新</span>;
  if (surface.status === "installed") return <span className="bd g">已安装</span>;
  if (surface.status === "unknown") return <span className="bd b">已检测</span>;
  return <span className="bd n">未安装</span>;
}

function installMethodHint(surface: VibeSurface) {
  switch (surface.install_method) {
    case "winget":
      return "安装来源：WinGet 包管理器。更新和卸载会优先使用 WinGet。";
    case "npm":
    case "conda-npm":
      return "安装来源：npm 全局包。更新和卸载会优先使用 npm。";
    case "appx":
      return "安装来源：Windows 应用商店或 MSIX 应用包。";
    case "registry":
      return "安装来源：标准 Windows 安装程序，已在系统应用列表中登记。";
    case "shortcut":
      return "安装来源：桌面或开始菜单快捷方式。";
    case "native":
      return "安装来源：官方安装脚本或官方安装器。";
    case "app":
      return "安装来源：本地应用。";
    case "download":
      return "安装来源：官方下载。";
    default:
      return surface.install_method_label ? `安装来源：${surface.install_method_label}` : undefined;
  }
}

/** Checked, installed, and still no latest and no error: nothing public to read. */
export function latestUnknown(surface: VibeSurface) {
  return Boolean(surface.installed && surface.latest_checked && !surface.latest && !surface.latest_error);
}

function surfaceStatusText(surface: VibeSurface) {
  if (surface.status === "pending") return surface.kind === "CLI" ? "命令入口待检测" : "桌面端待检测";
  if (surface.version) return `当前版本：${surface.version}`;
  if (surfaceDetected(surface)) {
    return surface.kind === "CLI" ? "命令入口已检测到，暂未获取版本信息" : "桌面端入口已检测到";
  }
  return surface.kind === "CLI" ? "未检测到命令入口" : "未检测到桌面端";
}

/** Badge and meta lines of one card surface, including broken installs. */
export function SurfaceState({ surface }: { surface: VibeSurface }) {
  const others = surface.other_installs ?? [];
  return (
    <>
      <div className="surface-title" title={surface.label}>
        {surface.label}
        {surface.health === "broken" ? <span className="bd e">已损坏</span> : surfaceBadge(surface)}
        {surface.install_method_label && <span className="bd b" title={installMethodHint(surface)}>{surface.install_method_label}</span>}
      </div>
      <div className="surface-desc" title={surface.description}>{surface.description}</div>
      <div className="surface-meta mono" title={surface.path || ""}>
        <span title={surface.broken_reason || undefined}>
          {surface.health === "broken" ? `生效入口无法运行：${surface.broken_reason ?? ""}` : surfaceStatusText(surface)}
        </span>
        {/* A lower "latest" comes from a different version scheme (e.g. Store vs WinGet); hide it. */}
        {surface.latest && (surface.update_available || surface.latest === surface.version) && (
          <span title={surface.latest_source ? `最新版本来自 ${surface.latest_source}` : undefined}>
            {` · 最新版本：${surface.latest}`}{surface.latest_source ? `（${surface.latest_source}）` : ""}
          </span>
        )}
        {latestUnknown(surface) && (
          <span className="surface-muted" title="这个应用没有公开的版本号查询渠道，更新由它自己检查。Stacker 不猜版本号。">
            {" · 最新版本：无公开渠道"}
          </span>
        )}
        {surface.latest_error && <span className="surface-warn" title={surface.latest_error}>{" · 最新版本查询失败"}</span>}
        {surface.path ? ` · ${surface.path}` : ""}
        {others.length > 0 && (
          <span title={others.map((info) => `${info.path}${info.version ? ` (${info.version})` : ""}${info.healthy ? "" : " ✕"}`).join("\n")}>
            {` · 另有 ${others.length} 个安装`}
          </span>
        )}
      </div>
    </>
  );
}

export default function Agents() {
  const { locale } = useI18n();
  const toast = useToast();
  const read = useBusyRead();
  const notices = useNotifications();
  const [tools, setTools] = useState<VibeTool[]>(vibeSnapshot().tools);
  const [loading, setLoading] = useState(vibeSnapshot().loading);
  const [checked, setChecked] = useState(vibeSnapshot().checked);
  const [checkedAt, setCheckedAt] = useState(vibeSnapshot().checkedAt);
  const [promptBusy, setPromptBusy] = useState(false);
  const [checkingTool, setCheckingTool] = useState("");
  const [query, setQuery] = useState("");
  const [edition, setEdition] = useState("all");
  const [status, setStatus] = useState("all");
  const [uninstall, setUninstall] = useState<{ tool: VibeTool; target: "cli" | "desktop"; surface: VibeSurface } | null>(null);
  const [repair, setRepair] = useState<{ tool: VibeTool; surface: VibeSurface } | null>(null);
  const [planOpen, setPlanOpen] = useState(false);
  const [tasks, setTasks] = useState(taskSnapshot());
  const [view, setViewState] = useState<AgentView>(storedView);
  const [detailId, setDetailId] = useState<string | null>(null);
  const setView = (next: AgentView) => {
    setViewState(next);
    try { localStorage.setItem(VIEW_KEY, next); } catch { /* per-viewer convenience only */ }
  };

  useEffect(() => subscribeTasks(setTasks), []);
  const closePlan = useCallback(() => setPlanOpen(false), []);

  useEffect(() => subscribeVibe((s) => {
    setTools(s.tools);
    setLoading(s.loading);
    setChecked(s.checked);
    setCheckedAt(s.checkedAt);
  }), []);

  useEffect(() => {
    void loadCatalog().catch(() => undefined);
  }, []);

  async function load(force = false) {
    return runVibeCheck(force);
  }

  async function refreshAgents() {
    try {
      await read("正在刷新智能体状态", () => load(true), "逐个检测已安装的智能体与最新版本。");
      notices.checkNow("agents").catch(() => undefined);
      toast("智能体状态已刷新", "ok");
    } catch (e) {
      toast("刷新智能体状态失败：" + e, "err");
    }
  }

  async function checkOne(tool: VibeTool) {
    setCheckingTool(tool.id);
    try {
      await read("正在检测智能体环境", () => refreshOneTool(tool.id));
      toast(`${tool.name} 环境检测完成`, "ok");
    } catch (e) {
      toast(`${tool.name} 环境检测失败：` + e, "err");
    } finally {
      setCheckingTool("");
    }
  }

  async function openUrl(url: string) {
    try {
      await invoke("app_open_url", { url });
    } catch (e) {
      toast("打开链接失败：" + e, "err");
    }
  }

  async function openTerminal(tool: VibeTool, command = tool.cli.command) {
    if (!tool.cli.path || !command) return toast("未检测到命令，安装后再打开终端使用。", "info");
    try {
      await invoke("open_shell", { kind: "powershell", cwd: null, command });
      toast(`已在 PowerShell 中运行 ${command}`, "ok");
    } catch (e) {
      toast("打开终端失败：" + e, "err");
    }
  }

  async function openDesktop(tool: VibeTool) {
    try {
      await invoke("vibe_open_desktop", { id: tool.id });
      toast(`已打开 ${tool.desktop.label}`, "ok");
    } catch (e) {
      toast("打开桌面端失败：" + e, "err");
    }
  }

  async function runToolAction(tool: VibeTool, target: "cli" | "desktop", action: AgentTaskAction) {
    setUninstall(null);
    setRepair(null);
    try {
      await startAgentTask(tool.id, target, action);
    } catch (e) {
      toast(`无法创建任务：${e}`, "err");
    }
  }

  async function generatePrompt(copyNow = true) {
    setPromptBusy(true);
    try {
      const text = await read("正在生成智能体摘要", () => invoke<string>("vibe_environment_prompt"));
      if (copyNow) {
        await navigator.clipboard.writeText(translateText(text));
        toast("已安装智能体摘要已复制", "ok");
      } else {
        toast("已安装智能体摘要已生成", "ok");
      }
    } catch (e) {
      toast("生成智能体摘要失败：" + e, "err");
    } finally {
      setPromptBusy(false);
    }
  }

  const cliTotal = tools.filter((t) => t.cli.available).length;
  const desktopTotal = tools.filter((t) => t.desktop.available).length;
  const cliInstalled = tools.filter((t) => t.cli.available && surfaceDetected(t.cli)).length;
  const desktopInstalled = tools.filter((t) => t.desktop.available && surfaceDetected(t.desktop)).length;
  const updates = tools.filter((t) => t.cli.update_available || t.desktop.update_available).length;
  const visibleTools = useMemo(() => {
    const keyword = query.trim().toLocaleLowerCase();
    return [...tools]
      .filter((tool) => edition === "all" || tool.edition === edition)
      .filter((tool) => {
        if (status === "installed") return surfaceDetected(tool.cli) || surfaceDetected(tool.desktop);
        if (status === "missing") return !surfaceDetected(tool.cli) && !surfaceDetected(tool.desktop);
        if (status === "update") return tool.cli.update_available || tool.desktop.update_available;
        return true;
      })
      .filter((tool) => !keyword || `${tool.name} ${tool.description} ${tool.edition_label}`.toLocaleLowerCase().includes(keyword))
      .sort((a, b) => (a.sort_order || 999) - (b.sort_order || 999) || a.name.localeCompare(b.name));
  }, [edition, query, status, tools]);

  function SurfaceRow({ tool, target, surface }: { tool: VibeTool; target: "cli" | "desktop"; surface: VibeSurface }) {
    if (!surface.available) return <UnavailableSurface target={target} surface={surface} />;
    const installed = surfaceDetected(surface);
    const canOpenOfficialDownload = target === "desktop" && Boolean(surface.install_url);
    const installFromOfficialPage = !surface.can_install && canOpenOfficialDownload;
    // With no public version source nothing says whether an update exists; the vendor's page does.
    const noVersionSource = latestUnknown(surface);
    const updateFromOfficialPage = installed && canOpenOfficialDownload && (!surface.can_update || noVersionSource);
    const installTitle = installed
      ? `${surface.label} 已安装`
      : surface.can_install
        ? `安装 ${surface.label}`
        : canOpenOfficialDownload
          ? `${surface.install_unavailable_reason ?? "暂不支持自动安装"}（点击打开 ${surface.label} 官方页面）`
          : surface.install_unavailable_reason || `${surface.label} 暂不支持自动安装`;
    const updateTitle = !installed
      ? `尚未安装 ${surface.label}`
      : updateFromOfficialPage && noVersionSource
        ? `${surface.label} 没有公开的版本号查询渠道，Stacker 判断不了是否需要更新；点击打开官方下载页，或打开应用让它自己检查`
      : updateFromOfficialPage ? `打开 ${surface.label} 官方下载页检查更新`
      : noVersionSource ? `${surface.label} 没有公开的版本号查询渠道，Stacker 判断不了是否需要更新；它会自己检查`
      : surface.latest_error ? `查不到 ${surface.label} 的最新版本，无法判断是否需要更新`
      : !surface.can_update ? `${surface.label} 暂无可自动执行的更新方式`
      : !surface.update_available ? `${surface.label} 已是最新版本` : `更新 ${surface.label}`;
    const uninstallTitle = !installed
      ? `尚未安装 ${surface.label}`
      : surface.can_uninstall ? `卸载 ${surface.label}` : `${surface.label} 暂无可自动执行的卸载方式`;
    const task = openTaskFor(tasks, tool.id, target === "cli" ? tool.cli_id : null, target);
    const taskText = surfaceTaskText(task);
    const busy = Boolean(task);
    return (
      <div className="vtool-surface">
        <span className={"surface-kind " + (target === "cli" ? "cli" : "desktop")}>{target === "cli" ? "CLI" : "桌面端"}</span>
        <div className="surface-main">
          <SurfaceState surface={surface} />
          {taskText && <div className="surface-task"><i className="ti ti-loader spin" /> <span title={taskText}>{taskText}</span></div>}
          {target === "cli" && tool.cli_note && <div className="surface-note"><i className="ti ti-info-circle" /> {tool.cli_note}</div>}
        </div>
        <div className="vtool-actions">
          <button
            className={!installed ? "pr sm" : "gh sm"}
            title={installTitle}
            disabled={busy || installed || (!surface.can_install && !canOpenOfficialDownload)}
            onClick={() => installFromOfficialPage ? openUrl(surface.install_url) : runToolAction(tool, target, "install")}
          >
            {installFromOfficialPage && !installed
              ? <><i className="ti ti-external-link" /> 前往官网</>
              : <><i className="ti ti-download" /> 安装</>}
          </button>
          <button
            className={surface.update_available ? "pr sm" : "gh sm"}
            title={updateTitle}
            disabled={busy || !installed || (!updateFromOfficialPage && (!surface.can_update || !surface.update_available))}
            onClick={() => updateFromOfficialPage ? openUrl(surface.install_url) : runToolAction(tool, target, "update")}
          >
            <i className="ti ti-cloud-upload" /> 更新
          </button>
          <button className="gh sm danger" title={uninstallTitle} disabled={busy || !installed || !surface.can_uninstall} onClick={() => setUninstall({ tool, target, surface })}>
            <i className="ti ti-trash" /> 卸载
          </button>
          {surface.can_repair && (
            <button className="pr sm" title={`移除无法运行的入口，改用本机另一份健康的 ${surface.label}`} disabled={busy} onClick={() => setRepair({ tool, surface })}>
              <i className="ti ti-tool" /> 修复
            </button>
          )}
          {task && (
            <button className="gh sm" title="取消这个任务" onClick={() => void cancelAgentTask(task.id).catch((error) => toast(String(error), "err"))}>
              <i className="ti ti-x" /> 取消
            </button>
          )}
          {target === "cli"
            // A CLI with its own web workbench is used there; a bare terminal adds nothing.
            ? tool.workbench_command
              ? <button className="gh sm" title={`在终端运行 ${tool.workbench_command}，启动本地 Web 工作台`} disabled={!tool.cli.path} onClick={() => openTerminal(tool, tool.workbench_command ?? undefined)}>
                <i className="ti ti-world-www" /> 打开 Web 工作台
              </button>
              : <button className="gh sm" title={tool.cli.path ? `在 PowerShell 中启动 ${surface.label}` : `尚未安装 ${surface.label}`} disabled={!tool.cli.path} onClick={() => openTerminal(tool)}><i className="ti ti-terminal-2" /> 打开终端</button>
            : <button className="gh sm" title={surface.can_open ? `打开 ${surface.label}` : `尚未安装 ${surface.label}`} disabled={!surface.can_open} onClick={() => openDesktop(tool)}><i className="ti ti-app-window" /> 打开桌面端</button>}
        </div>
      </div>
    );
  }

  function renderCard(tool: VibeTool, inDialog = false) {
    return (
            <div className={"vtool eco "
              + (tool.cli.update_available || tool.desktop.update_available ? "update " : "")
              + (checkingTool === tool.id ? "trace-card" : "")}
              key={tool.id} data-dialog={inDialog || undefined}>
              {checkingTool === tool.id && <span className="border-runner" aria-hidden="true" />}
              <div className="vtool-head">
                <span className={`vtool-brand ${tool.id}`} aria-hidden="true">
                  {tool.icon ? <img src={`/brands/${tool.icon}`} alt="" /> : <i className="ti ti-sparkles" />}
                </span>
                <div className="mt">
                  <div className="t">{tool.name}{tool.edition_label && <span className={"bd " + (tool.edition === "cn" ? "w" : tool.edition === "global" ? "b" : "n")}>{tool.edition_label}</span>}</div>
                  <div className="s dim" title={tool.description}>{tool.description}</div>
                </div>
                <div className="ghr">
                  <button className="gh sm" disabled={checkingTool === tool.id} onClick={() => checkOne(tool)}>
                    <i className={"ti " + (checkingTool === tool.id ? "ti-loader spin" : "ti-stethoscope")} /> 环境检测
                  </button>
                  <button className="gh sm" onClick={() => openUrl(tool.docs_url)}>
                    <i className="ti ti-file-text" /> 官方文档
                  </button>
                </div>
              </div>
              <SurfaceRow tool={tool} target="cli" surface={tool.cli} />
              <SurfaceRow tool={tool} target="desktop" surface={tool.desktop} />
            </div>
    );
  }

  function renderTile(tool: VibeTool) {
    const lines = (["cli", "desktop"] as const).map((target) => {
      const surface = target === "cli" ? tool.cli : tool.desktop;
      const busy = Boolean(openTaskFor(tasks, tool.id, target === "cli" ? tool.cli_id : null, target));
      return { target, ...tileLine(surface, busy) };
    });
    const update = tool.cli.update_available || tool.desktop.update_available;
    return (
      <button type="button" key={tool.id} className={"agent-tile" + (update ? " update" : "")}
        title={`${tool.name}${tool.edition_label ? ` · ${tool.edition_label}` : ""} — 点击查看详情和操作`}
        onClick={() => setDetailId(tool.id)}>
        {update && <span className="agent-tile-flag">可更新</span>}
        <span className="agent-tile-head">
          <span className={`vtool-brand ${tool.id}`} aria-hidden="true">
            {tool.icon ? <img src={`/brands/${tool.icon}`} alt="" /> : <i className="ti ti-sparkles" />}
          </span>
          <span className="agent-tile-name">
            <b>{tool.name}</b>
            {tool.edition_label && !tool.name.includes(tool.edition_label) && <small>{tool.edition_label}</small>}
          </span>
        </span>
        {lines.map((line) => (
          <span key={line.target} className={"agent-tile-line " + line.tone}>
            <span className="agent-tile-kind">{line.target === "cli" ? "CLI" : "桌面"}</span>
            {line.tone === "busy" ? <i className="ti ti-loader-2 spin" /> : <i className="agent-tile-dot" />}
            <span className="agent-tile-text">{line.text}</span>
          </span>
        ))}
      </button>
    );
  }

  const detailTool = detailId ? tools.find((tool) => tool.id === detailId) : undefined;

  return (
    <>
      <div className={"checkup agent" + (loading ? " checking" : "")}>
        {loading && <span className="border-runner" aria-hidden="true" />}
        <span className="av agent-hero-icon"><i className={"ti " + (loading ? "ti-loader spin" : "ti-sparkles")} /></span>
        <div className="ct">
          <div className="t1">安装更新</div>
          <div className="t2">{loading
            ? "正在检测各智能体的 CLI、桌面端、版本与安装来源…"
            : checked
              ? `CLI 已安装 ${cliInstalled} / ${cliTotal} · 桌面端已安装 ${desktopInstalled} / ${desktopTotal} · 可更新 ${updates} 项`
              : "已列出支持的智能体。点击“状态刷新”读取本机安装状态、版本和可用操作。"}</div>
          {!loading && checked && checkedAt && <div className="agent-last-check"><i className="ti ti-history" /> 最近检测：{new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(new Date(checkedAt))}</div>}
        </div>
        <div className="cacts">
          <button className="gh sm" disabled={promptBusy} onClick={() => generatePrompt(true)}>
            <i className={"ti " + (promptBusy ? "ti-loader spin" : "ti-copy")} /> {promptBusy ? "生成中…" : "复制摘要给 AI"}
          </button>
          <button className="pr sm" disabled={loading || !checked} title={checked ? "查看可更新的智能体并在后台批量更新" : "请先刷新状态"} onClick={() => setPlanOpen(true)}>
            <i className="ti ti-cloud-upload" /> 一键更新
          </button>
          <button className="gh sm" disabled={loading} onClick={refreshAgents}>
            <i className={"ti " + (loading ? "ti-loader spin" : "ti-refresh")} /> {loading ? "刷新中…" : "状态刷新"}
          </button>
        </div>
      </div>

      {tools.length > 0 && (
        <>
          <div className="agent-catalog-toolbar">
            <div className="seclabel">
              <i className="ti ti-sparkles" /> 智能体列表
              <span className="cnt">显示 {visibleTools.length} / {tools.length} 项 · 可更新 {updates} 项</span>
            </div>
            <div className="agent-catalog-filters">
              <label className="agent-search"><i className="ti ti-search" /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索智能体" /></label>
              <Select value={edition} width={126} onChange={setEdition} options={[
                { value: "all", label: "全部版本" },
                { value: "cn", label: "中国版" },
                { value: "global", label: "国际版" },
                { value: "unified", label: "通用版" },
              ]} />
              <Select value={status} width={126} onChange={setStatus} options={[
                { value: "all", label: "全部状态" },
                { value: "installed", label: "已安装" },
                { value: "missing", label: "未安装" },
                { value: "update", label: "可更新" },
              ]} />
              <div className="agent-view-toggle" role="group" aria-label="显示方式">
                <button type="button" className={view === "tiles" ? "on" : ""} aria-pressed={view === "tiles"}
                  title="缩略图：一屏看全部，点开看详情" onClick={() => setView("tiles")}>
                  <i className="ti ti-layout-grid" />
                </button>
                <button type="button" className={view === "cards" ? "on" : ""} aria-pressed={view === "cards"}
                  title="卡片：每个智能体的全部信息和操作" onClick={() => setView("cards")}>
                  <i className="ti ti-layout-list" />
                </button>
              </div>
            </div>
          </div>
          <div className={view === "tiles" ? "agent-tile-grid" : "agent-catalog-grid"}>
          {visibleTools.map((tool) => view === "tiles" ? renderTile(tool) : renderCard(tool))}
          {visibleTools.length === 0 && <div className="agent-catalog-empty"><i className="ti ti-search-off" /><b>没有符合条件的智能体</b><span>请调整名称、版本或安装状态筛选条件。</span></div>}
          </div>
        </>
      )}

      {detailTool && (
        <Modal title={detailTool.name} icon="ti-sparkles" wide onClose={() => setDetailId(null)}>
          <div className="agent-detail">{renderCard(detailTool, true)}</div>
        </Modal>
      )}
      {planOpen && <UpdatePlanModal onClose={closePlan} />}
      {repair && (
        <ConfirmModal
          title={`修复 ${repair.surface.label}`}
          icon="ti-tool"
          danger
          message={<>将卸载无法运行的生效入口 <code>{repair.surface.path}</code>，之后使用本机另一份健康的安装。不会删除账号登录信息、会话或项目文件。</>}
          confirmLabel="确认修复"
          onClose={() => setRepair(null)}
          onConfirm={() => runToolAction(repair.tool, "cli", "repair")}
        />
      )}
      {uninstall && (
        <ConfirmModal
          title={`卸载 ${uninstall.surface.label}`}
          icon="ti-trash"
          danger
          message={<>将卸载 {uninstall.surface.label}。账号登录信息、历史会话和项目文件通常不会被删除；具体行为取决于该工具的官方卸载器。</>}
          confirmLabel="确认卸载"
          onClose={() => setUninstall(null)}
          onConfirm={() => runToolAction(uninstall.tool, uninstall.target, "uninstall")}
        />
      )}
    </>
  );
}
