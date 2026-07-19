import { createContext, createElement, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { GENERATED_EN } from "./en.generated";
import { invoke } from "./invoke";

export type Locale = "zh-CN" | "en-US";

export const DEFAULT_LOCALE: Locale = "zh-CN";
export const SUPPORTED_LOCALES: Locale[] = ["zh-CN", "en-US"];

const STORAGE_KEY = "stacker.locale";
const CHINESE_TEXT = /[\u3400-\u9fff]/;

const messages = {
  "zh-CN": {
    "nav.overview": "生态环境体检",
    "nav.vibe": "AI工作智能体",
    "nav.git": "Git",
    "nav.python": "Python",
    "nav.node": "Node",
    "nav.java": "Java",
    "nav.maven": "Maven",
    "nav.gradle": "Gradle",
    "nav.go": "Go",
    "nav.rust": "Rust",
    "nav.proxy": "终端代理",
    "nav.cleanup": "磁盘清理",
    "nav.history": "历史",
    "nav.settings": "设置",
    "state.comingSoon": "此页正在完善中",
    "state.comingSoonDesc": "当前版本尚未开放此功能。",
  },
  "en-US": {
    "nav.overview": "Environment Check",
    "nav.vibe": "AI Work Agents",
    "nav.git": "Git",
    "nav.python": "Python",
    "nav.node": "Node",
    "nav.java": "Java",
    "nav.maven": "Maven",
    "nav.gradle": "Gradle",
    "nav.go": "Go",
    "nav.rust": "Rust",
    "nav.proxy": "Terminal Proxy",
    "nav.cleanup": "Disk Cleanup",
    "nav.history": "History",
    "nav.settings": "Settings",
    "state.comingSoon": "Coming soon",
    "state.comingSoonDesc": "This feature is not available in the current version.",
  },
} as const;

/**
 * Product terminology takes precedence over generated translations. Chinese is
 * the source language so existing feature code can be internationalized without
 * coupling business logic to translation keys.
 */
const CURATED_EN: Record<string, string> = {
  "空间分析": "Space Analysis",
  "大文件阈值": "Large-file Threshold",
  "大文件阈值（GB）": "Large-file Threshold (GB)",
  "记住上次扫描目标": "Remember Last Scan Targets",
  "设置大文件阈值；记住的目标仅用于下次选择，不会自动扫描。": "Set the large-file threshold. Remembered targets only repopulate the next selector and never start a scan.",
  "大文件列表默认显示达到此阈值的文件；扫描目标只会在手动开始扫描后保存。": "The large-files list shows files at or above this threshold. Targets are saved only after you manually start a scan.",
  "空间分析设置已保存": "Space Analysis settings saved",
  "已关闭并清除记住的扫描目标": "Target memory disabled and remembered scan targets cleared",
  "保存空间分析设置失败：": "Failed to save Space Analysis settings: ",
  "设置已保存，但无法清除记住的扫描目标。请检查系统存储权限后重试。": "The setting was saved, but remembered scan targets could not be cleared. Check system storage permissions and try again.",
  "快速扫描": "Quick Scan",
  "选择目录": "Choose Folder",
  "选择磁盘": "Choose Disk",
  "全盘分析": "All-disk Analysis",
  "选择扫描范围": "Choose Scan Scope",
  "开始空间分析": "Start Space Analysis",
  "选择快速检查、目录或本地固定磁盘。扫描仅在手动确认后开始。": "Choose a quick check, folder, or local fixed disk. Scans start only after manual confirmation.",
  "选择要分析的目录": "Choose a Folder to Analyze",
  "选择分析目录": "Choose Analysis Folders",
  "可连续添加多个目录；存在包含关系时仅保留上层目录，避免重复统计。": "Add multiple folders one at a time. When folders overlap, only the parent is kept to prevent duplicate counting.",
  "添加目录": "Add Folder",
  "已选择 {count} 个目录": "{count} folders selected",
  "尚未选择目录，请点击“添加目录”。": "No folders selected. Click Add Folder to continue.",
  "移除目录": "Remove Folder",
  "使用管理员权限扫描": "Scan as Administrator",
  "适用于包含受保护目录的范围；开始时将显示 Windows 用户账户控制提示。": "Use for scopes containing protected folders. Windows User Account Control will ask for approval when the scan starts.",
  "适用于需要统计系统受保护目录的磁盘；开始时将显示 Windows 用户账户控制提示。": "Use when protected system folders must be included. Windows User Account Control will ask for approval when the scan starts.",
  "已取消管理员授权，扫描未开始。": "Administrator approval was cancelled. The scan did not start.",
  "扫描常见开发缓存、历史版本和 Windows 临时目录，不会遍历整个磁盘。": "Scan common development caches, previous versions, and Windows temporary folders without traversing entire disks.",
  "选择一个或多个目录进行深入分析，可直接选择磁盘根目录。": "Choose one or more folders for a deep analysis. Disk roots can be selected directly.",
  "从本机固定磁盘列表中选择一个或多个磁盘进行完整分析。": "Choose one or more local fixed disks for a complete analysis.",
  "无法打开目录选择器，请重试。": "Unable to open the folder picker. Please retry.",
  "扫描已开始，但无法保存扫描目标。": "The scan started, but its targets could not be remembered.",
  "无法读取空间分析设置。扫描入口已保持禁用，请重试。": "Unable to load Space Analysis settings. Scan launchers remain disabled; please retry.",
  "全盘分析不会预选磁盘。请选择一个或多个本地固定磁盘。": "All-disk Analysis does not preselect disks. Choose one or more local fixed disks.",
  "可恢复上次选择，但扫描不会自动开始。仅显示本地固定磁盘。": "Your previous selection may be restored, but the scan never starts automatically. Only local fixed disks are shown.",
  "正在启动…": "Starting...",
  "开始分析": "Start Analysis",
  "正在读取本地磁盘…": "Reading local disks...",
  "无法读取本地磁盘，请关闭后重试。": "Unable to read local disks. Close this dialog and retry.",
  "未发现可分析的本地固定磁盘。": "No local fixed disks are available for analysis.",
  "上次选择 · 当前不是可用的本地固定磁盘": "Previous selection · Not currently available as a local fixed disk",
  "不可用": "Unavailable",
  "未知文件系统": "Unknown file system",
  "已用": "Used",
  "可用": "Free",
  "可移动磁盘、光驱和网络磁盘不会出现在此列表中。": "Removable disks, optical drives, and network disks are excluded from this list.",
  "尚未选择扫描目标": "No scan target selected",
  "快速扫描 · 已知开发工具缓存与临时目录": "Quick Scan · Known development caches and temporary folders",
  "目录": "Folder",
  "磁盘": "Disk",
  "正在分析所选范围": "Analyzing Selected Scope",
  "正在启动扫描": "Starting Scan",
  "正在等待后台接受扫描任务，切换页面不会重复启动。": "Waiting for the backend to accept the scan. Navigating away will not start it again.",
  "正在启动空间分析": "Starting Space Analysis",
  "正在等待后台接受扫描任务。切换页面不会重复启动。": "Waiting for the backend to accept the scan. Navigating away will not start it again.",
  "正在统计文件的实际分配空间。切换页面不会中断扫描。": "Measuring allocated file space. The scan continues when you navigate away.",
  "空间分析完成": "Space Analysis Complete",
  "所选范围已完成统计，可继续选择其他扫描范围。": "The selected scope has been measured. You can choose another scope to scan.",
  "页面仍可继续使用，请重新选择扫描范围。": "The page remains available. Choose a scan scope and try again.",
  "请选择扫描范围后手动开始分析。": "Choose a scan scope, then start the analysis manually.",
  "等待扫描进度…": "Waiting for scan progress...",
  "等待手动开始扫描": "Waiting for a manual scan",
  "已统计分配空间": "Allocated Space Accounted",
  "已跳过": "Skipped",
  "包括无权访问、扫描期间消失、无效或无法读取的路径；这些路径未计入占用统计。": "Includes paths that were inaccessible, disappeared during the scan, were invalid, or could not be read. They are excluded from usage totals.",
  "取消扫描": "Cancel Scan",
  "正在取消…": "Cancelling...",
  "重试": "Retry",
  "正在快速扫描": "Quick Scan in Progress",
  "正在取消扫描": "Cancelling Scan",
  "扫描完成": "Scan Complete",
  "扫描失败": "Scan Failed",
  "扫描已取消，结果不完整": "Scan cancelled. Results are incomplete.",
  "快速扫描会统计已知开发工具缓存、历史版本和 Windows 临时目录。": "Quick Scan measures known development caches, previous versions, and Windows temporary folders.",
  "正在统计已知可清理项，切换页面不会中断扫描。": "Measuring known cleanup items. The scan continues when you navigate away.",
  "正在停止后台任务，已统计的进度会保留。": "Stopping the background task. Measured progress will be retained.",
  "默认勾选可安全清理项；其他项目需要手动确认。": "Safe cleanup items are selected by default. Other items require manual confirmation.",
  "已统计的进度已保留，不完整结果不能用于快照比较。": "Measured progress has been retained. Incomplete results cannot be compared with snapshots.",
  "页面仍可继续使用，请重试扫描。": "The page remains available. Retry the scan when ready.",
  "扫描任务未完成，详细错误已记录。": "The scan did not complete. Error details were recorded.",
  "已扫描文件": "Files Scanned",
  "已扫描目录": "Folders Scanned",
  "已统计空间": "Space Measured",
  "无法访问": "Inaccessible",
  "耗时": "Elapsed",
  "等待扫描任务…": "Waiting for scan progress...",
  "任务 ID": "Task ID",
  "无法启动扫描，请重试。": "Unable to start the scan. Please retry.",
  "无法取消扫描，请重试。": "Unable to cancel the scan. Please retry.",
  "清理已完成，但无法启动重新扫描。": "Cleanup completed, but the follow-up scan could not be started.",
  "清理失败，请重试。": "Cleanup failed. Please retry.",
  "统计失败，请重试。": "Unable to calculate statistics. Please retry.",
  "Gradle 缓存": "Gradle Cache",
  "Go 模块缓存": "Go Module Cache",
  "pnpm 存储": "pnpm Store",
  "npm 缓存": "npm Cache",
  "Cargo registry 缓存": "Cargo Registry Cache",
  "pip 缓存": "pip Cache",
  "Electron 下载缓存": "Electron Download Cache",
  "Playwright 浏览器": "Playwright Browsers",
  "Hugging Face 模型缓存": "Hugging Face Model Cache",
  "Maven 本地仓库": "Maven Local Repository",
  "JetBrains 历史版本": "Previous JetBrains Version",
  "Windows 临时目录": "Windows Temporary Folder",
  "用户临时目录": "User Temporary Folder",
  "已知空间项目": "Known Space Item",
  "可安全清理": "Safe",
  "可重新生成": "Rebuildable",
  "需要确认": "Confirmation Required",
  "仅供查看": "View Only",
  "历史版本": "Previous Version",
  "临时文件": "Temporary Files",
  "谨慎": "Use Caution",
  "选择清理项": "Select cleanup item",
  "智能清理": "Clean by Age",
  "清理": "Clean",
  "清理所选": "Clean Selected",
  "已释放": "Recovered",
  "可清理项共占用": "Cleanup items use",
  "可安全释放": "safe to remove",
  "后台估算可安全清理约": "Background estimate of safely removable data:",
  "开始扫描后可查看完整清理项。": "Start a scan to review all cleanup items.",
  "未发现可清理项": "No Cleanup Items Found",
  "当前扫描范围内没有达到显示条件的缓存、历史版本或临时文件。": "No caches, previous versions, or temporary files in this scan meet the display threshold.",
  "可安全清理（纯缓存，删除后会自动重新获取）": "Safe cleanup (cache data that can be downloaded again)",
  "JetBrains IDE 历史版本（保留同产品最新版本）": "Previous JetBrains IDE versions (the latest version of each product is retained)",
  "Windows 临时目录（超过 1 GB 才显示）": "Windows temporary folders (shown above 1 GB)",
  "谨慎清理（重新下载可能耗时较长）": "Use caution (downloading this data again may take time)",
  "确认清理": "Confirm Cleanup",
  "将清理": "Items to clean:",
  "项": "items",
  "预计释放": "Estimated recovery",
  "缓存和临时目录会清理目录内容；JetBrains 历史版本会删除旧版本目录；被系统占用的临时文件会自动跳过。": "Cache and temporary folder contents will be removed. Previous JetBrains version folders will be deleted. Temporary files in use by the system will be skipped.",
  "清理中…": "Cleaning...",
  "智能清理（按未访问时长）": "Clean by Time Since Last Access",
  "清理超过以下天数未访问的文件": "Clean files not accessed for more than",
  "1 年": "1 year",
  "天": "days",
  "统计中…": "Calculating...",
  "未访问的文件共": "Files not accessed:",
  "个": "items",
  "约": "about",
  "删除后若再次使用会自动重新获取。": "Deleted data will be downloaded again when needed.",
  "生态环境体检": "Environment Check",
  "AI工作智能体": "AI Work Agents",
  "工作智能体生态": "Work Agent Ecosystem",
  "编程生态": "Development Ecosystems",
  "终端代理": "Terminal Proxy",
  "磁盘清理": "Disk Cleanup",
  "请在“开发产物”或“缓存与下载”页面选择需要清理的项目": "Select cleanup items on the Development Artifacts or Caches & Downloads tab.",
  "筛选路径或目录名": "Filter by path or directory name",
  "清除筛选": "Clear filter",
  "批量操作仅作用于当前筛选结果。": "Bulk actions apply only to the current filtered results.",
  "未找到匹配的可清理项。": "No matching cleanup items were found.",
  "历史": "History",
  "设置": "Settings",
  "源管理": "Source Management",
  "源目录": "Source Catalog",
  "源清单": "Source Catalog",
  "下载源": "Download Source",
  "仓库镜像": "Repository Mirror",
  "包源 / 镜像": "Package Sources / Mirrors",
  "大文件下载镜像": "Large-file Download Mirrors",
  "当前环境": "Current Environment",
  "环境源": "Active Source",
  "环境摘要": "Environment Summary",
  "复制摘要给 AI": "Copy Summary for AI",
  "复制摘要给AI": "Copy Summary for AI",
  "状态刷新": "Refresh Status",
  "刷新状态": "Refresh Status",
  "再次体检": "Run Check Again",
  "开始体检": "Start Check",
  "待体检": "Pending",
  "体检中…": "Checking...",
  "开发环境体检完成": "Environment check completed",
  "开发环境体检已完成": "Environment check completed",
  "开发环境体检任务异常结束：{error}": "Environment check failed: {error}",
  "开始扫描": "Start Scan",
  "重新扫描": "Scan Again",
  "智能优选源": "Optimize Sources",
  "运行时版本": "Runtime Versions",
  "管理工具更新": "Update Version Manager",
  "终端集成": "Terminal Integration",
  "临时生效": "Apply to Current Terminal",
  "安装新版本": "Install Version",
  "安装工具链": "Install Toolchain",
  "设为默认": "Set as Default",
  "重新应用": "Reapply",
  "清理残留": "Clean Up Leftovers",
  "排除工具自带": "Exclude Tool-bundled Runtimes",
  "扫描磁盘": "Scan Drives",
  "扫描目录": "Scan Folder",
  "选择文件": "Choose File",
  "选择": "Choose",
  "测速": "Test Speed",
  "应用": "Apply",
  "清除": "Clear",
  "保存": "Save",
  "关闭": "Close",
  "取消": "Cancel",
  "确认": "Confirm",
  "删除": "Delete",
  "卸载": "Uninstall",
  "安装": "Install",
  "更新": "Update",
  "刷新": "Refresh",
  "检查更新": "Check for Updates",
  "立即更新": "Update Now",
  "无需更新": "Up to Date",
  "当前无需更新": "Up to Date",
  "当前已是最新版本": "You are up to date",
  "发现新版本": "Update Available",
  "已安装": "Installed",
  "未安装": "Not Installed",
  "已配置": "Configured",
  "未配置": "Not Configured",
  "已生效": "Active",
  "生效中": "Active",
  "正常": "Healthy",
  "需处理": "Action Required",
  "建议": "Recommended",
  "提示": "Notice",
  "注意": "Attention",
  "详情": "Details",
  "官方": "Official",
  "官方源": "Official",
  "官方默认": "Official Default",
  "官方文档": "Official Documentation",
  "官方下载页": "Official Download Page",
  "阿里云": "Alibaba Cloud",
  "腾讯云": "Tencent Cloud",
  "华为云": "Huawei Cloud",
  "清华": "Tsinghua TUNA",
  "中科大": "USTC",
  "南京大学": "Nanjing University",
  "深色": "Dark",
  "浅色": "Light",
  "跟随系统": "Follow System",
  "外观": "Appearance",
  "语言": "Language",
  "界面语言": "Display Language",
  "简体中文": "Simplified Chinese",
  "通用与外观": "General & Appearance",
  "日志级别": "Log Level",
  "日志保留": "Log Retention",
  "实时日志": "Live Logs",
  "打开日志目录": "Open Log Folder",
  "提示管理": "Notifications",
  "后台检查提示": "Background Checks",
  "检查周期": "Check Interval",
  "程序更新": "Application Updates",
  "失效环境": "Invalid Environments",
  "磁盘清理提醒": "Disk Cleanup Reminder",
  "关于": "About",
  "账号执行环境": "Account Environments",
  "提交身份": "Commit Identity",
  "访问令牌（token）": "Access Token",
  "令牌（token）已验证": "Token Verified",
  "迁移仓库": "Migrate Repository",
  "初始化工程": "Initialize Project",
  "设置全局": "Set as Global",
  "打开终端": "Open Terminal",
  "打开桌面端": "Open Desktop App",
  "系统级": "System",
  "用户级": "Current User",
  "当前用户": "Current User",
  "仅当前用户": "Current User Only",
  "需要管理员权限": "Administrator Access Required",
  "需 UAC": "UAC Required",
  "新终端生效": "Takes Effect in New Terminals",
  "操作未完成": "Operation Not Completed",
  "操作失败：": "Operation failed: ",
  "安装失败：": "Installation failed: ",
  "更新失败：": "Update failed: ",
  "读取失败：": "Failed to load: ",
  "正在处理…": "Processing...",
  "检测中…": "Checking...",
  "检查中…": "Checking...",
  "测速中…": "Testing...",
  "保存中…": "Saving...",
  "安装中…": "Installing...",
  "更新中…": "Updating...",
  "删除中…": "Deleting...",
  "卸载中…": "Uninstalling...",
  "分类维护 · 测速 · 导入导出": "Categories · Speed Tests · Import / Export",
  "集中管理运行时下载源、包仓库源、大文件下载镜像和本地自定义源。具体应用到工具配置仍在各生态页面完成。": "Manage runtime downloads, package repositories, large-file mirrors, and custom sources in one place. Apply selections from the corresponding ecosystem page.",
  "管理下载源、仓库源和大文件镜像；应用配置仍在各生态页面完成。": "Manage download sources, repository mirrors, and large-file mirrors. Apply selections from the corresponding ecosystem page.",
  "服务器清单用于更新内置源，拉取后会以服务器清单为准全量替换；本地自定义源由当前电脑维护，不会被服务器清单覆盖。": "The remote manifest replaces the built-in source catalog when refreshed. Custom sources remain local and are never overwritten.",
  "全局代理地址": "Global Proxy Address",
  "终端代理和构建工具代理使用此地址。": "Used by terminal and build-tool proxy settings.",
  "最小化到托盘": "Minimize to Tray",
  "关闭窗口时隐藏到系统托盘。": "Keep Stacker running in the system tray when the window is closed.",
  "开机自启": "Launch at Startup",
  "登录 Windows 后自动启动 Stacker。": "Start Stacker automatically after signing in to Windows.",
  "级别切换立即生效；DEBUG 用于问题排查，日志按天归档。": "Changes take effect immediately. Use DEBUG for troubleshooting; logs are archived daily.",
  "自动清理超过保留期限的日志；清理日志会保留今天的记录。": "Automatically remove expired logs. Manual cleanup always keeps today's log.",
  "清理日志": "Clear Logs",
  "启动后和固定周期检查程序更新、源清单、生态版本、失效环境和清理阈值；仅显示红点，不自动弹窗。": "Check for application, source catalog, ecosystem, and environment changes at startup and on a schedule. Notifications appear as badges without interrupting your work.",
  "默认 30 分钟；周期越短，网络请求越频繁。": "Default: 30 minutes. Shorter intervals generate more network requests.",
  "可按使用习惯关闭程序更新、源清单、生态版本和失效环境提示。": "Choose which update and environment checks run in the background.",
  "生态版本": "Ecosystem Updates",
  "可安全清理项超过阈值时，在「磁盘清理」菜单显示红点。": "Show a Disk Cleanup badge when safely removable data exceeds this threshold.",
  "当前后台估算：": "Current estimate: ",
  "Windows 开发环境与工作智能体工具管理器": "Windows development environment and AI work agent manager",
  "Windows 开发工作站管理器": "Windows Developer Workstation Manager",
  "Windows 开发工作站管理器：统一管理运行时、AI 工作智能体、Git 账号、网络源与开发磁盘空间。": "A Windows developer workstation manager for runtimes, AI work agents, Git identities, network sources, and developer disk space.",
  "开源 · 无遥测": "Open source · No telemetry",
  "待体检项目": "Checks to Run",
  "核心运行时": "Core Runtimes",
  "检测 Git、Node.js、Python、Java、Go、Rust 等命令是否可用。": "Verify that Git, Node.js, Python, Java, Go, Rust, and other core commands are available.",
  "包管理器与构建工具": "Package Managers & Build Tools",
  "检测 npm、pip、Maven、Gradle、Cargo 等工具链状态。": "Check npm, pip, Maven, Gradle, Cargo, and related toolchains.",
  "配置、代理与缓存": "Configuration, Proxy & Cache",
  "检测终端集成、镜像源配置、代理环境变量和开发缓存占用。": "Review terminal integration, mirrors, proxy environment variables, and development caches.",
  "生态环境体检 · 未开始": "Environment Check · Not Started",
  "点击「开始体检」后，Stacker 将检测运行时、包管理器、构建工具、代理与缓存状态。": "Select Start Check to inspect runtimes, package managers, build tools, proxy settings, and caches.",
  "仅显示红点，不自动弹窗。": "Shows badges only and never opens pop-ups automatically.",
  "无法读取本次空间分析结果，请重新扫描。": "Unable to read this analysis result. Run the scan again.",
  "正在读取分析结果…": "Loading analysis results...",
  "本次空间分析结果不可用，请重新扫描。": "This analysis result is unavailable. Run the scan again.",
  "空间概览": "Space Overview",
  "目录排行": "Directory Ranking",
  "大文件": "Large Files",
  "空间分析视图": "Space analysis views",
  "实际占用": "Allocated Size",
  "逻辑大小": "Logical Size",
  "可用空间": "Free Space",
  "目录扫描或磁盘信息已变化，无法可靠计算可用空间。": "Free space is unavailable because this is a folder scan or the disk information has changed.",
  "已跳过路径": "Skipped Paths",
  "扫描范围占用": "Scanned Space Usage",
  "矩形面积按实际磁盘占用计算。": "Rectangle area represents allocated disk space.",
  "矩形面积按实际磁盘占用计算；单击下钻目录，右键可打开目录。": "Rectangle area represents allocated disk space. Click to drill down; right-click for directory actions.",
  "当前目录层级": "Current directory level",
  "返回扫描范围": "Return to scan scope",
  "扫描范围": "Scan Scope",
  "该目录没有可继续查看的子目录。": "This directory has no child directories to explore.",
  "个目录": "directories",
  "个文件": "files",
  "当前扫描结果没有可显示的占用数据。": "The current scan has no space-usage data to display.",
  "无法读取子目录，请重试。": "Unable to read the child directories. Try again.",
  "收起目录": "Collapse directory",
  "展开目录": "Expand directory",
  "打开目录": "Open directory",
  "复制路径": "Copy path",
  "路径已复制": "Path copied",
  "复制路径失败，请重试。": "Unable to copy the path. Try again.",
  "正在读取子目录…": "Loading child directories...",
  "加载更多": "Load More",
  "按实际磁盘占用排序；展开时才读取下一层目录。": "Sorted by allocated disk space. Child directories load only when expanded.",
  "每页最多 100 项": "Up to 100 items per page",
  "当前扫描结果没有目录数据。": "The current scan has no directory data.",
  "无法读取大文件列表，请重试。": "Unable to read the large-files list. Try again.",
  "无法读取更多大文件，请重试。": "Unable to load more large files. Try again.",
  "无法打开文件所在目录，请确认路径仍然存在。": "Unable to open the containing directory. Confirm that the path still exists.",
  "当前阈值": "Current threshold",
  "按实际磁盘占用排序，仅展示达到设置阈值的文件。": "Sorted by allocated disk space. Only files meeting the configured threshold are shown.",
  "正在读取大文件…": "Loading large files...",
  "没有达到当前阈值的大文件。": "No files meet the current threshold.",
  "仅查看": "Read-only",
  "未知时间": "Unknown time",
  "打开所在目录": "Open containing directory",
  "正在加载…": "Loading...",
  "无法打开目录，请确认路径仍然存在。": "Unable to open the directory. Confirm that the path still exists.",
  "开发产物": "Development Artifacts",
  "缓存与下载": "Caches & Downloads",
  "全选": "Select All",
  "取消全选": "Clear Selection",
  "已选择 {count} 个磁盘": "{count} drives selected",
  "全部取消": "Clear All",
  "选择本分类中的全部可清理项": "Select every cleanable item in this category",
  "取消本分类中的全部选择": "Clear all selections in this category",
  "空间变化": "Space Changes",
  "已选择": "Selected",
  "核对所选项目后进入清理确认": "Review the selected items before cleanup",
  "无法准备清理：": "Unable to prepare cleanup: ",
  "正在准备…": "Preparing...",
  "空间快照已关闭，或本次扫描尚未生成快照。": "Snapshots are disabled, or this scan has not produced a snapshot yet.",
  "正在读取可清理项…": "Loading cleanup candidates...",
  "无法读取可清理项，请重新扫描。": "Unable to load cleanup candidates. Run the scan again.",
  "当前扫描结果没有此类可清理项。": "No cleanup candidates of this type were found.",
  "仅列出已识别项目中可重新生成的依赖、构建目录和发布产物。": "Only rebuildable dependencies, build directories, and release artifacts from recognized projects are shown.",
  "清理后可能需要重新下载依赖；默认不勾选。": "Dependencies may need to be downloaded again after cleanup. These items are not selected by default.",
  "开始清理": "Start Cleanup",
  "清理失败：": "Cleanup failed: ",
  "部分项目需要管理员权限": "Some items require administrator access",
  "已处理": "Processed",
  "我已确认所选目录和影响，允许执行清理。": "I have reviewed the selected paths and their impact and approve this cleanup.",
  "清理结果": "Cleanup Results",
  "复查受影响目录": "Rescan Affected Directories",
  "实际释放": "Recovered",
  "状态": "Status",
  "安全清理": "Safe to Clean",
  "Node.js 依赖目录，清理后需要重新安装依赖": "Node.js dependencies; reinstall dependencies after cleanup",
  "Rust 构建产物，清理后首次构建会重新编译": "Rust build output; the next build will recompile it",
  "Maven 构建产物，清理后首次构建会重新生成": "Maven build output; the next build will regenerate it",
  "Gradle 项目缓存，清理后会重新下载或生成": "Gradle project cache; dependencies or metadata will be regenerated",
  "Gradle 构建产物，清理后首次构建会重新生成": "Gradle build output; the next build will regenerate it",
  "Go 发布产物，清理后需要重新构建": "Go release output; rebuild it after cleanup",
  "可重新生成的开发文件": "Rebuildable development files",
  "正在读取空间变化…": "Loading space changes...",
  "无法读取空间变化记录。": "Unable to load space-change history.",
  "这是当前目标的首份快照。完成下一次扫描后即可查看空间变化。": "This is the first snapshot for these targets. Complete another scan to compare changes.",
  "总占用变化": "Total Usage Change",
  "对比时间": "Compared Scans",
  "两次扫描之间没有目录占用变化。": "Directory usage did not change between these scans.",
  "常用目录": "Common Directories",
  "常用扫描目录": "Common Scan Directories",
  "保存常用目录后，可从磁盘清理页手动快速开始扫描。": "Save frequently scanned directories for one-click manual scans from Disk Cleanup.",
  "多个目录以分号分隔": "Separate multiple directories with semicolons",
  "空间快照": "Space Snapshots",
  "仅保存扫描目标指纹和相对目录占用，不保存文件名或文件内容。": "Only target fingerprints and relative directory usage are stored. File names and contents are never saved.",
  "用于比较同一组扫描目标在不同时间的空间变化。": "Compare space usage for the same scan targets over time.",
  "启用": "Enabled",
  "保留": "Keep",
  "每组最多": "Maximum per target set",
  "快照保留天数": "Snapshot retention in days",
  "每组目标最多快照数": "Maximum snapshots per target set",
  "份": "snapshots",
  "completed": "Completed",
  "cancelled": "Cancelled",
  "failed": "Failed",
  "spaceAnalysis.cleanup.reason.missing": "Path no longer exists",
  "spaceAnalysis.cleanup.reason.outsideRoot": "Path is outside the approved scan roots",
  "spaceAnalysis.cleanup.reason.linkDetected": "A link or reparse point was detected",
  "spaceAnalysis.cleanup.reason.identityChanged": "Path identity changed after the scan",
  "spaceAnalysis.cleanup.reason.classificationChanged": "Cleanup classification changed after the scan",
  "spaceAnalysis.cleanup.reason.accessDenied": "Access denied",
  "spaceAnalysis.cleanup.reason.deleteFailed": "Unable to delete the item",
  "spaceAnalysis.cleanup.reason.cancelled": "Cleanup was cancelled",
};

const EN = { ...GENERATED_EN, ...CURATED_EN };
const EN_PHRASES = Object.keys(EN)
  .filter((value) => CHINESE_TEXT.test(value))
  .sort((left, right) => right.length - left.length);

export type MessageKey = keyof typeof messages[typeof DEFAULT_LOCALE];

export function normalizeLocale(value?: string | null): Locale {
  if (!value) return DEFAULT_LOCALE;
  return value.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
}

export function getLocale(): Locale {
  if (typeof window === "undefined") return DEFAULT_LOCALE;
  const saved = window.localStorage.getItem(STORAGE_KEY);
  return normalizeLocale(saved || window.navigator.language);
}

function saveLocale(locale: Locale) {
  if (typeof window !== "undefined") window.localStorage.setItem(STORAGE_KEY, locale);
}

function polishEnglish(value: string) {
  return value
    .replace(/\bwarehouses\b/gi, "repositories")
    .replace(/\bwarehouse\b/gi, "repository")
    .replace(/mirror images?/gi, "mirrors")
    .replace(/image sources?/gi, "mirrors")
    .replace(/ecological environment physical examination/gi, "environment check")
    .replace(/ecological environment/gi, "development environment")
    .replace(/submission identity/gi, "commit identity")
    .replace(/the new terminal takes effect/gi, "takes effect in new terminals")
    .replace(/new terminal takes effect/gi, "takes effect in new terminals");
}

export function translateText(value: string, locale = getLocale()): string {
  if (locale === "zh-CN" || !value || !CHINESE_TEXT.test(value)) return value;
  const leading = value.match(/^\s*/)?.[0] ?? "";
  const trailing = value.match(/\s*$/)?.[0] ?? "";
  const core = value.slice(leading.length, value.length - trailing.length);
  const exact = EN[core];
  if (exact) return leading + polishEnglish(exact) + trailing;

  let translated = core;
  for (const source of EN_PHRASES) {
    if (translated.includes(source)) translated = translated.split(source).join(EN[source]);
  }
  return leading + polishEnglish(translated) + trailing;
}

export function t(key: MessageKey, locale = getLocale()): string {
  return messages[locale]?.[key] ?? messages[DEFAULT_LOCALE][key] ?? key;
}

type LocaleContextValue = {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: (key: MessageKey) => string;
  tr: (value: string) => string;
};

const LocaleContext = createContext<LocaleContextValue>({
  locale: DEFAULT_LOCALE,
  setLocale: () => undefined,
  t: (key) => messages[DEFAULT_LOCALE][key],
  tr: (value) => value,
});

type TextRecord = { source: string; rendered: string };
const textRecords = new WeakMap<Text, TextRecord>();
const attributeRecords = new WeakMap<Element, Map<string, TextRecord>>();
const TRANSLATED_ATTRIBUTES = ["title", "placeholder", "aria-label", "alt"] as const;

function translateTextNode(node: Text, locale: Locale) {
  const current = node.nodeValue ?? "";
  const existing = textRecords.get(node);
  const source = existing && current === existing.rendered ? existing.source : current;
  const rendered = translateText(source, locale);
  textRecords.set(node, { source, rendered });
  if (current !== rendered) node.nodeValue = rendered;
}

function translateElementAttributes(element: Element, locale: Locale) {
  const records = attributeRecords.get(element) ?? new Map<string, TextRecord>();
  for (const attribute of TRANSLATED_ATTRIBUTES) {
    if (!element.hasAttribute(attribute)) continue;
    const current = element.getAttribute(attribute) ?? "";
    const existing = records.get(attribute);
    const source = existing && current === existing.rendered ? existing.source : current;
    const rendered = translateText(source, locale);
    records.set(attribute, { source, rendered });
    if (current !== rendered) element.setAttribute(attribute, rendered);
  }
  attributeRecords.set(element, records);
}

function translateTree(root: Node, locale: Locale) {
  if (root.nodeType === Node.TEXT_NODE) {
    translateTextNode(root as Text, locale);
    return;
  }
  if (!(root instanceof Element) && !(root instanceof DocumentFragment)) return;
  if (root instanceof Element) translateElementAttributes(root, locale);
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
  let current = walker.nextNode();
  while (current) {
    if (current.nodeType === Node.TEXT_NODE) translateTextNode(current as Text, locale);
    else translateElementAttributes(current as Element, locale);
    current = walker.nextNode();
  }
}

function watchTranslatedUi(locale: Locale) {
  document.documentElement.lang = locale;
  if (document.body) translateTree(document.body, locale);
  const observer = new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      if (mutation.type === "characterData") translateTree(mutation.target, locale);
      else if (mutation.type === "attributes") translateElementAttributes(mutation.target as Element, locale);
      else mutation.addedNodes.forEach((node) => translateTree(node, locale));
    }
  });
  observer.observe(document.documentElement, {
    subtree: true,
    childList: true,
    characterData: true,
    attributes: true,
    attributeFilter: [...TRANSLATED_ATTRIBUTES],
  });
  return () => observer.disconnect();
}

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(getLocale);
  const changeLocale = useCallback((next: Locale) => {
    saveLocale(next);
    setLocaleState(next);
  }, []);

  useEffect(() => watchTranslatedUi(locale), [locale]);
  useEffect(() => {
    invoke("settings_set_locale", { locale }).catch(() => undefined);
  }, [locale]);

  const value = useMemo<LocaleContextValue>(() => ({
    locale,
    setLocale: changeLocale,
    t: (key) => t(key, locale),
    tr: (text) => translateText(text, locale),
  }), [changeLocale, locale]);

  return createElement(LocaleContext.Provider, { value }, children);
}

export function useI18n() {
  return useContext(LocaleContext);
}
