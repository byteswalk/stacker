import type { EntryView } from "./api";

export const EXPIRY_WARN_DAYS = 14;
export const MIN_PASSWORD_CHARS = 9;
const DAY_MS = 86_400_000;

export type ExpiryState = "none" | "ok" | "soon" | "expired";
export type ListFilter = { query: string; platform: string; soonOnly: boolean };

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
    .filter((entry) => !filter.soonOnly || ["soon", "expired"].includes(expiryState(entry.expiresAt, today)))
    .sort((a, b) => b.updatedAt - a.updatedAt);
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
