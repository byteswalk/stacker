import { useEffect, useState } from "react";
import { invoke } from "../invoke";
import { ConfirmModal, useBusy, useToast } from "../ui";
import { useNotifications } from "../notifications";
import { translateText } from "../i18n";

type VibeSurface = {
  available: boolean;
  label: string;
  kind: string;
  description: string;
  installed: boolean;
  status: "installed" | "update" | "missing" | "unknown";
  version?: string | null;
  probe_error?: string | null;
  latest?: string | null;
  update_available: boolean;
  path?: string | null;
  command?: string | null;
  install_method?: string | null;
  install_method_label?: string | null;
  install_url: string;
  docs_url: string;
  can_install: boolean;
  install_unavailable_reason?: string | null;
  can_update: boolean;
  can_uninstall: boolean;
  can_open: boolean;
};
type VibeTool = {
  id: string;
  name: string;
  description: string;
  docs_url: string;
  cli: VibeSurface;
  desktop: VibeSurface;
};

const TOOL_BRAND_ICONS: Record<string, string> = {
  claude: "/brands/claude.png",
  codex: "/brands/codex.png",
  antigravity: "/brands/antigravity.png",
  opencode: "/brands/opencode-icon.png",
  zcode: "/brands/zcode.svg",
  kimi: "/brands/kimi.ico",
  workbuddy: "/brands/workbuddy.svg",
  qoder: "/brands/qoder.svg",
  "trae-work": "/brands/trae-work.png",
  openclaw: "/brands/openclaw.svg",
  hermes: "/brands/hermes.png",
};

function surfaceBadge(surface: VibeSurface) {
  if (surface.status === "update") return <span className="bd w">可更新</span>;
  if (surface.status === "installed") return <span className="bd g">已安装</span>;
  if (surface.status === "unknown") return <span className="bd b">已检测</span>;
  return <span className="bd n">未安装</span>;
}

function surfaceDetected(surface: VibeSurface) {
  return surface.installed || !!surface.path;
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

function surfaceStatusText(surface: VibeSurface) {
  if (surface.version) return `当前版本：${surface.version}`;
  if (surfaceDetected(surface)) {
    return surface.kind === "CLI" ? "命令入口已检测到，暂未获取版本信息" : "桌面端入口已检测到";
  }
  return surface.kind === "CLI" ? "未检测到命令入口" : "未检测到桌面端";
}

type VibeCache = {
  tools: VibeTool[];
  loading: boolean;
  checked: boolean;
};
const VIBE_INITIAL: VibeCache = {
  tools: [],
  loading: false,
  checked: false,
};
let vibeCache: VibeCache = VIBE_INITIAL;
let vibeRun: Promise<void> | null = null;
const vibeListeners = new Set<(s: VibeCache) => void>();

function publishVibe(next: Partial<VibeCache>) {
  vibeCache = { ...vibeCache, ...next };
  vibeListeners.forEach((fn) => fn(vibeCache));
}

function subscribeVibe(fn: (s: VibeCache) => void) {
  vibeListeners.add(fn);
  return () => { vibeListeners.delete(fn); };
}

function runVibeCheck() {
  if (vibeRun) return vibeRun;
  publishVibe({ loading: true });
  vibeRun = (async () => {
    try {
      const tools = await invoke<VibeTool[]>("vibe_tools");
      publishVibe({ tools, checked: true });
    } finally {
      publishVibe({ loading: false });
      vibeRun = null;
    }
  })();
  return vibeRun;
}

async function refreshOneTool(id: string) {
  const next = await invoke<VibeTool>("vibe_tool", { id });
  const current = vibeCache.tools;
  const exists = current.some((tool) => tool.id === id);
  publishVibe({
    tools: exists
      ? current.map((tool) => tool.id === id ? next : tool)
      : [...current, next],
  });
  return next;
}

export default function Vibe() {
  const toast = useToast();
  const runBusy = useBusy();
  const notices = useNotifications();
  const [tools, setTools] = useState<VibeTool[]>(vibeCache.tools);
  const [loading, setLoading] = useState(vibeCache.loading);
  const [checked, setChecked] = useState(vibeCache.checked);
  const [promptBusy, setPromptBusy] = useState(false);
  const [checkingTool, setCheckingTool] = useState("");
  const [uninstall, setUninstall] = useState<{ tool: VibeTool; target: "cli" | "desktop"; surface: VibeSurface } | null>(null);

  useEffect(() => subscribeVibe((s) => {
    setTools(s.tools);
    setLoading(s.loading);
    setChecked(s.checked);
  }), []);

  async function load() {
    return runVibeCheck();
  }

  async function refreshAgents() {
    try {
      await load();
      notices.checkNow("vibe").catch(() => undefined);
      toast("智能体状态已刷新", "ok");
    } catch (e) {
      toast("刷新智能体状态失败：" + e, "err");
    }
  }

  async function checkOne(tool: VibeTool) {
    setCheckingTool(tool.id);
    try {
      await refreshOneTool(tool.id);
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

  async function openTerminal(tool: VibeTool) {
    if (!tool.cli.path || !tool.cli.command) return toast("未检测到命令，安装后再打开终端使用。", "info");
    try {
      await invoke("open_shell", { kind: "powershell", cwd: null, command: tool.cli.command });
      toast(`已在 PowerShell 中启动 ${tool.cli.label}`, "ok");
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

  async function runToolAction(tool: VibeTool, target: "cli" | "desktop", action: "install" | "update" | "uninstall") {
    const surface = target === "cli" ? tool.cli : tool.desktop;
    const actionText = action === "install" ? "安装" : action === "update" ? "更新" : "卸载";
    const nativeClaudeInstall = tool.id === "claude" && target === "cli" && action === "install";
    const message = nativeClaudeInstall
      ? "正在安装 Claude Code 官方 Windows 原生版，无需预先安装 Node.js。下载或安装连续 30 秒没有响应时会自动停止，也可随时取消。"
      : action === "uninstall"
      ? `正在卸载 ${surface.label}。Stacker 会优先使用检测到的包管理器或系统卸载入口。`
      : `正在${actionText} ${surface.label}。Stacker 会优先按官方推荐方式执行，并在完成后刷新当前项状态。`;
    let cancelled = false;
    try {
      const result = await runBusy(
        {
          title: `${actionText} ${surface.label}`,
          message,
          progressEvent: "vibe-progress",
          cancel: {
            label: `取消${actionText}`,
            onCancel: () => {
              cancelled = true;
              invoke("op_cancel").catch(() => undefined);
            },
          },
        },
        async () => {
          const actionResult = await invoke<string>("vibe_tool_action", { id: tool.id, target, action });
          await refreshOneTool(tool.id);
          return actionResult;
        },
      );
      toast(result || `${surface.label} ${actionText}完成`, "ok");
      setUninstall(null);
      void notices.checkNow("vibe-action").catch(() => undefined);
    } catch (e) {
      const detail = String(e);
      if (cancelled || detail.includes("已取消")) {
        toast(`已取消${surface.label}${actionText}`, "info");
      } else {
        toast(`${actionText}失败：` + detail, "err");
      }
    }
  }

  async function generatePrompt(copyNow = true) {
    setPromptBusy(true);
    try {
      const text = await invoke<string>("vibe_environment_prompt");
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

  function SurfaceRow({ tool, target, surface }: { tool: VibeTool; target: "cli" | "desktop"; surface: VibeSurface }) {
    if (!surface.available) return null;
    const installed = surfaceDetected(surface);
    const canOpenOfficialDownload = target === "desktop" && Boolean(surface.install_url);
    const installFromOfficialPage = !surface.can_install && canOpenOfficialDownload;
    const updateFromOfficialPage = installed && !surface.can_update && canOpenOfficialDownload;
    const installTitle = installed
      ? `${surface.label} 已安装`
      : surface.can_install
        ? `安装 ${surface.label}`
        : canOpenOfficialDownload
          ? `打开 ${surface.label} 官方下载页`
          : surface.install_unavailable_reason || `${surface.label} 暂不支持自动安装`;
    const updateTitle = !installed
      ? `尚未安装 ${surface.label}`
      : updateFromOfficialPage ? `打开 ${surface.label} 官方下载页检查更新`
      : !surface.can_update ? `${surface.label} 暂无可自动执行的更新方式`
      : !surface.update_available ? `${surface.label} 当前无需更新` : `更新 ${surface.label}`;
    const uninstallTitle = !installed
      ? `尚未安装 ${surface.label}`
      : surface.can_uninstall ? `卸载 ${surface.label}` : `${surface.label} 暂无可自动执行的卸载方式`;
    return (
      <div className="vtool-surface">
        <span className={"surface-kind " + (target === "cli" ? "cli" : "desktop")}>{target === "cli" ? "CLI" : "桌面端"}</span>
        <div className="surface-main">
          <div className="surface-title" title={surface.label}>
            {surface.label}
            {surfaceBadge(surface)}
            {surface.install_method_label && <span className="bd b" title={installMethodHint(surface)}>{surface.install_method_label}</span>}
          </div>
          <div className="surface-desc" title={surface.description}>{surface.description}</div>
          <div className="surface-meta mono" title={surface.path || ""}>
            <span title={surface.probe_error || undefined}>{surfaceStatusText(surface)}</span>
            {surface.latest ? ` · 最新版本：${surface.latest}` : ""}
            {surface.path ? ` · ${surface.path}` : ""}
          </div>
        </div>
        <div className="vtool-actions">
          <button
            className={!installed ? "pr sm" : "gh sm"}
            title={installTitle}
            disabled={installed || (!surface.can_install && !canOpenOfficialDownload)}
            onClick={() => installFromOfficialPage ? openUrl(surface.install_url) : runToolAction(tool, target, "install")}
          >
            <i className="ti ti-download" /> 安装
          </button>
          <button
            className={surface.update_available ? "pr sm" : "gh sm"}
            title={updateTitle}
            disabled={!installed || (!updateFromOfficialPage && (!surface.can_update || !surface.update_available))}
            onClick={() => updateFromOfficialPage ? openUrl(surface.install_url) : runToolAction(tool, target, "update")}
          >
            <i className="ti ti-cloud-upload" /> 更新
          </button>
          <button className="gh sm danger" title={uninstallTitle} disabled={!installed || !surface.can_uninstall} onClick={() => setUninstall({ tool, target, surface })}>
            <i className="ti ti-trash" /> 卸载
          </button>
          {target === "cli"
            ? <button className="gh sm" title={tool.cli.path ? `在 PowerShell 中启动 ${surface.label}` : `尚未安装 ${surface.label}`} disabled={!tool.cli.path} onClick={() => openTerminal(tool)}><i className="ti ti-terminal-2" /> 打开终端</button>
            : <button className="gh sm" title={surface.can_open ? `打开 ${surface.label}` : `尚未安装 ${surface.label}`} disabled={!surface.can_open} onClick={() => openDesktop(tool)}><i className="ti ti-app-window" /> 打开桌面端</button>}
        </div>
      </div>
    );
  }

  return (
    <>
      <div className={"checkup agent" + (loading ? " checking" : "")}>
        {loading && <span className="border-runner" aria-hidden="true" />}
        <span className="av" style={{ width: 52, height: 52 }}><i className={"ti " + (loading ? "ti-loader spin" : "ti-sparkles")} /></span>
        <div className="ct">
          <div className="t1">工作智能体生态</div>
          <div className="t2">{loading
            ? "正在检测各智能体的 CLI、桌面端、版本与安装来源…"
            : checked
              ? `CLI 已安装 ${cliInstalled} / ${cliTotal} · 桌面端已安装 ${desktopInstalled} / ${desktopTotal} · 可更新 ${updates} 项`
              : "尚未检测。刷新后可查看各智能体的安装状态、版本和可用操作。"}</div>
        </div>
        <div className="cacts">
          <button className="gh sm" disabled={promptBusy} onClick={() => generatePrompt(true)}>
            <i className={"ti " + (promptBusy ? "ti-loader spin" : "ti-copy")} /> {promptBusy ? "生成中…" : "复制摘要给 AI"}
          </button>
          <button className="pr sm" disabled={loading} onClick={refreshAgents}>
            <i className={"ti " + (loading ? "ti-loader spin" : "ti-refresh")} /> {loading ? "刷新中…" : "状态刷新"}
          </button>
        </div>
      </div>

      {!checked && (
        <div className="banner gray"><i className="ti ti-sparkles lead" /><div className="bt"><b>智能体状态尚未读取。</b><br />点击“状态刷新”检测已安装的 CLI 和桌面端，并检查可用更新。</div></div>
      )}

      {checked && (
        <>
          <div className="seclabel">
            <i className="ti ti-sparkles" /> 工作智能体生态
            <span className="cnt">共 {tools.length} 项 · 可更新 {updates} 项</span>
          </div>
          {tools.map((tool) => {
            const surfaceCount = Number(tool.cli.available) + Number(tool.desktop.available);
            return (
            <div className={"vtool eco "
              + (tool.cli.update_available || tool.desktop.update_available ? "update " : "")
              + (checkingTool === tool.id ? "trace-card" : "")}
              style={{ minHeight: 64 + surfaceCount * 84 }}
              key={tool.id}>
              {checkingTool === tool.id && <span className="border-runner" aria-hidden="true" />}
              <div className="vtool-head">
                <span className={`vtool-brand ${tool.id}`} aria-hidden="true">
                  {TOOL_BRAND_ICONS[tool.id] ? <img src={TOOL_BRAND_ICONS[tool.id]} alt="" /> : <i className="ti ti-sparkles" />}
                </span>
                <div className="mt">
                  <div className="t">{tool.name}</div>
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
          })}

        </>
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
