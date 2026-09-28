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

const GB = 1024 ** 3;
const gb = (bytes: number) => `${(bytes / GB).toFixed(1)} GB`;

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

/** The disk under the most pressure, which is the one worth saying something about. */
export function tightestVolume(volumes: VolumeInfo[]): { root: string; ratio: number } | null {
  const fixed = volumes.filter((v) => v.fixed && v.totalBytes > 0);
  if (!fixed.length) return null;
  const scored = fixed.map((v) => ({ root: v.root.replace(/\\$/, ""), ratio: (v.totalBytes - v.freeBytes) / v.totalBytes }));
  return scored.sort((a, b) => b.ratio - a.ratio)[0];
}

/** Four lines about the machine, each one click from the page that acts on it. */
export function WorkstationGroups({ onOpen }: { onOpen: (page: Page) => void }) {
  const { tr: t } = useI18n();
  const [groups, setGroups] = useState<Group[]>([]);

  useEffect(() => {
    let alive = true;
    const add = (group: Group) => {
      if (alive) setGroups((old) => [...old.filter((g) => g.id !== group.id), group].sort((a, b) => ORDER.indexOf(a.id) - ORDER.indexOf(b.id)));
    };

    void invoke<VibeTool[]>("vibe_tools").then((tools) => {
      const { installed, total, updates } = agentTally(tools);
      add({
        id: "agents", icon: "ti-robot", label: "工作智能体", page: "agents",
        tone: updates ? "warn" : installed ? "ok" : "idle",
        summary: installed
          ? `${t("已安装")} ${installed} / ${total}${updates ? ` · ${updates} ${t("个可更新")}` : ` · ${t("都是最新")}`}`
          : t("还没有安装任何智能体"),
      });
    }).catch(() => undefined);

    void invoke<GatewayStatus>("gateway_status").then((status) => {
      add({
        id: "gateway", icon: "ti-plug-connected", label: "接口服务", page: "gateway",
        tone: status.running ? "ok" : "idle",
        summary: status.running
          ? `${t("运行中")} · ${status.port}${status.lanAccess ? ` · ${t("已对局域网开放")}` : ` · ${t("仅本机")}`}`
          : t("未开启：开启后本机程序可以直接调用已登录的智能体"),
      });
    }).catch(() => undefined);

    void invoke<SyncReport>("proxy_sync_report").then((report) => {
      const on = report.system.state === "on";
      add({
        id: "proxy", icon: "ti-world-bolt", label: "网络代理", page: "proxy",
        tone: report.rows.length ? "warn" : on ? "ok" : "idle",
        summary: `${t(on ? "系统代理已开启" : report.system.state === "stale" ? "系统代理设置残留" : "系统代理未开启")}${on ? ` ${report.system.server}` : ""}`
          + (report.rows.length ? ` · ${report.rows.length} ${t("处与系统不一致")}` : ` · ${t("各处一致")}`),
      });
    }).catch(() => undefined);

    void invoke<VolumeInfo[]>("space_fixed_volumes").then((volumes) => {
      const tight = tightestVolume(volumes);
      const free = volumes.filter((v) => v.fixed).reduce((sum, v) => sum + v.freeBytes, 0);
      add({
        id: "disk", icon: "ti-database", label: "磁盘空间", page: "cleanup",
        tone: !tight ? "idle" : tight.ratio >= 0.9 ? "bad" : tight.ratio >= 0.75 ? "warn" : "ok",
        summary: tight
          ? `${tight.root} ${t("已用")} ${Math.round(tight.ratio * 100)}% · ${t("本机剩余")} ${gb(free)}`
          : t("读不到本机磁盘"),
      });
    }).catch(() => undefined);

    return () => { alive = false; };
  }, [t]);

  if (!groups.length) return null;
  return <div className="ws-groups">
    {groups.map((group) => <button type="button" className={"ws-group " + group.tone} key={group.id} onClick={() => onOpen(group.page)}>
      <span className="ws-icon"><i className={"ti " + group.icon} /></span>
      <span className="ws-text">
        <b>{t(group.label)}</b>
        <small>{group.summary}</small>
      </span>
      <i className="ti ti-chevron-right" />
    </button>)}
  </div>;
}

const ORDER = ["agents", "gateway", "proxy", "disk"];
