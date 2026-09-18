import type { MessageKey } from "./i18n";
import type { Page } from "./pageState";

export type NavItem = { id: Page; icon: string; labelKey: MessageKey };
export type NavSection = { labelKey: MessageKey | null; icon?: string; items: NavItem[] };

// Section titles are five Chinese characters and menu items four; product names stay as-is.
export const NAV_SECTIONS: NavSection[] = [
  { labelKey: null, items: [{ id: "overview", icon: "ti-layout-dashboard", labelKey: "nav.overview" }] },
  {
    labelKey: "nav.section.agents",
    icon: "ti-robot",
    items: [
      { id: "agents", icon: "ti-sparkles", labelKey: "nav.agents" },
      { id: "agent-data", icon: "ti-database", labelKey: "nav.agentData" },
    ],
  },
  {
    labelKey: "nav.section.devEnv",
    icon: "ti-stack-2",
    items: [
      { id: "git", icon: "ti-brand-git", labelKey: "nav.git" },
      { id: "python", icon: "ti-brand-python", labelKey: "nav.python" },
      { id: "php", icon: "ti-brand-php", labelKey: "nav.php" },
      { id: "node", icon: "ti-brand-nodejs", labelKey: "nav.node" },
      { id: "java", icon: "ti-coffee", labelKey: "nav.java" },
      { id: "maven", icon: "ti-feather", labelKey: "nav.maven" },
      { id: "gradle", icon: "ti-box", labelKey: "nav.gradle" },
      { id: "go", icon: "ti-brand-golang", labelKey: "nav.go" },
      { id: "rust", icon: "ti-brand-rust", labelKey: "nav.rust" },
    ],
  },
  {
    labelKey: "nav.section.system",
    icon: "ti-server-cog",
    items: [
      { id: "proxy", icon: "ti-world-bolt", labelKey: "nav.proxy" },
      { id: "cleanup", icon: "ti-eraser", labelKey: "nav.cleanup" },
    ],
  },
];

export const NAV_FOOT: NavItem[] = [
  { id: "history", icon: "ti-history", labelKey: "nav.history" },
  { id: "settings", icon: "ti-settings", labelKey: "nav.settings" },
];

export const ALL_NAV_ITEMS: NavItem[] = [...NAV_SECTIONS.flatMap((section) => section.items), ...NAV_FOOT];

type SectionStorage = Pick<Storage, "getItem" | "setItem">;
const COLLAPSED_KEY = "stackerLocal.navCollapsed.v1";

function browserStorage(): SectionStorage | null {
  return typeof window === "undefined" ? null : window.localStorage;
}

/** Section label keys the user collapsed; persisted locally, never required. */
export function readCollapsedSections(storage: SectionStorage | null = browserStorage()): string[] {
  try {
    const value: unknown = JSON.parse(storage?.getItem(COLLAPSED_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
}

export function toggleSection(collapsed: string[], key: string, storage: SectionStorage | null = browserStorage()): string[] {
  const next = collapsed.includes(key) ? collapsed.filter((item) => item !== key) : [...collapsed, key];
  try {
    storage?.setItem(COLLAPSED_KEY, JSON.stringify(next));
  } catch {
    // Collapse state is a convenience; navigation works without it.
  }
  return next;
}

export function sectionKeyOf(page: Page): string | null {
  return NAV_SECTIONS.find((section) => section.items.some((item) => item.id === page))?.labelKey ?? null;
}
