import { SiteError } from "../shared/types";

const broken = (path: string) => new SiteError("E_BROKEN", path);

export function obj(v: unknown, path: string): Record<string, unknown> {
  if (typeof v !== "object" || v === null || Array.isArray(v)) throw broken(path);
  return v as Record<string, unknown>;
}
export function arr(v: unknown, path: string): unknown[] {
  if (!Array.isArray(v)) throw broken(path);
  return v;
}
export function str(v: unknown, path: string): string {
  if (typeof v !== "string") throw broken(path);
  return v;
}
export function optStr(v: unknown): string {
  return typeof v === "string" ? v : "";
}
export function bool(v: unknown): boolean {
  return v === true;
}
/** ISO text, epoch seconds or epoch milliseconds → milliseconds. */
export function time(v: unknown, path: string): number {
  if (typeof v === "number" && Number.isFinite(v)) return v < 1e11 ? Math.round(v * 1000) : Math.round(v);
  if (typeof v === "string") {
    const ms = Date.parse(v);
    if (!Number.isNaN(ms)) return ms;
  }
  throw broken(path);
}
