import { surfaceDetected, type VibeSurface } from "./catalogStore";

export type TileTone = "ok" | "update" | "broken" | "missing" | "na" | "busy";

/** "codex-cli 0.155.1" → "0.155.1", "2.1.268 (Claude Code)" → "2.1.268"; anything else as is. */
export function shortVersion(version: string | null | undefined): string | null {
  if (!version) return null;
  const match = /\d+(?:\.\d+){1,3}/.exec(version);
  return match ? match[0] : version;
}

/** One line of a thumbnail: how a CLI or desktop app stands, in a few characters. */
export function tileLine(surface: VibeSurface, busy: boolean): { tone: TileTone; text: string } {
  if (!surface.available) return { tone: "na", text: "不支持" };
  if (busy) return { tone: "busy", text: "处理中" };
  if (surface.health === "broken" || surface.status === "broken") return { tone: "broken", text: "已损坏" };
  if (!surfaceDetected(surface)) return { tone: "missing", text: "未安装" };
  const current = shortVersion(surface.version);
  if (surface.update_available) {
    return { tone: "update", text: `${current ?? "?"} → ${shortVersion(surface.latest) ?? "?"}` };
  }
  return { tone: "ok", text: current ?? "已安装" };
}
