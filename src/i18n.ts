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
  "生态环境体检": "Environment Check",
  "AI工作智能体": "AI Work Agents",
  "工作智能体生态": "Work Agent Ecosystem",
  "编程生态": "Development Ecosystems",
  "终端代理": "Terminal Proxy",
  "磁盘清理": "Disk Cleanup",
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
