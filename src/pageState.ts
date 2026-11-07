export const PAGE_IDS = [
  "overview",
  "agents",
  "agent-data",
  "git",
  "python",
  "php",
  "node",
  "java",
  "maven",
  "gradle",
  "go",
  "rust",
  "proxy",
  "cleanup",
  "vault",
  "history",
  "settings",
] as const;

export type Page = (typeof PAGE_IDS)[number];

export const DEFAULT_PAGE: Page = "overview";
