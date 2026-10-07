import type { EntryView } from "./api";

export const EXPIRY_WARN_DAYS = 14;
export const MIN_PASSWORD_CHARS = 9;
const DAY_MS = 86_400_000;

export type ExpiryState = "none" | "ok" | "soon" | "expired";
export type ListFilter = {
  query: string; platform: string; soonOnly: boolean;
  /** "" any kind, or one kind. */
  kind?: "" | "other" | "ssh_key";
  /** "" anywhere, "browser" imported from a browser (tagged 浏览器), "own" everything else. */
  source?: "" | "browser" | "own";
  /** "" either way, "on" in Windows credentials, "off" not. */
  windows?: "" | "on" | "off";
};

export const BROWSER_TAG = "浏览器";

function startOfDay(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

export function daysUntil(expiresAt: string, today: Date): number {
  const [year, month, day] = expiresAt.split("-").map(Number);
  return Math.round((new Date(year, month - 1, day).getTime() - startOfDay(today)) / DAY_MS);
}

export function expiryState(expiresAt: string | null, today: Date): ExpiryState {
  if (!expiresAt) return "none";
  const days = daysUntil(expiresAt, today);
  if (days < 0) return "expired";
  return days <= EXPIRY_WARN_DAYS ? "soon" : "ok";
}

export function soonCount(entries: EntryView[], today: Date): number {
  return entries.filter((entry) => ["soon", "expired"].includes(expiryState(entry.expiresAt, today))).length;
}

function searchable(entry: EntryView): string {
  const visible = entry.fields.filter((field) => !field.secret && field.value).map((field) => field.value);
  return [entry.title, entry.platform, entry.note, ...entry.tags, ...visible].join("\n").toLowerCase();
}

export function filterEntries(entries: EntryView[], filter: ListFilter, today: Date): EntryView[] {
  const query = filter.query.trim().toLowerCase();
  return entries
    .filter((entry) => !query || searchable(entry).includes(query))
    .filter((entry) => !filter.platform || entry.platform === filter.platform)
    .filter((entry) => !filter.kind || (filter.kind === "ssh_key") === (entry.kind === "ssh_key"))
    .filter((entry) => !filter.source || (filter.source === "browser") === entry.tags.includes(BROWSER_TAG))
    .filter((entry) => !filter.windows || (filter.windows === "on") === entry.windows)
    .filter((entry) => !filter.soonOnly || ["soon", "expired"].includes(expiryState(entry.expiresAt, today)))
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

/** Second-level names that belong to a country, where the site is one label further in. */
const COUNTRY_SECOND = new Set(["com.cn", "net.cn", "org.cn", "gov.cn", "edu.cn", "co.uk", "org.uk", "co.jp", "com.hk", "com.tw", "com.au", "co.kr", "com.sg"]);

/** The site an entry belongs to, for grouping: its registrable domain, an IP as it is, or else its platform or title. */
export function siteKey(entry: EntryView): string {
  const url = entry.fields.find((field) => field.name === "网址")?.value ?? "";
  let fromUrl = "";
  try { fromUrl = url ? new URL(url).hostname : ""; } catch { /* not a URL: the platform or title stands in */ }
  const host = (fromUrl || entry.platform || entry.title).trim().toLowerCase().replace(/:\d+$/, "").replace(/^www\./, "");
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(host) || !host.includes(".")) return host;
  const labels = host.split(".");
  const two = labels.slice(-2).join(".");
  return COUNTRY_SECOND.has(two) ? labels.slice(-3).join(".") : two;
}

/**
 * Which system an entry's login belongs to: its whole address, as the backend decides it.
 * Another port, path or page may be another system, so only the same address counts; only
 * spelling is evened out (case of scheme and host, a default port, a trailing slash, `#…`).
 */
export function systemOf(entry: EntryView): string {
  const url = entry.fields.find((field) => field.name === "网址")?.value?.trim() ?? "";
  if (!url) return (entry.platform || entry.title).toLowerCase();
  try {
    const parsed = new URL(url);
    if (!parsed.hostname) return url;
    const port = parsed.port || ({ "https:": "443", "http:": "80" } as Record<string, string>)[parsed.protocol] || "";
    return `${parsed.protocol}//${parsed.hostname}:${port}${parsed.pathname.replace(/\/+$/, "")}${parsed.search}`;
  } catch {
    return url;
  }
}

export function accountOf(entry: EntryView): string {
  return entry.fields.find((field) => field.name === "账号")?.value?.trim() ?? "";
}

export type EntryGroup = { key: string; entries: EntryView[]; duplicates: EntryView[][] };

/**
 * Entries of one site together, in the order the list already has (the newest change first).
 * `duplicates` are the sets of one account kept more than once, newest first in each.
 */
export function groupEntries(entries: EntryView[]): EntryGroup[] {
  const groups = new Map<string, EntryView[]>();
  for (const entry of entries) {
    const key = siteKey(entry) || entry.id;
    groups.set(key, [...(groups.get(key) ?? []), entry]);
  }
  return [...groups].map(([key, members]) => {
    const byAccount = new Map<string, EntryView[]>();
    for (const entry of members) {
      const account = accountOf(entry);
      // One account on one system: the same name on another port is another login.
      const key = `${systemOf(entry)}
${account}`;
      if (account && entry.kind !== "ssh_key") byAccount.set(key, [...(byAccount.get(key) ?? []), entry]);
    }
    const duplicates = [...byAccount.values()].filter((set) => set.length > 1).map((set) => [...set].sort((a, b) => b.updatedAt - a.updatedAt));
    return { key, entries: members, duplicates };
  });
}

export function platformsOf(entries: EntryView[]): string[] {
  return [...new Set(entries.map((entry) => entry.platform).filter(Boolean))].sort((a, b) => a.localeCompare(b));
}

/** 0 when empty, 1 below the minimum length, then 2 to 4: one more for three kinds of character, one more for 14 characters or longer. */
export function passwordStrength(password: string): 0 | 1 | 2 | 3 | 4 {
  const length = [...password].length;
  if (length === 0) return 0;
  if (length < MIN_PASSWORD_CHARS) return 1;
  const kinds = [/[a-z]/, /[A-Z]/, /\d/, /[^A-Za-z0-9]/].filter((kind) => kind.test(password)).length;
  return (2 + Number(kinds >= 3) + Number(length >= 14)) as 2 | 3 | 4;
}

export function formatTime(ms: number): string {
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
