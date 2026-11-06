import { useCallback, useEffect, useState, type ComponentProps, type ReactNode } from "react";
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
import { vaultApi, type EntryView, type VaultStatus } from "../vault/api";
import { CoreArt } from "./coreArt";

type GatewayStatus = { running: boolean; port: number; error: string; lanAccess: boolean };
type SystemState = "on" | "off" | "stale" | "unknown";
type SyncReport = { system: { state: SystemState; server: string }; rows: { id: string }[] };
type ProxyOverview = { locations: { id: string; value: string | null }[] };
type VolumeInfo = { root: string; totalBytes: number; freeBytes: number; fixed: boolean };
export type GroupTone = "ok" | "warn" | "bad" | "idle";

const SESSIONS_KEY = "stacker.overview.sessions.v2";
const VAULT_KEY = "stacker.overview.vault.v1";

type SessionUsage = { total: number; reclaimable: number; shares: ReturnType<typeof sessionShares> };
export type VaultTally = { total: number; credentials: number; ssh: number; expiring: number };

/** What the cards read last, kept across page switches so coming back shows it at once. */
type Seen = {
  gateway: GatewayStatus;
  proxy: { report: SyncReport; on: Set<string> };
  disks: VolumeInfo[];
  sessions: SessionUsage;
  vault: { status: VaultStatus; tally: VaultTally | null };
};
const seen: Partial<Seen> = { sessions: restore<SessionUsage>(SESSIONS_KEY) };
const failed = new Set<keyof Seen>();

const GB = 1024 ** 3;
const MAX_LOGOS = 19;
/** The room one disk's ring and its two lines take, gap included. */
const RING_SLOT = 92;
const RING_RADIUS = 24;
const PROBE_URL = "https://api.openai.com";
const GATEWAY_ERRORS: Record<string, string> = { E_PORT: "端口被占用或无效（需 1024–65535），请换一个端口。" };
const PROXY_CHIPS: [string, string][] = [["env", "终端"], ["git", "Git"], ["npm", "npm"], ["maven", "Maven"]];

// Counting the session data takes half a minute on a busy machine: the card shows the last
// count at once and replaces it when the new one is in.
function restore<T>(key: string): T | undefined {
  try {
    return JSON.parse(localStorage.getItem(key) ?? "null") ?? undefined;
  } catch {
    return undefined;
  }
}

function keep<T>(key: string, value: T) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Only saves a wait on the next launch.
  }
  return value;
}

/**
 * What the vault holds, in numbers only. Kept from the last unlock so a locked vault can still
 * say how much is in it: the vault file itself tells nothing without the password.
 */
export function vaultTally(entries: Pick<EntryView, "kind" | "expiresAt">[], today = new Date()): VaultTally {
  const soon = new Date(today.getTime() + 30 * 24 * 3600 * 1000).toISOString().slice(0, 10);
  const ssh = entries.filter((entry) => entry.kind === "ssh_key").length;
  return {
    total: entries.length,
    credentials: entries.length - ssh,
    ssh,
    expiring: entries.filter((entry) => entry.expiresAt && entry.expiresAt <= soon).length,
  };
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
export function logoTools(tools: Pick<VibeTool, "id" | "name" | "family_id" | "sort_order" | "icon" | "cli" | "desktop">[]) {
  const used = (tool: (typeof tools)[number]) => tool.cli.installed || tool.desktop.installed;
  const ordered = tools.filter((tool) => tool.icon)
    .sort((a, b) => Number(used(b)) - Number(used(a)) || a.sort_order - b.sort_order);
  const families = new Set<string>();
  return ordered.filter((tool) => {
    if (families.has(tool.family_id)) return false;
    families.add(tool.family_id);
    return true;
  }).slice(0, MAX_LOGOS).map((tool) => ({
    ...tool,
    on: used(tool),
    update: [tool.cli, tool.desktop].some((surface) => surface.installed && surface.update_available),
  }));
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

/** The disks the card has room for: the tightest ones, shown in drive-letter order. */
export function ringVolumes(disks: ReturnType<typeof rankedVolumes>, room = 3) {
  return disks.slice(0, Math.max(1, room)).sort((a, b) => a.root.localeCompare(b.root));
}

/** How much of a ring is drawn, as an SVG dash: the drawn part, then the rest of the circle. */
export function ringDash(ratio: number, radius = RING_RADIUS) {
  const round = 2 * Math.PI * radius;
  const drawn = Math.min(Math.max(ratio, 0), 1) * round;
  return `${drawn.toFixed(2)} ${round.toFixed(2)}`;
}

/** One entry: the whole card opens the page; its own buttons sit above that and act directly. */
function Entry({ av, art, wide, icon, title, tagline, page, onOpen, children }: {
  av: string; art: ComponentProps<typeof CoreArt>["name"]; wide?: boolean; icon: string; title: string; tagline: string; page: Page;
  onOpen: (page: Page) => void; children: ReactNode;
}) {
  const { tr: t } = useI18n();
  return <section className={"core-card" + (wide ? " wide" : "")}>
    <CoreArt name={art} />
    <div className="core-side">
      <div className="core-head">
        <span className={"av core-tile " + av}><i className={"ti " + icon} aria-hidden="true" /></span>
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
  const logos = logoTools(vibe.tools);
  const more = total - logos.length;

  return <><section className="core-card core-hero">
    <div className="core-head">
      <span className="core-tile" style={{ display: "flex", alignItems: "center", justifyContent: "center" }}><i className="ti ti-sparkles" aria-hidden="true" /></span>
      <button type="button" className="core-open" onClick={() => onOpen("agents")}>{t("智能体")}</button>
      <i className="ti ti-chevron-right core-arrow" aria-hidden="true" />
    </div>
    <p className="core-tag">{t("一处安装、更新你所有的 AI 编程工具")}</p>
    <div className="core-logos">
      {logos.map((tool) => <span key={tool.id} className={"core-logo" + (tool.on ? "" : " off")} title={tool.name}>
        <img src={`/brands/${tool.icon}`} alt="" />
        {tool.update && <i className="core-logo-dot" aria-hidden="true" />}
      </span>)}
      {more > 0 && <button type="button" className="core-logo core-logo-more" title={t("查看全部智能体")} onClick={() => onOpen("agents")}>+{more}</button>}
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
  </section>
  {planOpen && <UpdatePlanModal onClose={closePlan} />}</>;
}

function SessionsEntry({ onOpen }: { onOpen: (page: Page) => void }) {
  const { tr: t } = useI18n();
  const usage = seen.sessions;
  const shares = usage?.shares ?? [];
  return <Entry av="py" art="sessions" icon="ti-database" title="会话数据" tagline="查看、搜索、迁移 AI 的聊天记录，清掉没用的" page="agent-data" onOpen={onOpen}>
    <div className="core-foot">
      <div className="core-bar" role="img" aria-label={t("会话占比")} title={shares.map((s) => `${s.name} ${formatBytes(s.bytes)}`).join(" · ")}>
        {shares.map((s) => <span key={s.name} style={{ width: `${s.percent}%` }} />)}
      </div>
      <div className="core-legend">{shares.map((s) => <span key={s.name}><i aria-hidden="true" />{s.name}</span>)}</div>
      <div className="core-row">
        {usage
          ? <div className="core-stat tiny"><strong>{formatBytes(usage.total)}</strong>{usage.reclaimable > 0 && <span>{t("{size} 可清理").replace("{size}", formatBytes(usage.reclaimable))}</span>}</div>
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

  return <Entry av="php" art="gateway" icon="ti-plug-connected" title="接口服务" tagline="把已登录的智能体变成 OpenAI 接口，给你的程序用" page="gateway" onOpen={onOpen}>
    <div className="core-foot">
      <div className="core-address">{address || "—"}</div>
      <div className="core-row">
        <span className={"core-note" + (status?.running ? " good" : "")}>{!status ? t(failed.has("gateway") ? "暂时读不到" : "正在读取…") : status.running ? `${t("运行中")} · ${t(status.lanAccess ? "局域网可用" : "仅本机")}` : t("未启用")}</span>
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
    : on ? { text: `${t("已开启")} · ${proxy.report.system.server}`, className: " good" }
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

  return <Entry av="pxg" art="proxy" icon="ti-world-bolt" title="网络代理" tagline="一处管好终端、Git、npm 用的代理" page="proxy" onOpen={onOpen}>
    <div className="core-foot">
      <div className="core-chips">
        {PROXY_CHIPS.map(([id, label]) => <span key={id} className={proxy?.on.has(id) ? "on" : ""}>{t(label)}</span>)}
      </div>
      <div className="core-row">
        <span className={"core-note" + state.className} title={state.text}>{t(state.text)}</span>
        <button type="button" className="gh sm" disabled={!on || probing} onClick={() => void probe()}>
          {t(probing ? "测试中…" : "测连通")}
        </button>
      </div>
    </div>
  </Entry>;
}

function DiskRing({ disk }: { disk: ReturnType<typeof rankedVolumes>[number] }) {
  const { tr: t } = useI18n();
  const percent = Math.round(disk.ratio * 100);
  return <div className={"core-ring " + disk.tone}>
    <span className="core-ring-dial" role="img" aria-label={`${disk.root} ${percent}%`}>
      <svg viewBox="0 0 60 60" aria-hidden="true">
        <circle cx="30" cy="30" r={RING_RADIUS} />
        <circle cx="30" cy="30" r={RING_RADIUS} strokeDasharray={ringDash(disk.ratio)} transform="rotate(-90 30 30)" />
      </svg>
      <b>{percent}%</b>
    </span>
    <span className="core-ring-text">
      <b>{t("{disk} 盘").replace("{disk}", disk.root.replace(":", ""))}</b>
      <small>{t("剩 {free}").replace("{free}", formatBytes(disk.free))}</small>
    </span>
  </div>;
}

/** How many rings fit the width the card gives them: a wide window shows every disk it can. */
function useRingRoom() {
  const [box, setBox] = useState<HTMLDivElement | null>(null);
  const [room, setRoom] = useState(3);
  useEffect(() => {
    if (!box) return;
    const measure = () => setRoom(Math.max(1, Math.floor((box.clientWidth + 8) / RING_SLOT)));
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(box);
    return () => watch.disconnect();
  }, [box]);
  return [setBox, room] as const;
}

function DiskEntry({ onOpen, onScan, cleanable }: { onOpen: (page: Page) => void; onScan: () => void; cleanable: number }) {
  const [ringBox, room] = useRingRoom();
  const { tr: t } = useI18n();
  const disks = rankedVolumes(seen.disks ?? []);
  const free = disks.reduce((sum, disk) => sum + disk.free, 0);
  return <Entry av="rs" art="disk" wide icon="ti-eraser" title="磁盘清理" tagline="找出开发缓存和大文件，安全腾出空间" page="cleanup" onOpen={onOpen}>
    <button type="button" className="pr sm core-scan" onClick={onScan}><i className="ti ti-radar-2" /> {t("快速扫描")}</button>
    <div className="core-disks">
      {cleanable > 0
        ? <div className="core-stat"><strong className="orange">{formatBytes(cleanable)}</strong><span>{t("可安全清理")}</span></div>
        : disks.length > 0
          ? <div className="core-stat"><strong>{formatBytes(free)}</strong><span>{t("全部磁盘剩余")}</span></div>
          : <div className="core-stat"><span>{t(failed.has("disks") ? "无法读取本机磁盘" : "正在读取…")}</span></div>}
      <div className="core-rings" ref={ringBox}>{ringVolumes(disks, room).map((disk) => <DiskRing key={disk.root} disk={disk} />)}</div>
    </div>
  </Entry>;
}

function VaultEntry({ onOpen }: { onOpen: (page: Page) => void }) {
  const { tr: t } = useI18n();
  const vault = seen.vault;
  const state = vault?.status.state;
  const tally = vault?.tally;
  const stateText = !vault ? (failed.has("vault") ? "暂时读不到" : "正在读取…")
    : state === "missing" ? "尚未创建" : state === "unlocked" ? "已解锁" : "已锁定";
  return <Entry av="go" art="vault" icon="ti-shield-lock" title="密钥保管" tagline="加密保存 API Key、令牌和网站登录" page="vault" onOpen={onOpen}>
    <div className="core-foot">
      {tally && state !== "missing"
        ? <>
          <div className="core-stat tiny"><strong>{tally.total}</strong><span>{t("条密钥")}</span></div>
          <div className="core-chips teal">
            {tally.credentials > 0 && <span>{t("凭据 {count}").replace("{count}", String(tally.credentials))}</span>}
            {tally.ssh > 0 && <span>SSH {tally.ssh}</span>}
            {tally.expiring > 0 && <span className="warn">{t("{count} 条快到期").replace("{count}", String(tally.expiring))}</span>}
          </div>
        </>
        : vault && state !== "missing" && <div className="core-hint">{t("解锁一次后显示统计")}</div>}
      <div className="core-row">
        <span className={"core-note icon" + (state === "unlocked" ? " good" : "")}>
          {vault && state !== "missing" && <i className={"ti " + (state === "unlocked" ? "ti-lock-open" : "ti-lock")} aria-hidden="true" />}{t(stateText)}
        </span>
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
    read("sessions", scanFootprint(false).then((report) => keep(SESSIONS_KEY, { total: report.total, reclaimable: report.reclaimable, shares: sessionShares(report) })));
    read("proxy", Promise.all([invoke<SyncReport>("proxy_sync_report"), invoke<ProxyOverview>("proxy_overview")])
      .then(([report, overview]) => ({ report, on: new Set(overview.locations.filter((place) => place.value).map((place) => place.id)) })));
    read("vault", vaultApi.status().then(async (status) => ({
      status,
      tally: status.state === "unlocked" ? keep(VAULT_KEY, vaultTally(await vaultApi.list(false))) : restore<VaultTally>(VAULT_KEY) ?? null,
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
