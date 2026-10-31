import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { formatBytes } from "../../notifications";
import type { Page } from "../../pageState";
import { useToast } from "../../ui";
import { UpdatePlanModal } from "../agents/UpdatePlanModal";
import { loadCatalog, runVibeCheck, subscribeVibe, vibeSnapshot, type VibeSurface, type VibeTool } from "../agents/catalogStore";
import { showSessionTab } from "../sessions/SessionCatalog";
import { scanFootprint } from "../sessions/api";
import type { FootprintReport } from "../sessions/types";
import { vaultApi, type VaultStatus } from "../vault/api";
import "./coreEntries.css";

type GatewayStatus = { running: boolean; port: number; error: string };
type SystemState = "on" | "off" | "stale" | "unknown";
type SyncReport = { system: { state: SystemState; server: string }; rows: { id: string }[] };
type ProxyOverview = { locations: { id: string; value: string | null }[] };
type VolumeInfo = { root: string; totalBytes: number; freeBytes: number; fixed: boolean };
export type GroupTone = "ok" | "warn" | "bad" | "idle";

const SESSIONS_KEY = "stacker.overview.sessions.v1";

type SessionUsage = { total: number; shares: ReturnType<typeof sessionShares> };

/** What the cards read last, kept across page switches so coming back shows it at once. */
type Seen = {
  gateway: GatewayStatus;
  proxy: { report: SyncReport; on: Set<string> };
  disks: VolumeInfo[];
  sessions: SessionUsage;
  vault: { status: VaultStatus; entries: number | null };
};
const seen: Partial<Seen> = { sessions: restoreSessions() };
const failed = new Set<keyof Seen>();

const GB = 1024 ** 3;
const MAX_LOGOS = 8;
const PROBE_URL = "https://api.openai.com";
const GATEWAY_ERRORS: Record<string, string> = { E_PORT: "端口被占用或无效（需 1024–65535），请换一个端口。" };
const PROXY_CHIPS: [string, string][] = [["env", "终端"], ["git", "Git"], ["npm", "npm"], ["maven", "Maven"]];

// Counting the session data takes half a minute on a busy machine: the card shows the last
// count at once and replaces it when the new one is in.
function restoreSessions(): SessionUsage | undefined {
  try {
    return JSON.parse(localStorage.getItem(SESSIONS_KEY) ?? "null") ?? undefined;
  } catch {
    return undefined;
  }
}

function keepSessions(usage: SessionUsage) {
  try {
    localStorage.setItem(SESSIONS_KEY, JSON.stringify(usage));
  } catch {
    // Only saves a wait on the next launch.
  }
  return usage;
}

/** How many agents are installed, and how many of their surfaces want an update. */
export function agentTally(tools: { cli?: Pick<VibeSurface, "installed" | "update_available"> | null; desktop?: Pick<VibeSurface, "installed" | "update_available"> | null }[]) {
  let installed = 0;
  let updates = 0;
  for (const tool of tools) {
    const surfaces = [tool.cli, tool.desktop].filter((s) => !!s);
    if (surfaces.some((s) => s.installed)) installed += 1;
    updates += surfaces.filter((s) => s.installed && s.update_available).length;
  }
  return { installed, total: tools.length, updates };
}

/** The logos on the card: one per product, the ones in use first. */
export function logoTools(tools: Pick<VibeTool, "id" | "family_id" | "sort_order" | "icon" | "cli" | "desktop">[]) {
  const used = (tool: (typeof tools)[number]) => tool.cli.installed || tool.desktop.installed;
  const ordered = tools.filter((tool) => tool.icon)
    .sort((a, b) => Number(used(b)) - Number(used(a)) || a.sort_order - b.sort_order);
  const families = new Set<string>();
  return ordered.filter((tool) => {
    if (families.has(tool.family_id)) return false;
    families.add(tool.family_id);
    return true;
  }).slice(0, MAX_LOGOS).map((tool) => ({ ...tool, on: used(tool) }));
}

/** The biggest users of the session card's bar, each as a share of the whole. */
export function sessionShares(report: Pick<FootprintReport, "agents" | "total">, slices = 3) {
  if (report.total <= 0) return [];
  return report.agents.filter((agent) => agent.total > 0)
    .sort((a, b) => b.total - a.total).slice(0, slices)
    .map((agent) => ({ name: agent.product.name, bytes: agent.total, percent: agent.total / report.total * 100 }));
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

/** Fixed disks, the one under the most pressure first. */
export function rankedVolumes(volumes: VolumeInfo[]) {
  return volumes.filter((v) => v.fixed && v.totalBytes > 0).map((v) => {
    const ratio = (v.totalBytes - v.freeBytes) / v.totalBytes;
    return { root: v.root.replace(/\\$/, ""), ratio, free: v.freeBytes, tone: diskTone(v.freeBytes, ratio) };
  }).sort((a, b) => TONE_RANK[a.tone] - TONE_RANK[b.tone] || b.ratio - a.ratio);
}

/** One entry: the whole card opens the page; its own buttons sit above that and act directly. */
function Entry({ hue, wide, icon, title, tagline, page, onOpen, children }: {
  hue: string; wide?: boolean; icon: string; title: string; tagline: string; page: Page;
  onOpen: (page: Page) => void; children: ReactNode;
}) {
  const { tr: t } = useI18n();
  return <section className={`core-card ${hue}${wide ? " wide" : ""}`}>
    <div className="core-side">
      <div className="core-head">
        <span className="core-tile"><i className={"ti " + icon} aria-hidden="true" /></span>
        <button type="button" className="core-open" onClick={() => onOpen(page)}>{t(title)}</button>
        <i className="ti ti-chevron-right core-arrow" aria-hidden="true" />
      </div>
      <p className="core-tag">{t(tagline)}</p>
    </div>
    {children}
  </section>;
}

function AgentsEntry({ onOpen }: { onOpen: (page: Page) => void }) {
  const { tr: t } = useI18n();
  const [vibe, setVibe] = useState(vibeSnapshot);
  const [planOpen, setPlanOpen] = useState(false);
  const closePlan = useCallback(() => setPlanOpen(false), []);
  useEffect(() => subscribeVibe(setVibe), []);
  useEffect(() => {
    void loadCatalog().catch(() => undefined);
    void runVibeCheck().catch(() => undefined);
  }, []);
  const { installed, total, updates } = agentTally(vibe.tools);
  const reading = !vibe.checked;

  return <section className="core-card core-hero">
    <div className="core-hero-head">
      <span className="core-tile"><i className="ti ti-sparkles" aria-hidden="true" /></span>
      <i className="ti ti-chevron-right core-arrow" aria-hidden="true" />
    </div>
    <button type="button" className="core-open" onClick={() => onOpen("agents")}>{t("智能体")}</button>
    <p className="core-tag">{t("一处安装、更新你所有的 AI 编程工具")}</p>
    <div className="core-logos">
      {logoTools(vibe.tools).map((tool) => <span key={tool.id} className={"core-logo" + (tool.on ? " on" : "")}><img src={`/brands/${tool.icon}`} alt="" /></span>)}
    </div>
    <div className="core-hero-foot">
      {reading
        ? <div className="core-stat"><span><i className="ti ti-loader spin" /> {t("正在检测…")}</span></div>
        : updates > 0
          ? <div className="core-stat"><strong className="accent">{updates}</strong><span>{t("个可更新 · 已装 {installed} / {total}").replace("{installed}", String(installed)).replace("{total}", String(total))}</span></div>
          : <div className="core-stat"><strong>{installed}</strong><span>{t("个已安装 · 共 {total} 个").replace("{total}", String(total))}</span></div>}
      {updates > 0
        ? <button type="button" className="pr core-wide" onClick={() => setPlanOpen(true)}><i className="ti ti-cloud-upload" /> {t("一键更新")}</button>
        : <button type="button" className="gh core-wide" onClick={() => onOpen("agents")}><i className="ti ti-apps" /> {t(installed || reading ? "查看全部智能体" : "去安装智能体")}</button>}
    </div>
    {planOpen && <UpdatePlanModal onClose={closePlan} />}
  </section>;
}

function SessionsEntry({ onOpen }: { onOpen: (page: Page) => void }) {
  const { tr: t } = useI18n();
  const usage = seen.sessions;
  const shares = usage?.shares ?? [];
  return <Entry hue="blue" icon="ti-database" title="会话数据" tagline="查看、搜索、迁移 AI 的聊天记录，清掉没用的" page="agent-data" onOpen={onOpen}>
    <div className="core-foot">
      <div className="core-bar" role="img" aria-label={t("会话占比")} title={shares.map((s) => `${s.name} ${formatBytes(s.bytes)}`).join(" · ")}>
        {shares.map((s) => <span key={s.name} style={{ width: `${s.percent}%` }} />)}
      </div>
      <div className="core-row">
        {usage
          ? <strong>{formatBytes(usage.total)}</strong>
          : <span className="core-note">{t(failed.has("sessions") ? "暂时读不到" : "正在统计…")}</span>}
        <button type="button" className="gh sm" onClick={() => { showSessionTab("footprint"); onOpen("agent-data"); }}>{t("看占用")}</button>
      </div>
    </div>
  </Entry>;
}

function GatewayEntry({ onOpen, refresh }: { onOpen: (page: Page) => void; refresh: () => void }) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const status = seen.gateway;
  const address = status ? `127.0.0.1:${status.port}/v1` : "";

  async function start() {
    if (!status) return;
    setBusy(true);
    try {
      const next = await invoke<GatewayStatus>("gateway_set", { enabled: true, port: status.port });
      seen.gateway = next;
      refresh();
      if (next.error) toast(t(GATEWAY_ERRORS[next.error] ?? next.error), "err");
      else toast(t("接口服务已开启"), "ok");
    } catch (error) {
      toast(t(GATEWAY_ERRORS[String(error)] ?? String(error)), "err");
    } finally {
      setBusy(false);
    }
  }

  return <Entry hue="violet" icon="ti-plug-connected" title="接口服务" tagline="把已登录的智能体变成 OpenAI 接口，给你的程序用" page="gateway" onOpen={onOpen}>
    <div className="core-foot">
      <div className="core-address">{address || "—"}</div>
      <div className="core-row">
        <span className={"core-note" + (status?.running ? " good" : "")}>{t(!status ? (failed.has("gateway") ? "暂时读不到" : "正在读取…") : status.running ? "运行中" : "未启用")}</span>
        {status?.running
          ? <button type="button" className="gh sm" onClick={() => void navigator.clipboard.writeText(`http://${address}`).then(() => toast(t("已复制"), "ok"))}>{t("复制地址")}</button>
          : <button type="button" className="gh sm" disabled={!status || busy} onClick={() => void start()}>{t(busy ? "启动中…" : "启动")}</button>}
      </div>
    </div>
  </Entry>;
}

function ProxyEntry({ onOpen }: { onOpen: (page: Page) => void }) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const [probing, setProbing] = useState(false);
  const proxy = seen.proxy;
  const on = proxy?.report.system.state === "on";
  const drift = proxy?.report.rows.length ?? 0;
  const state = !proxy ? { text: failed.has("proxy") ? "暂时读不到" : "正在读取…", className: "" }
    : proxy.report.system.state === "stale" ? { text: "设置残留", className: " warn" }
    : drift ? { text: t("{count} 处不一致").replace("{count}", String(drift)), className: " warn" }
    : on ? { text: "已开启", className: " good" }
    : { text: "未开启", className: "" };

  async function probe() {
    setProbing(true);
    try {
      const outcome = await invoke<string | null>("proxy_test", { url: PROBE_URL }) ?? t("没有可用的代理");
      toast(`${t("经代理访问")} ${PROBE_URL}：${outcome}`, outcome.startsWith("HTTP") ? "ok" : "err");
    } catch (error) {
      toast(t("测试失败：") + error, "err");
    } finally {
      setProbing(false);
    }
  }

  return <Entry hue="green" icon="ti-world-bolt" title="网络代理" tagline="一处管好终端、Git、npm 用的代理" page="proxy" onOpen={onOpen}>
    <div className="core-foot">
      <div className="core-chips">
        {PROXY_CHIPS.map(([id, label]) => <span key={id} className={proxy?.on.has(id) ? "on" : ""}>{t(label)}</span>)}
      </div>
      <div className="core-row">
        <span className={"core-note" + state.className}>{t(state.text)}</span>
        <button type="button" className="gh sm" disabled={!on || probing} onClick={() => void probe()}>
          {t(probing ? "测试中…" : "测连通")}
        </button>
      </div>
    </div>
  </Entry>;
}

function DiskEntry({ onOpen, onScan, cleanable }: { onOpen: (page: Page) => void; onScan: () => void; cleanable: number }) {
  const { tr: t } = useI18n();
  const disks = rankedVolumes(seen.disks ?? []);
  const tight = disks[0];
  return <Entry hue={"red" + (tight && tight.tone !== "ok" ? " " + tight.tone : "")} wide icon="ti-eraser" title="磁盘清理" tagline="找出开发缓存和大文件，安全腾出空间" page="cleanup" onOpen={onOpen}>
    <button type="button" className="pr sm core-scan" onClick={onScan}><i className="ti ti-radar-2" /> {t("快速扫描")}</button>
    <div className="core-disks">
      {cleanable > 0
        ? <div className="core-stat"><strong className="hue">{formatBytes(cleanable)}</strong><span>{t("可清理")}</span></div>
        : tight
          ? <div className="core-stat"><strong>{formatBytes(tight.free)}</strong><span>{t("{disk} 剩余").replace("{disk}", tight.root)}</span></div>
          : <div className="core-stat"><span>{t(failed.has("disks") ? "无法读取本机磁盘" : "正在读取…")}</span></div>}
      {disks.slice(0, 2).map((disk) => <div className={"core-disk " + disk.tone} key={disk.root}>
        <div><span>{disk.root}</span><span>{Math.round(disk.ratio * 100)}% · {t("剩 {free}").replace("{free}", formatBytes(disk.free))}</span></div>
        <div className="core-meter"><span style={{ width: `${disk.ratio * 100}%` }} /></div>
      </div>)}
    </div>
  </Entry>;
}

function VaultEntry({ onOpen }: { onOpen: (page: Page) => void }) {
  const { tr: t } = useI18n();
  const vault = seen.vault;
  const state = vault?.status.state;
  return <Entry hue="teal" icon="ti-shield-lock" title="密钥保管" tagline="加密保存 API Key、令牌和网站登录" page="vault" onOpen={onOpen}>
    <div className="core-foot">
      <div className="core-row">
        {state === "unlocked" && vault?.entries !== null
          ? <div className="core-stat small"><strong>{vault?.entries}</strong><span>{t("条 · 已解锁")}</span></div>
          : <span className="core-note">{t(!vault ? (failed.has("vault") ? "暂时读不到" : "正在读取…") : state === "missing" ? "尚未创建" : "已锁定")}</span>}
        <button type="button" className="gh sm" onClick={() => onOpen("vault")}>{t(state === "missing" ? "创建" : state === "unlocked" ? "打开" : "解锁")}</button>
      </div>
    </div>
  </Entry>;
}

/** The app's core features as the first thing on the home page, each saying what it is for. */
export function CoreEntries({ onOpen, onScan, cleanable }: { onOpen: (page: Page) => void; onScan: () => void; cleanable: number }) {
  const [, redraw] = useState(0);
  const refresh = useCallback(() => redraw((n) => n + 1), []);

  useEffect(() => {
    let alive = true;
    const read = <K extends keyof Seen>(key: K, task: Promise<Seen[K]>) => {
      void task.then((value) => {
        seen[key] = value;
        failed.delete(key);
      }).catch(() => {
        if (!seen[key]) failed.add(key);
      }).finally(() => {
        if (alive) refresh();
      });
    };
    read("gateway", invoke<GatewayStatus>("gateway_status"));
    read("disks", invoke<VolumeInfo[]>("space_fixed_volumes"));
    read("sessions", scanFootprint(false).then((report) => keepSessions({ total: report.total, shares: sessionShares(report) })));
    read("proxy", Promise.all([invoke<SyncReport>("proxy_sync_report"), invoke<ProxyOverview>("proxy_overview")])
      .then(([report, overview]) => ({ report, on: new Set(overview.locations.filter((place) => place.value).map((place) => place.id)) })));
    read("vault", vaultApi.status().then(async (status) => ({
      status,
      entries: status.state === "unlocked" ? (await vaultApi.list(false)).length : null,
    })));
    return () => { alive = false; };
  }, [refresh]);

  return <div className="core-grid">
    <AgentsEntry onOpen={onOpen} />
    <SessionsEntry onOpen={onOpen} />
    <GatewayEntry onOpen={onOpen} refresh={refresh} />
    <ProxyEntry onOpen={onOpen} />
    <DiskEntry onOpen={onOpen} onScan={onScan} cleanable={cleanable} />
    <VaultEntry onOpen={onOpen} />
  </div>;
}
