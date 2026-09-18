export const PAGE_IDS = [
  "overview",
  "vibe",
  "agent-space",
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
  "history",
  "settings",
] as const;

export type Page = (typeof PAGE_IDS)[number];

export const DEFAULT_PAGE: Page = "overview";

const LAST_PAGE_STORAGE_KEY = "stackerLocal.lastPage.v1";
const PAGE_ID_SET = new Set<string>(PAGE_IDS);

type PageStorage = Pick<Storage, "getItem" | "setItem">;

function browserStorage(): PageStorage | null {
  if (typeof window === "undefined") return null;
  return window.localStorage;
}

export function normalizePage(value: unknown): Page {
  return typeof value === "string" && PAGE_ID_SET.has(value)
    ? value as Page
    : DEFAULT_PAGE;
}

export function readLastPage(storage: PageStorage | null = browserStorage()): Page {
  if (!storage) return DEFAULT_PAGE;
  try {
    return normalizePage(storage.getItem(LAST_PAGE_STORAGE_KEY));
  } catch {
    return DEFAULT_PAGE;
  }
}

export function saveLastPage(
  page: Page,
  storage: PageStorage | null = browserStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(LAST_PAGE_STORAGE_KEY, page);
  } catch {
    // Navigation persistence is optional; the default page remains available.
  }
}
