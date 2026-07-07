export type Locale = "zh-CN" | "en-US";

export const DEFAULT_LOCALE: Locale = "zh-CN";
export const SUPPORTED_LOCALES: Locale[] = ["zh-CN", "en-US"];

const STORAGE_KEY = "stacker.locale";

const messages = {
  "zh-CN": {
    "nav.overview": "概览",
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
    "state.comingSoonDesc": "该功能模块尚未启用，后续版本会继续补充。",
  },
  "en-US": {
    "nav.overview": "Overview",
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
    "state.comingSoon": "This page is being prepared",
    "state.comingSoonDesc": "This module is not enabled yet and will be expanded in a future version.",
  },
} as const;

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

export function setLocale(locale: Locale) {
  if (typeof window !== "undefined") window.localStorage.setItem(STORAGE_KEY, locale);
}

export function t(key: MessageKey, locale = getLocale()): string {
  return messages[locale]?.[key] ?? messages[DEFAULT_LOCALE][key] ?? key;
}
