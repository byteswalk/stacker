import type { AgentName, ClientTag, SessionStatus } from "./types";

export const AGENT_LABEL: Record<AgentName, string> = { codex: "Codex", claude: "Claude" };

export const CLIENT_LABEL: Record<ClientTag, string> = {
  desktop: "桌面端",
  terminal: "终端",
  ide: "IDE",
  automation: "自动化",
  sdk: "SDK",
  unknown: "未知来源",
};

export const STATUS_LABEL: Record<SessionStatus, string> = {
  // Not archived; terminal sessions have no archive, so they are never "in progress" as such.
  active: "未归档",
  archived: "已归档",
  orphaned: "孤儿",
};

export function toggleSelection(current: string[], id: string): string[] {
  return current.includes(id) ? current.filter((item) => item !== id) : [...current, id];
}

/** Keeps only selections still present in the current result. */
export function currentSelection(selected: string[], ids: string[]): string[] {
  const present = new Set(ids);
  return selected.filter((id) => present.has(id));
}

export function formatAge(secs: number, nowSecs: number): string {
  const diff = Math.max(0, nowSecs - secs);
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  return `${Math.floor(diff / 86400)} 天前`;
}
