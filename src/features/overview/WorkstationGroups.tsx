import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import type { Page } from "../../pageState";

type VibeTool = {
  id: string;
  cli?: { installed?: boolean; updateAvailable?: boolean } | null;
  desktop?: { installed?: boolean; updateAvailable?: boolean } | null;
};
type GatewayStatus = { running: boolean; port: number; lanAccess: boolean };
type SyncReport = {
  system: { state: "on" | "off" | "stale" | "unknown"; server: string };
  rows: { id: string }[];
};
type VolumeInfo = { root: string; totalBytes: number; freeBytes: number; fixed: boolean };

export type GroupTone = "ok" | "warn" | "bad" | "idle";
export type Group = { id: string; icon: string; label: string; summary: string; tone: GroupTone; page: Page };
type Slot = { id: string; icon: string; label: string; page: Page };

const GB = 1024 ** 3;
const gb = (bytes: number) => `${(bytes / GB).toFixed(1)} GB`;

/** The four cards, in their places from the first frame: a card waits for its line, the page does not wait for a card. */
const SLOTS: Slot[] = [
  { id: "agents", icon: "ti-robot", label: "工作智能体", page: "agents" },
  { id: "gateway", icon: "ti-plug-connected", label: "接口服务", page: "gateway" },
  { id: "proxy", icon: "ti-world-bolt", label: "网络代理", page: "proxy" },
  { id: "disk", icon: "ti-database", label: "磁盘空间", page: "cleanup" },
];
const TONE_TEXT: Record<GroupTone, string> = { ok: "正常", warn: "留意", bad: "需处理", idle: "未启用" };

// What the cards said last time, kept across page switches: coming back shows it at once
// while the fresh reading replaces it line by line.
const lastSeen = new Map<string, Group>();
const failed = new Set<string>();

/** How many of an agent's two surfaces are installed, and how many want an update. */
export function agentTally(tools: VibeTool[]): { installed: number; total: number; updates: number } {
  let installed = 0;
  let updates = 0;
  for (const tool of tools) {
    const surfaces = [tool.cli, tool.desktop].filter(Boolean) as NonNullable<VibeTool["cli"]>[];
    if (surfaces.some((s) => s.installed)) installed += 1;
    updates += surfaces.filter((s) => s.installed && s.updateAvailable).length;
  }
  return { installed, total: tools.length, updates };
}

/**
 * How one disk is doing, by the room it has left: a big disk 90% full still has hundreds of
 * gigabytes, which is plenty, while a small one with a few left is a problem whatever its share.
 */
export function diskTone(free: number, ratio: number): GroupTone {
  if (free < 10 * GB) return "bad";
  if (free < 30 * GB || (ratio >= 0.95 && free < 100 * GB)) return "warn";
  return "ok";
}

const TONE_RANK: Record<GroupTone, number> = { bad: 0, warn: 1, ok: 2, idle: 3 };

/** The disk under the most pressure, which is the one worth saying something about. */
export function tightestVolume(volumes: VolumeInfo[]): { root: string; ratio: number; free: number; tone: GroupTone } | null {
  const fixed = volumes.filter((v) => v.fixed && v.totalBytes > 0);
  if (!fixed.length) return null;
  const scored = fixed.map((v) => {
    const ratio = (v.totalBytes - v.freeBytes) / v.totalBytes;
    return { root: v.root.replace(/\\$/, ""), ratio, free: v.freeBytes, tone: diskTone(v.freeBytes, ratio) };
  });
  return scored.sort((a, b) => TONE_RANK[a.tone] - TONE_RANK[b.tone] || b.ratio - a.ratio)[0];
}

/** Four lines about the machine, each one click from the page that acts on it. */
export function WorkstationGroups({ onOpen }: { onOpen: (page: Page) => void }) {
  const { tr: t } = useI18n();
  const [, redraw] = useState(0);

  useEffect(() => {
    let alive = true;
    const add = (id: string, tone: GroupTone, summary: string) => {
      const slot = SLOTS.find((s) => s.id === id)!;
      lastSeen.set(id, { ...slot, tone, summary });
      failed.delete(id);
      if (alive) redraw((n) => n + 1);
    };
    const fail = (id: string) => () => {
      if (!lastSeen.has(id)) failed.add(id);
      if (alive) redraw((n) => n + 1);
    };

    void invoke<VibeTool[]>("vibe_tools").then((tools) => {
      const { installed, total, updates } = agentTally(tools);
      add("agents", updates ? "warn" : installed ? "ok" : "idle", installed
        ? `${t("已安装")} ${installed} / ${total}${updates ? ` · ${updates} ${t("个可更新")}` : ` · ${t("都是最新")}`}`
        : t("还没有安装任何智能体"));
    }).catch(fail("agents"));

    void invoke<GatewayStatus>("gateway_status").then((status) => {
      add("gateway", status.running ? "ok" : "idle", status.running
        ? `${t("运行中")} · ${status.port}${status.lanAccess ? ` · ${t("已对局域网开放")}` : ` · ${t("仅本机")}`}`
        : t("未开启：开启后本机程序可以直接调用已登录的智能体"));
    }).catch(fail("gateway"));

    void invoke<SyncReport>("proxy_sync_report").then((report) => {
      const on = report.system.state === "on";
      add("proxy", report.rows.length ? "warn" : on ? "ok" : "idle",
        `${t(on ? "系统代理已开启" : report.system.state === "stale" ? "系统代理设置残留" : "系统代理未开启")}${on ? ` ${report.system.server}` : ""}`
        + (report.rows.length ? ` · ${report.rows.length} ${t("处与系统不一致")}` : ` · ${t("各处一致")}`));
    }).catch(fail("proxy"));

    void invoke<VolumeInfo[]>("space_fixed_volumes").then((volumes) => {
      const tight = tightestVolume(volumes);
      const fixed = volumes.filter((v) => v.fixed);
      const free = fixed.reduce((sum, v) => sum + v.freeBytes, 0);
      // The disk's own room first; the total only when there is more than one disk.
      add("disk", tight ? tight.tone : "idle", tight
        ? `${tight.root} ${t("剩余")} ${gb(tight.free)}（${t("已用")} ${Math.round(tight.ratio * 100)}%）`
          + (fixed.length > 1 ? ` · ${t("全部磁盘剩余")} ${gb(free)}` : "")
        : t("无法读取本机磁盘"));
    }).catch(fail("disk"));

    return () => { alive = false; };
  }, [t]);

  return <div className="ws-groups">
    {SLOTS.map((slot) => {
      const group = lastSeen.get(slot.id);
      const state = group ? group.tone : failed.has(slot.id) ? "idle" : "wait";
      const summary = group ? group.summary : t(failed.has(slot.id) ? "暂时无法读取，点击查看详情" : "正在读取…");
      return <button type="button" className={"ws-group " + state} key={slot.id} title={`${t(slot.label)}：${summary}`} onClick={() => onOpen(slot.page)}>
        <span className="ws-icon"><i className={"ti " + (group || failed.has(slot.id) ? slot.icon : "ti-loader spin")} /></span>
        <span className="ws-text">
          <b>{t(slot.label)}{group && <em>{t(TONE_TEXT[group.tone])}</em>}</b>
          <small>{summary}</small>
        </span>
        <i className="ti ti-chevron-right" />
      </button>;
    })}
  </div>;
}
