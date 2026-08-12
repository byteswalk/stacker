import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Select } from "../../Select";
import { useI18n } from "../../i18n";
import { ConfirmModal, Modal, useToast } from "../../ui";
import { formatSpaceBytes } from "../space-analysis/components/SpaceOverview";
import {
  cleanupTargetsForTrackingRoots,
  deleteWorkSessionReport,
  loadWorkEnvironmentContract,
  loadWorkSessionReports,
  loadWorkSessionTrackingRoots,
  loadDesktopSessionProcesses,
  managedWorkSessionSnapshot,
  openWorkSessionPath,
  prepareWorkSessionCleanup,
  recoverInterruptedWorkSession,
  saveManagedWorkSessionProfile,
  startManagedWorkSession,
  stopManagedWorkSession,
  subscribeManagedWorkSession,
  updateManagedWorkSessionProfile,
  type ManagedWorkSessionState,
  type WorkSessionProfile,
  type WorkSessionReport,
  type WorkSessionTrackingRoot,
  type WorkSessionDesktopProcess,
} from "./managedWorkSessionStore";

export interface ManagedAgentOption {
  id: string;
  name: string;
  cliInstalled: boolean;
  cliPath: string | null;
  desktopInstalled: boolean;
  desktopPath: string | null;
  desktopName: string;
}

function signedBytes(bytes: number) {
  if (bytes === 0) return "0 B";
  return `${bytes > 0 ? "+" : "-"}${formatSpaceBytes(Math.abs(bytes))}`;
}

function elapsed(startedAt: string, endedAt: string, en: boolean) {
  const milliseconds = Math.max(0, Date.parse(endedAt) - Date.parse(startedAt));
  const minutes = Math.floor(milliseconds / 60_000);
  if (minutes < 1) return en ? "under 1 min" : "不足 1 分钟";
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (!hours) return en ? `${minutes} min` : `${minutes} 分钟`;
  return en ? `${hours} h ${rest} min` : `${hours} 小时 ${rest} 分钟`;
}

const CHINESE_TRACKING_ROOTS: Record<string, { label: string; reason: string }> = {
  project: { label: "项目工作区", reason: "项目源代码与生成内容。只跟踪变化，不提供自动清理。" },
  "cache:npm": { label: "npm 包缓存", reason: "已下载的 npm 包，可在需要时重新获取。" },
  "cache:pnpm": { label: "pnpm 包存储", reason: "pnpm 的共享包存储，可通过重新安装依赖恢复。" },
  "cache:pip": { label: "pip 下载缓存", reason: "已下载的 Python 包，可在需要时重新获取。" },
  "cache:maven": { label: "Maven 本地仓库", reason: "依赖通常可重新下载；离线包和私有制品需清理前复核。" },
  "cache:gradle": { label: "Gradle 缓存", reason: "Gradle 可重新生成依赖与构建转换缓存。" },
  "cache:go": { label: "Go 模块缓存", reason: "Go 模块可通过已配置的模块源重新下载。" },
  "cache:cargo-registry": { label: "Cargo 注册表缓存", reason: "Crate 压缩包可通过已配置的注册表重新下载。" },
  "cache:cargo-git": { label: "Cargo Git 缓存", reason: "Cargo 使用的 Git 依赖可在需要时重新获取。" },
};

function trackingRootText(root: WorkSessionTrackingRoot, en: boolean) {
  if (en) return { label: root.label, reason: root.reason };
  if (root.id.startsWith("agent:")) {
    return { label: "工作智能体数据", reason: "智能体配置、会话与本地状态。只观测增长，不作为可清理内容。" };
  }
  return CHINESE_TRACKING_ROOTS[root.id] ?? { label: root.label, reason: root.reason };
}

function workSessionErrorText(error: unknown, en: boolean) {
  const text = String(error);
  if (en) return text;
  const exact: Record<string, string> = {
    "A managed work session is already active.": "已有工作会话正在运行。",
    "Select a project folder first.": "请先选择项目目录。",
    "Select an installed work agent first.": "请先选择已安装的工作智能体。",
    "Project tracking is required for a managed work session.": "工作会话必须跟踪项目目录。",
    "Select whether to launch the desktop app or attach a running app.": "请选择启动桌面应用或关联正在运行的应用。",
    "Select a running desktop process to attach.": "请选择要关联的桌面进程。",
    "The selected desktop process is no longer running.": "所选桌面进程已经退出，请刷新后重选。",
    "The selected project folder does not exist or cannot be accessed.": "所选项目目录不存在或无法访问。",
    "A project folder is required.": "请选择有效的项目目录。",
    "Unable to establish the tracking baseline.": "无法建立目录跟踪基线。",
    "Work session start was cancelled.": "工作会话启动已取消。",
    "Tracking did not stop within the expected time. The session evidence remains available for recovery.": "目录跟踪未能按时停止，会话记录仍保留，可在下次进入时恢复。",
  };
  return exact[text] ?? text;
}

export function ManagedWorkSession({ agents, onOpenCleanup }: {
  agents: ManagedAgentOption[];
  onOpenCleanup?: () => void;
}) {
  const { locale } = useI18n();
  const toast = useToast();
  const en = locale === "en-US";
  const [session, setSession] = useState<ManagedWorkSessionState>(managedWorkSessionSnapshot());
  const [detail, setDetail] = useState<WorkSessionReport | null>(null);
  const [removeReport, setRemoveReport] = useState<WorkSessionReport | null>(null);
  const [desktopProcesses, setDesktopProcesses] = useState<WorkSessionDesktopProcess[]>([]);
  const [desktopProcessesLoading, setDesktopProcessesLoading] = useState(false);
  const contractAutoLoadAttempted = useRef(false);
  const installedAgents = useMemo(() => agents.filter((agent) => session.profile.mode === "desktop" ? agent.desktopInstalled : agent.cliInstalled && agent.cliPath), [agents, session.profile.mode]);
  const busy = session.phase === "baseline" || session.phase === "launching" || session.phase === "stopping";
  const active = session.phase === "tracking" || busy;
  const rootsKey = `${session.profile.workspace}|${session.profile.agentId}|${[...session.profile.enabledItems].sort().join(",")}`;
  const copy = en ? {
    notice: "CLI sessions control the approved command environment. Desktop sessions observe the selected app and folders without changing the app environment.",
    title: "Managed work session", subtitle: "Run or observe an agent and keep an evidence-based record of its disk impact.",
    project: "Project folder", choose: "Choose folder", open: "Open folder", agent: "Work agent", shell: "Terminal", mode: "Session type", cliMode: "CLI", desktopMode: "Desktop app",
    noAgents: "No installed agent is available for this session type. Refresh agent status and install one first.", desktopAction: "Desktop action", launchDesktop: "Launch app", attachDesktop: "Attach running app", process: "Running process", refreshProcesses: "Refresh processes", noProcesses: "No matching desktop process is running.",
    contract: "Approved environment", contractDesc: "Checked commands are added to this session PATH. Other development tools stay outside the managed terminal.",
    refresh: "Refresh environment", save: "Save profile", saved: "Work profile saved.", copy: "Copy summary for AI",
    start: "Start session", stop: "Finish session", baseline: "Measuring the selected folders before the agent starts...",
    launching: "Preparing the selected work agent...", tracking: "Work session in progress",
    trackingDesc: "Continue working in the selected agent, then finish the session here to preserve the final report.",
    stopped: "Session finished. The recorded disk impact is available below.", unavailable: "Unavailable", changed: "The saved profile no longer matches the current machine.",
    scope: "Tracking scope", scopeDesc: "Track the project, agent data and selected caches together. Protected data is measured but never offered for cleanup.",
    scopeLoading: "Reading available tracking folders...", selected: "selected", required: "Required", protected: "Protected", review: "Review before cleanup", rebuildable: "Rebuildable",
    projectCategory: "Project", agentCategory: "Agent data", cacheCategory: "Cache",
    diskImpact: "Tracked space change", files: "Changed files", current: "Current tracked size", agents: "Agent processes",
    baselineFiles: "Files measured", baselineDirectories: "Folders measured", measured: "Space measured", skipped: "Skipped paths",
    directories: "Largest directory changes", recent: "Recent file changes", cleanup: "Review cache cleanup",
    attribution: "Stacker records changes inside the selected folders. Process names are supporting context, not unsupported file-level attribution.",
    selectProject: "Select a project folder first.", selectAgent: "Select an installed agent first.", selectProcess: "Select a running desktop process first.", copied: "Environment summary copied for AI.", desktopLimit: "Desktop observation cannot replace the environment inherited by an already running app. Ending observation does not close the app.",
    history: "Session reports", historyDesc: "Finished and interrupted sessions are retained locally for review.", noHistory: "No work session report yet.",
    view: "View report", remove: "Delete report", deleteTitle: "Delete session report", deleteMessage: "This deletes only the local report. It does not delete project files, agent data or caches.",
    report: "Work session report", status: "Status", duration: "Duration", ended: "Ended", tracked: "Tracked folders", environment: "Approved commands", processes: "Observed processes",
    setup: "Session setup", setupDesc: "Choose the project, work agent and launch method used by this session.", processUnbound: "The desktop app was launched, but its process could not be identified. Tracking will continue until you finish it manually.",
    completed: "Completed", failed: "Failed", interrupted: "Interrupted", close: "Close", cleanupReady: "Cache folders were sent to Disk Cleanup for review.",
    noCleanup: "This report has no rebuildable cache folder to review.", recovered: "An interrupted work session was recovered and saved as a report.",
  } : {
    notice: "CLI 会话使用已确认命令环境；桌面会话只观察所选应用和目录，不会改写桌面应用的运行环境。",
    title: "受控工作会话", subtitle: "启动或观察工作智能体，并持续记录本次工作的磁盘影响。",
    project: "项目目录", choose: "选择目录", open: "打开目录", agent: "工作智能体", shell: "终端", mode: "会话类型", cliMode: "CLI", desktopMode: "桌面端",
    noAgents: "当前会话类型没有可用的智能体，请先刷新状态并完成安装。", desktopAction: "桌面操作", launchDesktop: "启动应用", attachDesktop: "关联运行中应用", process: "运行中进程", refreshProcesses: "刷新进程", noProcesses: "没有检测到匹配的桌面进程。",
    contract: "已确认环境", contractDesc: "勾选的命令会加入本次会话 PATH，其他开发工具不会进入受控终端。",
    refresh: "刷新环境", save: "保存配置", saved: "工作配置已保存。", copy: "复制摘要给 AI",
    start: "开始会话", stop: "结束会话", baseline: "正在统计所选目录，完成后再启动智能体…",
    launching: "正在准备所选工作智能体…", tracking: "工作会话进行中",
    trackingDesc: "请继续在所选智能体中工作；完成后在此结束会话，以保存最终报告。",
    stopped: "会话已结束，记录的磁盘影响保留在下方。", unavailable: "不可用", changed: "已保存配置与本机当前环境不一致。",
    scope: "跟踪范围", scopeDesc: "同时跟踪项目、智能体数据和选定缓存。受保护数据只统计，不提供清理。",
    scopeLoading: "正在读取可跟踪目录…", selected: "已选", required: "必选", protected: "受保护", review: "清理前复核", rebuildable: "可重新生成",
    projectCategory: "项目", agentCategory: "智能体数据", cacheCategory: "缓存",
    diskImpact: "跟踪空间变化", files: "变更文件", current: "当前跟踪占用", agents: "智能体进程",
    baselineFiles: "已统计文件", baselineDirectories: "已统计目录", measured: "已统计空间", skipped: "已跳过路径",
    directories: "空间变化集中目录", recent: "最近文件变化", cleanup: "复核缓存清理",
    attribution: "Stacker 记录所选目录内的变化。进程名称只提供运行上下文，不会在缺少证据时强行归因到具体文件。",
    selectProject: "请先选择项目目录。", selectAgent: "请先选择已安装的智能体。", selectProcess: "请先选择一个正在运行的桌面进程。", copied: "环境摘要已复制，可交给 AI 使用。", desktopLimit: "桌面观察不能替换应用已经继承的环境；结束观察也不会关闭桌面应用。",
    history: "会话报告", historyDesc: "已完成和意外中断的会话会保存在本机，供后续复盘。", noHistory: "还没有工作会话报告。",
    view: "查看报告", remove: "删除报告", deleteTitle: "删除会话报告", deleteMessage: "只删除本机报告，不会删除项目文件、智能体数据或缓存。",
    report: "工作会话报告", status: "状态", duration: "持续时间", ended: "结束时间", tracked: "跟踪目录", environment: "已确认命令", processes: "观察到的进程",
    setup: "会话设置", setupDesc: "选择本次工作的项目、智能体与启动方式。", processUnbound: "桌面应用已启动，但暂未识别到对应进程；目录跟踪会继续运行，请在工作结束后手动完成会话。",
    completed: "已完成", failed: "失败", interrupted: "意外中断", close: "关闭", cleanupReady: "缓存目录已送入磁盘清理页面，等待你复核。",
    noCleanup: "这份报告没有可供复核的可重建缓存目录。", recovered: "已恢复一份意外中断的工作会话，并保存为报告。",
  };

  useEffect(() => subscribeManagedWorkSession(setSession), []);
  useEffect(() => {
    void (async () => {
      try {
        await loadWorkSessionReports();
        if (await recoverInterruptedWorkSession()) toast(copy.recovered, "info");
      } catch (error) {
        toast(workSessionErrorText(error, en), "err");
      }
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (session.profile.mode === "cli" && !session.contract && !session.contractLoading && !contractAutoLoadAttempted.current) {
      contractAutoLoadAttempted.current = true;
      void loadWorkEnvironmentContract().catch((error) => toast(workSessionErrorText(error, en), "err"));
    }
  }, [session.profile.mode, session.contract, session.contractLoading]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!session.profile.agentId && installedAgents[0]) updateManagedWorkSessionProfile({ agentId: installedAgents[0].id });
  }, [installedAgents, session.profile.agentId]);
  useEffect(() => {
    if (active || session.profile.mode !== "desktop" || session.profile.desktopAction !== "attach" || !session.profile.agentId) {
      setDesktopProcesses([]);
      return;
    }
    setDesktopProcessesLoading(true);
    void loadDesktopSessionProcesses(session.profile.agentId)
      .then((processes) => {
        setDesktopProcesses(processes);
        if (session.profile.desktopPid && !processes.some((process) => process.pid === session.profile.desktopPid)) {
          patchProfile({ desktopPid: null });
        }
      })
      .catch((error) => toast(workSessionErrorText(error, en), "err"))
      .finally(() => setDesktopProcessesLoading(false));
  }, [active, session.profile.mode, session.profile.desktopAction, session.profile.agentId]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!active && session.profile.workspace && session.profile.agentId) {
      void loadWorkSessionTrackingRoots().catch((error) => toast(workSessionErrorText(error, en), "err"));
    }
  }, [active, rootsKey]); // eslint-disable-line react-hooks/exhaustive-deps

  function patchProfile(patch: Partial<WorkSessionProfile>) {
    updateManagedWorkSessionProfile(patch);
  }

  async function chooseProject() {
    const selected = await open({ directory: true, multiple: false, defaultPath: session.profile.workspace || undefined, title: copy.project });
    if (typeof selected === "string") patchProfile({ workspace: selected, trackingRootIds: [], trackingRootsConfigured: false });
  }

  async function start() {
    if (!session.profile.workspace) return toast(copy.selectProject, "info");
    if (!session.profile.agentId) return toast(copy.selectAgent, "info");
    if (session.profile.mode === "desktop" && session.profile.desktopAction === "attach" && !session.profile.desktopPid) return toast(copy.selectProcess, "info");
    try {
      await startManagedWorkSession();
      toast(en ? "Managed work session started." : "受控工作会话已启动。", "ok");
    } catch (error) {
      toast((en ? "Unable to start work session: " : "启动工作会话失败：") + workSessionErrorText(error, en), "err");
    }
  }

  async function stop() {
    try {
      await stopManagedWorkSession();
      toast(en ? "Work session report saved." : "工作会话已结束并保存报告。", "ok");
    } catch (error) {
      toast((en ? "Unable to finish the work session: " : "结束工作会话失败：") + workSessionErrorText(error, en), "err");
    }
  }

  async function copyContract() {
    const selectedAgent = installedAgents.find((agent) => agent.id === session.profile.agentId);
    const selected = (session.contract?.items ?? []).filter((item) => item.available && session.profile.enabledItems.includes(item.id));
    const roots = session.trackingRoots.filter((root) => session.profile.trackingRootIds.includes(root.id));
    const selectedProcess = session.launch?.process
      ?? desktopProcesses.find((process) => process.pid === session.profile.desktopPid);
    const safetyText = (root: WorkSessionTrackingRoot) => en
      ? root.safety
      : root.safety === "protected" ? "只观察" : root.safety === "review" ? "清理前复核" : "可重新生成";
    const desktopMethod = en
      ? session.profile.desktopAction === "attach" ? "Attach to a running desktop process" : "Launch the desktop application"
      : session.profile.desktopAction === "attach" ? "关联运行中的桌面进程" : "启动桌面应用";
    const lines = en ? [
      "# Local Work Environment",
      `- Project: ${session.profile.workspace || "Not selected"}`,
      `- Work agent: ${selectedAgent?.name || "Not selected"}`,
      `- Session type: ${session.profile.mode === "desktop" ? `Desktop observation (${desktopMethod})` : `Managed terminal (${session.profile.shell})`}`,
      ...(selectedProcess ? [`- Desktop process: ${selectedProcess.processName}; PID ${selectedProcess.pid}`] : []),
      ...(session.profile.mode === "cli" ? ["", "## Approved local commands", ...selected.map((item) => `- ${item.label}: ${item.version || "version unavailable"}; path: ${item.path}`)] : []),
      "", "## Observed folders",
      ...roots.map((root) => `- ${trackingRootText(root, true).label}: ${root.path}; policy: ${safetyText(root)}`),
      "", "## Operating rules",
      "- Treat the selected project folder as the working directory unless the user says otherwise.",
      "- Prefer the approved commands above. Do not replace or reinstall working runtimes without confirmation.",
      "- Do not modify global or system environment variables. Ask before adding large dependencies or generated artifacts.",
      ...(session.profile.mode === "desktop" ? ["- Desktop observation does not control the environment inherited by the running app and does not close it when tracking ends."] : []),
    ] : [
      "# 本机工作环境",
      `- 项目目录：${session.profile.workspace || "未选择"}`,
      `- 工作智能体：${selectedAgent?.name || "未选择"}`,
      `- 会话类型：${session.profile.mode === "desktop" ? `桌面应用观察（${desktopMethod}）` : `受控终端（${session.profile.shell}）`}`,
      ...(selectedProcess ? [`- 桌面进程：${selectedProcess.processName}；进程 ID ${selectedProcess.pid}`] : []),
      ...(session.profile.mode === "cli" ? ["", "## 已确认本机命令", ...selected.map((item) => `- ${item.label}：${item.version || "版本未知"}；路径：${item.path}`)] : []),
      "", "## 观察目录",
      ...roots.map((root) => `- ${trackingRootText(root, false).label}：${root.path}；处理策略：${safetyText(root)}`),
      "", "## 操作约束",
      "- 除非用户另有说明，将所选项目目录作为工作目录。",
      "- 优先使用上方已确认命令；未经确认，不要替换或重新安装当前可用的运行时。",
      "- 不要修改全局或系统环境变量；添加大型依赖或生成大量构建产物前先征得用户确认。",
      ...(session.profile.mode === "desktop" ? ["- 桌面观察不会改变应用已经继承的运行环境，结束观察也不会关闭该应用。"] : []),
    ];
    try {
      await navigator.clipboard.writeText(lines.join("\n"));
      toast(copy.copied, "ok");
    } catch (error) {
      toast((en ? "Unable to copy the summary: " : "复制摘要失败：") + workSessionErrorText(error, en), "err");
    }
  }

  function toggleItem(id: string) {
    const enabled = new Set(session.profile.enabledItems);
    if (enabled.has(id)) enabled.delete(id); else enabled.add(id);
    patchProfile({ enabledItems: [...enabled] });
  }

  function toggleRoot(root: WorkSessionTrackingRoot) {
    if (root.id === "project") return;
    const selected = new Set(session.profile.trackingRootIds);
    if (selected.has(root.id)) selected.delete(root.id); else selected.add(root.id);
    selected.add("project");
    patchProfile({ trackingRootIds: [...selected], trackingRootsConfigured: true });
  }

  function openCleanup(roots: readonly WorkSessionTrackingRoot[]) {
    const result = prepareWorkSessionCleanup(roots);
    if (!result.targets.length) return toast(copy.noCleanup, "info");
    if (!result.stored) return toast(en ? "Unable to prepare cleanup review." : "无法准备清理复核。", "err");
    toast(copy.cleanupReady, "ok");
    onOpenCleanup?.();
  }

  const monitor = session.monitor;
  const contractChanged = Boolean(session.profile.contractFingerprint && session.contract && session.profile.contractFingerprint !== session.contract.fingerprint);
  const selectedRootCount = session.trackingRoots.filter((root) => session.profile.trackingRootIds.includes(root.id)).length;
  const statusLabel = (status: WorkSessionReport["status"]) => status === "completed" ? copy.completed : status === "failed" ? copy.failed : copy.interrupted;
  const safetyLabel = (root: WorkSessionTrackingRoot) => root.safety === "protected" ? copy.protected : root.safety === "review" ? copy.review : copy.rebuildable;
  const categoryLabel = (root: WorkSessionTrackingRoot) => root.category === "project" ? copy.projectCategory : root.category === "agent-data" ? copy.agentCategory : copy.cacheCategory;

  return <div className="agent-prototype-page managed-work-session">
    <div className="agent-prototype-notice"><i className="ti ti-shield-check" /><span>{copy.notice}</span></div>
    <div className={`agent-prototype-hero ${busy ? "trace-card" : ""}`}>
      {busy && <span className="border-runner" aria-hidden="true" />}
      <div className="agent-prototype-hero-icon"><i className={`ti ${busy ? "ti-loader spin" : "ti-terminal-2"}`} /></div>
      <div className="agent-prototype-hero-copy"><b>{copy.title}</b><span>{copy.subtitle}</span></div>
      {active
        ? <button className="gh sm danger" disabled={session.phase === "stopping"} onClick={() => void stop()}><i className="ti ti-player-stop" /> {copy.stop}</button>
        : <button className="pr sm" disabled={(session.profile.mode === "cli" && session.contractLoading) || session.trackingRootsLoading} onClick={() => void start()}><i className="ti ti-player-play" /> {copy.start}</button>}
    </div>

    <section className="agent-panel managed-session-config">
      <div className="agent-panel-head managed-config-head"><div><b>{copy.setup}</b><span>{copy.setupDesc}</span></div><div className="managed-contract-actions">
        <button className="gh sm" disabled={active || session.profileSaved} onClick={() => { saveManagedWorkSessionProfile(); toast(copy.saved, "ok"); }}><i className="ti ti-device-floppy" /> {copy.save}</button>
        <button className="gh sm" disabled={!session.profile.workspace || !session.profile.agentId} onClick={() => void copyContract()}><i className="ti ti-copy" /> {copy.copy}</button>
      </div></div>
      <div className="managed-project-field">
        <span>{copy.project}</span><b title={session.profile.workspace}>{session.profile.workspace || copy.selectProject}</b>
        <button className="gh sm" disabled={active} onClick={() => void chooseProject()}><i className="ti ti-folder" /> {copy.choose}</button>
        <button className="space-icon-button" title={copy.open} disabled={!session.profile.workspace} onClick={() => void openWorkSessionPath(session.profile.workspace).catch(() => toast(copy.selectProject, "err"))}><i className="ti ti-folder-open" /></button>
      </div>
      <div className="managed-session-choice">
        <div><span>{copy.mode}</span><div className="managed-shell-switch"><button disabled={active} className={session.profile.mode === "cli" ? "active" : ""} onClick={() => patchProfile({ mode: "cli", agentId: "", desktopPid: null, trackingRootIds: [], trackingRootsConfigured: false })}><i className="ti ti-terminal-2" /> {copy.cliMode}</button><button disabled={active} className={session.profile.mode === "desktop" ? "active" : ""} onClick={() => patchProfile({ mode: "desktop", agentId: "", desktopPid: null, trackingRootIds: [], trackingRootsConfigured: false })}><i className="ti ti-app-window" /> {copy.desktopMode}</button></div></div>
        <label><span>{copy.agent}</span><Select value={session.profile.agentId} disabled={active || installedAgents.length === 0} onChange={(agentId) => patchProfile({ agentId, desktopPid: null, trackingRootIds: [], trackingRootsConfigured: false })} placeholder={copy.noAgents} options={installedAgents.map((agent) => ({ value: agent.id, label: agent.name, title: (session.profile.mode === "desktop" ? agent.desktopPath : agent.cliPath) || agent.name }))} /></label>
        {session.profile.mode === "cli" ? <div><span>{copy.shell}</span><div className="managed-shell-switch">{(["powershell", "gitbash", "cmd"] as const).map((shell) => <button key={shell} disabled={active} className={session.profile.shell === shell ? "active" : ""} onClick={() => patchProfile({ shell })}>{shell === "powershell" ? "PowerShell" : shell === "gitbash" ? "Git Bash" : "cmd"}</button>)}</div></div> : <>
          <div><span>{copy.desktopAction}</span><div className="managed-shell-switch"><button disabled={active} className={session.profile.desktopAction === "launch" ? "active" : ""} onClick={() => patchProfile({ desktopAction: "launch", desktopPid: null })}><i className="ti ti-player-play" /> {copy.launchDesktop}</button><button disabled={active} className={session.profile.desktopAction === "attach" ? "active" : ""} onClick={() => patchProfile({ desktopAction: "attach" })}><i className="ti ti-link" /> {copy.attachDesktop}</button></div></div>
          {session.profile.desktopAction === "attach" && <label><span>{copy.process}</span><div className="managed-process-choice"><Select value={session.profile.desktopPid?.toString() || ""} disabled={active || desktopProcessesLoading} onChange={(pid) => patchProfile({ desktopPid: Number(pid) || null })} placeholder={desktopProcessesLoading ? copy.refreshProcesses : copy.noProcesses} options={desktopProcesses.map((process) => ({ value: process.pid.toString(), label: `${process.processName} · PID ${process.pid}`, title: `${process.desktopName} · PID ${process.pid}` }))} /><button className="space-icon-button" disabled={active || desktopProcessesLoading || !session.profile.agentId} title={copy.refreshProcesses} onClick={() => { setDesktopProcessesLoading(true); void loadDesktopSessionProcesses().then(setDesktopProcesses).catch((error) => toast(String(error), "err")).finally(() => setDesktopProcessesLoading(false)); }}><i className={`ti ${desktopProcessesLoading ? "ti-loader spin" : "ti-refresh"}`} /></button></div></label>}
        </>}
      </div>
      {installedAgents.length === 0 && <div className="agent-scope-note"><i className="ti ti-info-circle" /><span>{copy.noAgents}</span></div>}
      {session.profile.mode === "desktop" && <div className="agent-scope-note"><i className="ti ti-eye" /><span>{copy.desktopLimit}</span></div>}
      {session.phase === "tracking" && session.launch?.mode === "desktop" && !session.launch.process && <div className="managed-contract-warning"><i className="ti ti-info-circle" /> {copy.processUnbound}</div>}
    </section>

    {session.profile.mode === "cli" && <section className="agent-panel managed-contract">
      <div className="agent-panel-head managed-contract-head"><div><b>{copy.contract}</b><span>{copy.contractDesc}</span></div><div className="managed-contract-actions">
        <button className="gh sm" disabled={session.contractLoading || active} onClick={() => void loadWorkEnvironmentContract(true).catch((error) => toast(String(error), "err"))}><i className={`ti ${session.contractLoading ? "ti-loader spin" : "ti-refresh"}`} /> {copy.refresh}</button>
      </div></div>
      {contractChanged && <div className="managed-contract-warning"><i className="ti ti-alert-triangle" /> {copy.changed}</div>}
      <div className="managed-contract-list">
        {session.contract?.items.map((item) => <label className={`managed-contract-item ${item.available ? "" : "unavailable"}`} key={item.id} title={item.path || copy.unavailable}>
          <input type="checkbox" disabled={!item.available || active} checked={item.available && session.profile.enabledItems.includes(item.id)} onChange={() => toggleItem(item.id)} />
          <span className="managed-contract-icon"><i className={`ti ${item.kind === "runtime" ? "ti-player-play" : item.kind === "package-manager" ? "ti-package" : item.kind === "source-control" ? "ti-brand-git" : "ti-tool"}`} /></span>
          <span><b>{item.label}</b><small>{item.available ? item.version || item.path : copy.unavailable}</small></span>
          <code>{item.path || "-"}</code>
        </label>)}
      </div>
    </section>}

    <section className="agent-panel managed-tracking-scope">
      <div className="agent-panel-head"><div><b>{copy.scope}</b><span>{copy.scopeDesc}</span></div><span className="bd b">{selectedRootCount} {copy.selected}</span></div>
      {session.trackingRootsLoading
        ? <div className="managed-scope-loading"><i className="ti ti-loader spin" /> {copy.scopeLoading}</div>
        : <div className="managed-tracking-root-list">{session.trackingRoots.map((root) => { const text = trackingRootText(root, en); return <label className={`managed-tracking-root ${root.safety}`} key={root.id} title={`${text.reason}\n${root.path}`}>
          <input type="checkbox" disabled={active || root.id === "project"} checked={session.profile.trackingRootIds.includes(root.id)} onChange={() => toggleRoot(root)} />
          <span className="managed-tracking-root-icon"><i className={`ti ${root.category === "project" ? "ti-folders" : root.category === "agent-data" ? "ti-brain" : "ti-database"}`} /></span>
          <span className="managed-tracking-root-copy"><b>{text.label}</b><small>{text.reason}</small><code>{root.path}</code></span>
          <span className="managed-root-badges"><em>{categoryLabel(root)}</em><em className={root.safety}>{root.id === "project" ? copy.required : safetyLabel(root)}</em></span>
        </label>; })}</div>}
    </section>

    {(session.phase !== "idle" || monitor) && <section className={`agent-panel managed-session-status ${active ? "active" : ""}`}>
      <div className="agent-panel-head"><div><b>{session.phase === "tracking" ? copy.tracking : session.phase === "stopped" ? copy.stopped : copy.title}</b><span>{session.phase === "baseline" ? copy.baseline : session.phase === "launching" ? copy.launching : session.phase === "tracking" ? copy.trackingDesc : session.error || copy.stopped}</span></div>{session.launch && <span className="bd g">{session.launch.agentName} · {session.launch.mode === "desktop" ? copy.desktopMode : session.launch.shell}</span>}</div>
      {monitor && <>
        <div className="managed-session-metrics">
          {session.phase === "baseline" ? <>
            <div><span>{copy.baselineFiles}</span><b>{monitor.filesScanned.toLocaleString()}</b></div>
            <div><span>{copy.baselineDirectories}</span><b>{monitor.directoriesScanned.toLocaleString()}</b></div>
            <div><span>{copy.measured}</span><b>{formatSpaceBytes(monitor.currentBytes)}</b></div>
            <div><span>{copy.skipped}</span><b>{monitor.skippedPaths.toLocaleString()}</b></div>
          </> : <>
            <div><span>{copy.diskImpact}</span><b className={monitor.deltaBytes > 0 ? "negative" : "positive"}>{signedBytes(monitor.deltaBytes)}</b></div>
            <div><span>{copy.files}</span><b>{monitor.filesChanged.toLocaleString()}</b></div>
            <div><span>{copy.current}</span><b>{formatSpaceBytes(monitor.currentBytes)}</b></div>
            <div><span>{copy.agents}</span><b>{monitor.runningAgents.length.toLocaleString()}</b></div>
          </>}
        </div>
        <div className="managed-session-columns">
          <div><strong>{copy.directories}</strong>{monitor.directories.slice(0, 6).map((directory) => <button key={directory.path} title={directory.path} onClick={() => void openWorkSessionPath(directory.path)}><span>{directory.path}</span><b>{signedBytes(directory.deltaBytes)}</b></button>)}</div>
          <div><strong>{copy.recent}</strong>{monitor.events.slice(0, 6).map((event) => <button key={`${event.kind}:${event.path}:${event.modifiedAt}`} title={event.path} onClick={() => void openWorkSessionPath(event.path)}><span>{event.path}</span><b>{signedBytes(event.deltaBytes)}</b></button>)}</div>
        </div>
        <div className="agent-scope-note"><i className="ti ti-info-circle" /><span>{copy.attribution}</span>{onOpenCleanup && cleanupTargetsForTrackingRoots(session.activeTrackingRoots).length > 0 && <button className="gh xs" onClick={() => openCleanup(session.activeTrackingRoots)}><i className="ti ti-eraser" /> {copy.cleanup}</button>}</div>
      </>}
    </section>}

    <section className="managed-session-history">
      <div className="agent-panel-head"><div><b>{copy.history}</b><span>{copy.historyDesc}</span></div><span className="cnt">{session.reports.length}</span></div>
      {session.reportsLoading ? <div className="managed-scope-loading"><i className="ti ti-loader spin" /> {copy.history}</div> : session.reports.length === 0
        ? <div className="managed-empty-history"><i className="ti ti-history" /><span>{copy.noHistory}</span></div>
        : <div className="managed-report-list">{session.reports.slice(0, 12).map((report) => <div className="managed-report-row" key={report.id}>
          <span className={`managed-report-status ${report.status}`}><i className={`ti ${report.status === "completed" ? "ti-check" : report.status === "failed" ? "ti-alert-triangle" : "ti-plug-connected-x"}`} /></span>
          <span className="managed-report-main"><b>{report.launch.agentName}</b><small title={report.launch.workspace}>{report.launch.workspace}</small></span>
          <span className="managed-report-impact"><b className={report.monitor.deltaBytes > 0 ? "negative" : "positive"}>{signedBytes(report.monitor.deltaBytes)}</b><small>{statusLabel(report.status)} · {elapsed(report.launch.startedAt, report.endedAt, en)}</small></span>
          <button className="gh xs" title={copy.view} onClick={() => setDetail(report)}><i className="ti ti-file-description" /> {copy.view}</button>
          <button className="space-icon-button danger" title={copy.remove} onClick={() => setRemoveReport(report)}><i className="ti ti-trash" /></button>
        </div>)}</div>}
    </section>

    {detail && <Modal wide title={copy.report} icon="ti-file-description" onClose={() => setDetail(null)} footer={<>
      {onOpenCleanup && cleanupTargetsForTrackingRoots(detail.trackingRoots).length > 0 && <button className="gh sm" onClick={() => openCleanup(detail.trackingRoots)}><i className="ti ti-eraser" /> {copy.cleanup}</button>}
      <button className="pr sm" onClick={() => setDetail(null)}>{copy.close}</button>
    </>}>
      <div className="managed-report-detail">
        <div className="managed-report-summary">
          <div><span>{copy.status}</span><b>{statusLabel(detail.status)}</b></div><div><span>{copy.duration}</span><b>{elapsed(detail.launch.startedAt, detail.endedAt, en)}</b></div>
          <div><span>{copy.diskImpact}</span><b>{signedBytes(detail.monitor.deltaBytes)}</b></div><div><span>{copy.files}</span><b>{detail.monitor.filesChanged.toLocaleString()}</b></div>
        </div>
        <div className="managed-report-meta"><span>{copy.ended}</span><b>{new Date(detail.endedAt).toLocaleString()}</b><span>{copy.project}</span><button title={detail.launch.workspace} onClick={() => void openWorkSessionPath(detail.launch.workspace)}>{detail.launch.workspace}</button></div>
        <ReportSection title={copy.tracked} rows={detail.trackingRoots.map((root) => ({ primary: trackingRootText(root, en).label, secondary: root.path, badge: safetyLabel(root) }))} />
        <ReportSection title={detail.launch.mode === "desktop" ? copy.desktopMode : copy.environment} rows={detail.launch.mode === "desktop" ? [{ primary: detail.launch.targetName || detail.launch.agentName, secondary: detail.launch.process ? `${detail.launch.process.processName} · PID ${detail.launch.process.pid}` : detail.launch.commandPath }] : (detail.launch.environment || []).map((item) => ({ primary: item.label, secondary: `${item.version || copy.unavailable} · ${item.path || copy.unavailable}` }))} />
        <ReportSection title={copy.processes} rows={detail.monitor.runningAgents.map((process) => ({ primary: `${process.agent} · PID ${process.pid}`, secondary: process.processName }))} />
        <ReportSection title={copy.directories} rows={detail.monitor.directories.map((directory) => ({ primary: directory.path, secondary: `${signedBytes(directory.deltaBytes)} · ${directory.filesChanged} ${copy.files}` }))} />
      </div>
    </Modal>}
    {removeReport && <ConfirmModal title={copy.deleteTitle} icon="ti-trash" danger message={copy.deleteMessage} confirmLabel={copy.remove} onClose={() => setRemoveReport(null)} onConfirm={() => void deleteWorkSessionReport(removeReport.id).then(() => setRemoveReport(null)).catch((error) => toast(String(error), "err"))} />}
  </div>;
}

function ReportSection({ title, rows }: { title: string; rows: { primary: string; secondary: string; badge?: string }[] }) {
  if (!rows.length) return null;
  return <section className="managed-report-section"><b>{title}</b>{rows.map((row, index) => <div key={`${row.primary}:${index}`} title={row.secondary}><span><strong>{row.primary}</strong><small>{row.secondary}</small></span>{row.badge && <em>{row.badge}</em>}</div>)}</section>;
}
