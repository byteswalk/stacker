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
      { id: "vault", icon: "ti-shield-lock", labelKey: "nav.vault" },
    ],
  },
];

export const NAV_FOOT: NavItem[] = [
  { id: "history", icon: "ti-history", labelKey: "nav.history" },
  { id: "settings", icon: "ti-settings", labelKey: "nav.settings" },
];

export const ALL_NAV_ITEMS: NavItem[] = [...NAV_SECTIONS.flatMap((section) => section.items), ...NAV_FOOT];

/** The section every launch opens with; the others start collapsed. */
export const OPEN_SECTION = "nav.section.agents";

/** Collapsed sections at launch: all but 智能体管理. Toggles last for the session only. */
export function initialCollapsedSections(): string[] {
  return openOnly(OPEN_SECTION);
}

/** The collapsed list that leaves only `key` open. */
export function openOnly(key: string): string[] {
  return NAV_SECTIONS.flatMap((section) => section.labelKey && section.labelKey !== key ? [section.labelKey] : []);
}

/** Sections open one at a time: opening one closes the others; closing it leaves all closed. */
export function toggleSection(collapsed: string[], key: string): string[] {
  return collapsed.includes(key) ? openOnly(key) : [...collapsed, key];
}

export function sectionKeyOf(page: Page): string | null {
  return NAV_SECTIONS.find((section) => section.items.some((item) => item.id === page))?.labelKey ?? null;
}
