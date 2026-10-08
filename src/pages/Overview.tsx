import { useEffect, useState } from "react";
import { invoke } from "../invoke";
import { translateText } from "../i18n";
import type { Page } from "../pageState";
import { ConfirmModal, useBusy, useToast } from "../ui";
import { useNotifications } from "../notifications";
import { WorkstationGroups } from "../features/overview/WorkstationGroups";
import { quickScanUnlessBusy } from "../features/space-analysis/store";
import { AiAskModal, AiButton, askAi } from "../features/ai/AiAsk";

type Mirror = { id: string; name: string; url: string; host: string };
type ToolState = {
  id: string; name: string; icon: string; config: string;
  installed: boolean; current: string | null; current_label: string; mirrors: Mirror[];
};

type CheckItem = { id: string; sev: string; title: string; desc: string; page: Page; action: string };
type EcosystemSnapshot = {
  id: Page;
  label: string;
  kind: string;
  status: "ok" | "warn" | "missing";
  summary: string;
  detail: string;
};
type CodingEcosystemCheck = {
  ready: boolean;
  title: string;
  summary: string;
  ecosystems: EcosystemSnapshot[];
};
type HostPing = { host: string; ms: number | null };
type ProxyStatus = { host?: string | null; port?: number | null; detected_port?: number | null };

const RUNTIME_SOURCE_KEYS: Record<string, string> = {
  "python-runtime": "stacker.python.downloadSource",
  "node-runtime": "stacker.node.downloadSource",
  "git-runtime": "stacker.git.downloadSource",
  "maven-runtime": "stacker.maven.downloadSource",
  "gradle-runtime": "stacker.gradle.downloadSource",
  "go-runtime": "stacker.go.downloadSource",
};

const ECOSYSTEM_SOURCE_TOOLS: Partial<Record<Page, string[]>> = {
  git: ["git-runtime"],
  python: ["pip", "python-runtime"],
  node: ["npm", "node-runtime"],
  java: [],
  maven: ["maven", "maven-runtime"],
  gradle: ["gradle", "gradle-runtime"],
  go: ["go", "go-runtime"],
  rust: ["cargo", "rust-runtime"],
};

const JAVA_VENDOR_STORAGE = "stacker.java.vendor";
const JAVA_VENDOR_SOURCES = {
  temurin: { label: "清华 Temurin", host: "mirrors.tuna.tsinghua.edu.cn" },
  zulu: { label: "Azul Zulu", host: "cdn.azul.com" },
  dragonwell: { label: "阿里 Dragonwell", host: "dragonwell.oss-cn-shanghai.aliyuncs.com" },
} as const;

function mirrorHost(mirror: Mirror) {
  if (mirror.host.trim()) return mirror.host.trim();
  const raw = mirror.url.replace(/^sparse\+/, "").split(",")[0].trim();
  if (!raw) return "";
  try { return new URL(raw).hostname; } catch { return ""; }
}

function selectedSourceLabel(tool: ToolState) {
  const stored = RUNTIME_SOURCE_KEYS[tool.id]
    ? localStorage.getItem(RUNTIME_SOURCE_KEYS[tool.id])
    : null;
  const selected = stored || tool.current;
  return tool.mirrors.find((mirror) => mirror.id === selected)?.name
    || tool.current_label
    || "未配置";
}
// 可由 Stacker 直接修复的 extra 项 → 返回执行函数（成功时给提示语）；null = 只跳页处理（如 Java 对齐需 UAC + 选方向）。
function extraFixer(id: string): null | (() => Promise<string>) {
  switch (id) {
    case "fnm_no_integration":
      return async () => { await invoke("fnm_write_integration", { shells: ["powershell", "gitbash", "cmd"] }); return "已写入 fnm shell 集成（新终端生效）"; };
    case "proxy_stale":
      return async () => { const count = await invoke<number>("proxy_clear_stale"); return `已清除 ${count} 处 Stacker 写入的失效代理`; };
    case "cache_safe_high":
      return async () => { const freed = await invoke<number>("cleanup_delete_safe"); return `已清理安全缓存，释放 ${(freed / 1073741824).toFixed(1)} GB`; };
    case "windows_temp_high":
      return async () => { const freed = await invoke<number>("cleanup_delete_category", { category: "temp" }); return `已清理临时文件，释放 ${(freed / 1073741824).toFixed(1)} GB`; };
    case "jetbrains_history":
      return async () => { const freed = await invoke<number>("cleanup_delete_category", { category: "history" }); return `已清理历史版本，释放 ${(freed / 1073741824).toFixed(1)} GB`; };
    default:
      return null;
  }
}
// 纳入「一键优化全部」的 extra 项：仅安全、可还原、免提权的（fnm 集成）；
// 缓存清理（删除）、关代理这些副作用项只给各自的行内按钮，不卷进批量。
const BATCH_EXTRA = new Set(["fnm_no_integration"]);
// Deleting from the overview is asked about first: what goes, and that it does not come back.
const CONFIRM_EXTRA: Record<string, string> = {
  windows_temp_high: "清空 Windows 和当前用户的临时目录。正在被程序占用的文件会自动跳过；删除后不能恢复。",
  jetbrains_history: "删除旧版 JetBrains IDE / Android Studio 的数据目录，每个产品保留最新的一个版本；删除后不能恢复。",
};

const PENDING_CHECKS = [
  {
    title: "核心运行时",
    badge: "待体检",
    desc: "检测 Git、Node.js、Python、Java、Go、Rust 等命令是否可用。",
  },
  {
    title: "包管理器与构建工具",
    badge: "待体检",
    desc: "检测 npm、pip、Maven、Gradle、Cargo 等工具链状态。",
  },
  {
    title: "配置、代理与缓存",
    badge: "待体检",
    desc: "检测终端集成、镜像源配置、代理环境变量和开发缓存占用。",
  },
];

type OverviewCache = {
  tools: ToolState[] | null;
  extra: CheckItem[];
  ecosystem: CodingEcosystemCheck | null;
  checking: boolean;
  checked: boolean;
};
const OVERVIEW_INITIAL: OverviewCache = {
  tools: null,
  extra: [],
  ecosystem: null,
  checking: false,
  checked: false,
};
let overviewCache: OverviewCache = OVERVIEW_INITIAL;
let overviewRun: Promise<void> | null = null;
const overviewListeners = new Set<(s: OverviewCache) => void>();

function publishOverview(next: Partial<OverviewCache>) {
  overviewCache = { ...overviewCache, ...next };
  overviewListeners.forEach((fn) => fn(overviewCache));
}

function subscribeOverview(fn: (s: OverviewCache) => void) {
  overviewListeners.add(fn);
  return () => { overviewListeners.delete(fn); };
}

/**
 * The ecosystem cards' own data (each tool's state and its download source), read when the
 * page opens so the cards are there before any checkup; the score waits for「开始体检」.
 */
let statusRun: Promise<void> | null = null;
function loadEcosystemStatus() {
  if (statusRun || overviewRun || overviewCache.tools !== null) return statusRun ?? overviewRun ?? Promise.resolve();
  statusRun = (async () => {
    const [toolsResult, ecosystemResult] = await Promise.allSettled([
      invoke<ToolState[]>("list_sources"),
      invoke<CodingEcosystemCheck>("coding_ecosystem_check"),
    ]);
    // A full checkup that finished meanwhile has fresher data.
    if (!overviewCache.checked) {
      publishOverview({
        tools: toolsResult.status === "fulfilled" ? toolsResult.value : overviewCache.tools,
        ecosystem: ecosystemResult.status === "fulfilled" ? ecosystemResult.value : overviewCache.ecosystem,
      });
    }
  })().finally(() => { statusRun = null; });
  return statusRun;
}

/** Puts one tool on one of its sources, keeping Maven's and Gradle's proxy switch as it is. */
async function applyMirror(tool: ToolState, mirrorId: string, proxy: { host: string; port: number; maven: boolean; gradle: boolean }) {
  const storageKey = RUNTIME_SOURCE_KEYS[tool.id];
  if (storageKey) {
    localStorage.setItem(storageKey, mirrorId);
  } else if (tool.id === "go") {
    await invoke("apply_source_scoped", { toolId: tool.id, mirrorId, scope: "user" });
  } else if (tool.id === "maven" || tool.id === "gradle") {
    await invoke("apply_source", {
      toolId: tool.id,
      mirrorId,
      proxyEnabled: tool.id === "maven" ? proxy.maven : proxy.gradle,
      proxyHost: proxy.host,
      proxyPort: proxy.port,
    });
  } else {
    await invoke("apply_source", { toolId: tool.id, mirrorId });
  }
}

async function currentProxy() {
  const [proxyStatus, maven, gradle] = await Promise.all([
    invoke<ProxyStatus>("proxy_status").catch(() => ({} as ProxyStatus)),
    invoke<boolean>("source_proxy_state", { toolId: "maven", path: null }).catch(() => false),
    invoke<boolean>("source_proxy_state", { toolId: "gradle", path: null }).catch(() => false),
  ]);
  return { host: proxyStatus.host || "127.0.0.1", port: proxyStatus.port || proxyStatus.detected_port || 0, maven, gradle };
}

/** The source a tool is on now: the page's own choice for download sources, else its config. */
function currentSource(tool: ToolState) {
  const storageKey = RUNTIME_SOURCE_KEYS[tool.id];
  return storageKey ? (localStorage.getItem(storageKey) || "official") : tool.current;
}

function runOverviewCheck() {
  if (overviewRun) return overviewRun;
  publishOverview({ checking: true });
  overviewRun = (async () => {
    try {
      const [toolsResult, extraResult, ecosystemResult] = await Promise.allSettled([
        invoke<ToolState[]>("list_sources"),
        invoke<CheckItem[]>("checkup_extra"),
        invoke<CodingEcosystemCheck>("coding_ecosystem_check"),
      ]);
      const next: Partial<OverviewCache> = {};
      const errors: string[] = [];
      if (toolsResult.status === "fulfilled") next.tools = toolsResult.value;
      else errors.push("生态源状态");
      if (extraResult.status === "fulfilled") next.extra = extraResult.value;
      else errors.push("配置与缓存状态");
      if (ecosystemResult.status === "fulfilled") next.ecosystem = ecosystemResult.value;
      else errors.push("开发命令状态");
      if (errors.length < 3) next.checked = true;
      publishOverview(next);
      if (errors.length) throw new Error(`${errors.join("、")}未能完成，请稍后重试。`);
    } finally {
      publishOverview({ checking: false });
      overviewRun = null;
    }
  })();
  return overviewRun;
}

export default function Overview({ goto }: { goto: (p: Page) => void }) {
  const toast = useToast();
  const runBusy = useBusy();
  const notices = useNotifications();
  const [tools, setTools] = useState<ToolState[] | null>(overviewCache.tools);
  const [extra, setExtra] = useState<CheckItem[]>(overviewCache.extra);
  const [ecosystem, setEcosystem] = useState<CodingEcosystemCheck | null>(overviewCache.ecosystem);
  const [checking, setChecking] = useState(overviewCache.checking);
  const [checked, setChecked] = useState(overviewCache.checked);
  const [busy, setBusy] = useState(false);
  const [sourceBusy, setSourceBusy] = useState(false);
  const [rowBusy, setRowBusy] = useState<Record<string, boolean>>({});
  const [diagnosing, setDiagnosing] = useState(false);
  const [confirming, setConfirming] = useState<CheckItem | null>(null);
  const [confirmOfficial, setConfirmOfficial] = useState(false);
  // Disk items open the cleanup list with a quick scan already started, so the details are there.
  const openDetail = (page: Page) => {
    if (page === "cleanup") void quickScanUnlessBusy().catch(() => undefined);
    goto(page);
  };

  useEffect(() => subscribeOverview((s) => {
    setTools(s.tools);
    setExtra(s.extra);
    setEcosystem(s.ecosystem);
    setChecking(s.checking);
    setChecked(s.checked);
  }), []);
  const [statusRead, setStatusRead] = useState(overviewCache.ecosystem !== null);
  useEffect(() => { void loadEcosystemStatus().catch(() => undefined).finally(() => setStatusRead(true)); }, []);

  async function load() {
    return runOverviewCheck();
  }
  async function reloadAll() {
    const wasChecked = hasChecked;
    try {
      // No dialog over the page: the card at the top shows the check running, and it keeps
      // running when another page is opened.
      await load();
      toast(wasChecked ? "开发环境体检已完成" : "开发环境体检完成", "ok");
    } catch (e) {
      toast("体检失败：" + e, "err");
    }
  }

  const batchExtra = extra.filter((e) => BATCH_EXTRA.has(e.id));
  const optimizeCount = batchExtra.length;
  // The score is the checkup's; the cards below show as soon as their data is read.
  const hasChecked = checked;
  const offOfficial = (tools ?? []).filter((tool) => tool.installed && tool.mirrors.some((m) => m.id === "official") && currentSource(tool) !== "official");
  const installedCount = (tools ?? []).filter((t) => !(t.id in RUNTIME_SOURCE_KEYS) && t.installed).length;
  const availableCommands = (ecosystem?.ecosystems ?? []).filter((item) => item.status === "ok").length;
  const emptySetup = hasChecked && tools !== null && installedCount === 0 && availableCommands === 0;
  const allOk = hasChecked && !emptySetup && ecosystem?.ready === true && extra.length === 0;
  const envPenalty = emptySetup ? 100 : Math.min(100, extra.reduce((sum, e) => sum + (e.sev === "warn" ? 20 : e.sev === "mid" ? 10 : 5), 0));
  const envScore = emptySetup ? 0 : Math.max(0, 100 - envPenalty);
  const subtitle = (() => {
    const parts: string[] = [];
    if (extra.length) parts.push(`${extra.length} 项配置 / 缓存可优化`);
    return parts.join(" · ");
  })();
  const checkingAll = checking;
  const ecosystemScore = ecosystem ? Math.max(0, 100 - ecosystem.ecosystems.reduce((sum, item) => sum + (item.status === "missing" ? 15 : item.status === "warn" ? 8 : 0), 0)) : 0;
  const overallScore = emptySetup ? 0 : Math.round(ecosystemScore * 0.85 + envScore * 0.15);
  const overallClass = !hasChecked ? "" : emptySetup || !ecosystem?.ready || overallScore < 60 ? " bad" : overallScore >= 90 ? " ok" : "";
  const overallTitle = !hasChecked ? (checkingAll ? "体检中" : "未开始")
    : emptySetup ? "需要初始化"
    : ecosystem?.title || "需要处理";
  const overallSummary = (() => {
    if (checkingAll) return "正在检测 Git / Node / Python / Java / Go / Rust、包管理器、构建工具、代理与缓存…";
    if (!hasChecked) return "点击「开始体检」后，Stacker 将检测运行时、包管理器、构建工具、代理与缓存状态。";
    if (emptySetup) return "尚未检测到可用的开发命令。可从左侧生态页面安装所需运行时和工具链。";
    const base = ecosystem?.summary || "核心运行时、包管理器和常用构建工具检测完成。";
    return allOk || !subtitle ? base : `${base}；${subtitle}。`;
  })();

  async function runRowTask(key: string, task: () => Promise<void>) {
    if (rowBusy[key]) return;
    setRowBusy((prev) => ({ ...prev, [key]: true }));
    try {
      await task();
    } finally {
      setRowBusy((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    }
  }
  // 单个 extra 项的行内一键修复（fnm 集成 / 关代理 / 清缓存）
  async function runExtra(key: string, fixer: () => Promise<string>) {
    const page = overviewCache.extra.find((item) => item.id === key)?.page;
    await runRowTask(`extra:${key}`, async () => {
      try {
        const msg = await fixer();
        const refreshed = page ? await invoke<CheckItem[]>("checkup_page", { page }) : [];
        if (page) {
          publishOverview({
            extra: [
              ...overviewCache.extra.filter((item) => item.page !== page),
              ...refreshed,
            ],
          });
        } else {
          publishOverview({ extra: overviewCache.extra.filter((item) => item.id !== key) });
        }
        if (page === "cleanup") notices.checkNow("cleanup-row").catch(() => undefined);
        else if (page) notices.checkNow(`${page}-overview-fix`).catch(() => undefined);
        toast(msg, "ok");
      } catch (err) {
        toast("操作失败：" + err, "err");
      }
    });
  }
  async function optimizeAll() {
    setBusy(true);
    let done = 0;
    try {
      await runBusy({ title: "一键修复" }, async () => {
        for (const e of batchExtra) {
          const f = extraFixer(e.id);
          if (f) { await f(); done++; }
        }
        await load();
      });
      notices.checkNow("node-overview-fix").catch(() => undefined);
      toast(`已修复 ${done} 项`, "ok");
    } catch (e) { toast("一键修复未完成：" + e, "err"); } finally { setBusy(false); }
  }

  async function optimizeSources() {
    if (!tools?.length) return;
    setSourceBusy(true);
    try {
      const result = await runBusy(
        {
          title: "智能优选源",
          message: "正在比较各生态下载源与仓库镜像的连接延迟，并应用响应更快的可用源。现有配置会自动备份。",
        },
        async () => {
          const candidates = tools.filter((tool) => tool.installed && tool.mirrors.some((mirror) => !!mirrorHost(mirror)));
          const hasJava = (ecosystem?.ecosystems ?? []).some((item) => item.id === "java" && item.status !== "missing");
          const javaHosts = hasJava ? Object.values(JAVA_VENDOR_SOURCES).map((item) => item.host) : [];
          const hosts = [...new Set([...candidates.flatMap((tool) => tool.mirrors.map(mirrorHost)).filter(Boolean), ...javaHosts])];
          const rows = await invoke<HostPing[]>("speedtest_hosts", { hosts });
          const latency = new Map(rows.filter((row) => typeof row.ms === "number").map((row) => [row.host, row.ms as number]));
          const selected = candidates.flatMap((tool) => {
            const fastest = tool.mirrors
              .map((mirror) => ({ mirror, ms: latency.get(mirrorHost(mirror)) }))
              .filter((item): item is { mirror: Mirror; ms: number } => typeof item.ms === "number")
              .sort((a, b) => a.ms - b.ms)[0];
            return fastest ? [{ tool, mirror: fastest.mirror }] : [];
          });
          const fastestJava = hasJava
            ? (Object.keys(JAVA_VENDOR_SOURCES) as Array<keyof typeof JAVA_VENDOR_SOURCES>)
              .map((id) => ({ id, ms: latency.get(JAVA_VENDOR_SOURCES[id].host) }))
              .filter((item): item is { id: keyof typeof JAVA_VENDOR_SOURCES; ms: number } => typeof item.ms === "number")
              .sort((a, b) => a.ms - b.ms)[0]
            : undefined;
          if (!selected.length && !fastestJava) return { applied: 0, available: false, tools: null as ToolState[] | null };

          const proxy = await currentProxy();
          let applied = 0;
          if (fastestJava && localStorage.getItem(JAVA_VENDOR_STORAGE) !== fastestJava.id) {
            localStorage.setItem(JAVA_VENDOR_STORAGE, fastestJava.id);
            applied++;
          }
          for (const { tool, mirror } of selected) {
            if (currentSource(tool) === mirror.id) continue;
            await applyMirror(tool, mirror.id, proxy);
            applied++;
          }
          return { applied, available: true, tools: await invoke<ToolState[]>("list_sources") };
        },
      );
      if (result.tools) publishOverview({ tools: result.tools });
      notices.checkNow("source-changed").catch(() => undefined);
      toast(!result.available
        ? "未发现可用的下载源，已保留现有配置"
        : result.applied > 0
          ? `智能优选完成，已更新 ${result.applied} 项源配置`
          : "当前配置已是本次测速的优选结果", result.available ? "ok" : "info");
    } catch (error) {
      toast("智能优选源未完成。已完成的配置已自动备份，原因：" + error, "err");
    } finally {
      setSourceBusy(false);
    }
  }

  // Every installed tool back on its official source (with a proxy, the official ones are
  // the most complete and current); each change is backed up as usual.
  async function restoreOfficial() {
    setSourceBusy(true);
    try {
      const changed = await runBusy({ title: "恢复官方源", message: "正在把各生态的下载源与仓库镜像改回官方源，现有配置会自动备份。" }, async () => {
        const proxy = await currentProxy();
        const failed: string[] = [];
        let done = 0;
        for (const tool of offOfficial) {
          try { await applyMirror(tool, "official", proxy); done++; } catch (error) { failed.push(`${tool.name}：${error}`); }
        }
        publishOverview({ tools: await invoke<ToolState[]>("list_sources") });
        if (failed.length) throw new Error(failed.join("；"));
        return done;
      });
      notices.checkNow("source-changed").catch(() => undefined);
      toast(translateText("已把 {count} 项恢复为官方源").replace("{count}", String(changed)), "ok");
    } catch (error) {
      toast("恢复官方源未全部完成：" + error, "err");
    } finally {
      setSourceBusy(false);
    }
  }

  return (
    <>
      {(
        <>
          <div className={"checkup agent" + overallClass + (checkingAll ? " checking" : "")}>
            {checkingAll && <span className="border-runner" aria-hidden="true" />}
            <span className={"cnum" + (!checkingAll && !emptySetup ? " score" : "")}>
              {checkingAll ? <i className="ti ti-loader spin" style={{ fontSize: 24 }} /> : !hasChecked ? <><b>--</b><span>分</span></> : emptySetup ? <i className="ti ti-package-off" style={{ fontSize: 26 }} /> : <><b>{overallScore}</b><span>分</span></>}
            </span>
            <div className="ct">
              <div className="t1">编程生态体检 · {overallTitle}</div>
              <div className="t2">{overallSummary}</div>
            </div>
            <div className="cacts">
              <button className="gh sm" disabled={checkingAll} onClick={reloadAll}>
                <i className={"ti " + (checkingAll ? "ti-loader spin" : hasChecked ? "ti-refresh" : "ti-player-play")} /> {checkingAll ? "体检中…" : hasChecked ? "再次体检" : "开始体检"}
              </button>
              {!(hasChecked && (allOk || emptySetup)) && <AiButton label="AI 诊断" title="把没通过的项目交给 AI，排出先修哪个、怎么修；还没体检的话会先体检"
                onClick={() => setDiagnosing(true)} />}
              {optimizeCount > 0 && <button className="pr" disabled={busy || checkingAll} onClick={optimizeAll}><i className="ti ti-tool" /> {busy ? "修复中…" : `一键修复（${optimizeCount}）`}</button>}
            </div>
          </div>
          {diagnosing && <AiAskModal title="体检诊断" sub="编程生态体检"
            waiting="正在体检并请 AI 分析…"
            note="只把没通过的检测项（名称、状态、说明）发给 AI；AI 只给建议，修复仍由你来点。"
            run={async () => {
              // Not checked yet, or a check is under way: the diagnosis waits for its result.
              if (overviewRun) await overviewRun.catch(() => undefined);
              if (!overviewCache.checked) await runOverviewCheck();
              const items = [
                ...(overviewCache.ecosystem?.ecosystems ?? []).filter((item) => item.status !== "ok")
                  .map((item) => ({ name: item.label, status: item.status, summary: item.summary, detail: item.detail })),
                ...overviewCache.extra.map((item) => ({ name: item.title, status: item.sev, detail: item.desc })),
              ];
              if (!items.length) return "体检全部通过，没有需要诊断的项目。";
              return askAi("checkup", { items });
            }}
            onClose={() => setDiagnosing(false)} />}
          {!hasChecked && (
            <>
              <div className="seclabel"><i className="ti ti-list-check" /> 待体检项目</div>
              {PENDING_CHECKS.map((item) => (
                <div className="fixrow" key={item.title}>
                  <span className="fdot info" />
                  <div className="ft">
                    <div className="fh">{item.title} <span className="bd b">{item.badge}</span></div>
                    <div className="fs">{item.desc}</div>
                  </div>
                </div>
              ))}
            </>
          )}
        </>
      )}

      <div className="seclabel"><i className="ti ti-layout-grid" /> 工作站</div>
      <WorkstationGroups onOpen={goto} />

      {hasChecked && !allOk && !emptySetup && extra.length > 0 && <div className="seclabel"><i className="ti ti-list-check" /> 可优化项</div>}

      {extra.map((e) => {
        const fixer = extraFixer(e.id);
        const directBusy = !!rowBusy[`extra:${e.id}`];
        return (
          <div className={"fixrow" + (directBusy ? " trace-card" : "")} key={e.id}>
            {directBusy && <span className="border-runner" aria-hidden="true" />}
            <span className={"fdot " + e.sev} />
            <div className="ft">
              <div className="fh">{e.title} <span className={"bd " + (e.sev === "warn" ? "r" : e.sev === "mid" ? "w" : "b")}>{e.sev === "warn" ? "注意" : e.sev === "mid" ? "建议" : "提示"}</span></div>
              <div className="fs">{e.desc}</div>
            </div>
            <span className="fixrow-ops">
              <button className={e.sev === "info" ? "gh sm" : "pr sm"} disabled={directBusy}
                onClick={!fixer ? () => openDetail(e.page) : CONFIRM_EXTRA[e.id] ? () => setConfirming(e) : () => runExtra(e.id, fixer)}>
                <i className={"ti " + (directBusy ? "ti-loader spin" : fixer ? "ti-broom" : "ti-arrow-right")} /> {directBusy ? "处理中…" : e.action}
              </button>
              {fixer && e.page === "cleanup" && <button className="gh sm" disabled={directBusy} title="打开磁盘清理，自动快速扫描，逐项查看再决定清哪些"
                onClick={() => openDetail(e.page)}><i className="ti ti-list-search" /> 查看详情</button>}
            </span>
          </div>
        );
      })}

      {confirming && <ConfirmModal title={confirming.title} icon="ti-broom" danger confirmLabel={confirming.action}
        message={CONFIRM_EXTRA[confirming.id]}
        onClose={() => setConfirming(null)}
        onConfirm={() => { const item = confirming; setConfirming(null); const fixer = extraFixer(item.id); if (fixer) void runExtra(item.id, fixer); }} />}

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-stack-2" /> 编程生态</span>
        <div className="ghr">
          <button className="gh sm" disabled={sourceBusy || checkingAll || !offOfficial.length} onClick={() => setConfirmOfficial(true)}
            title={offOfficial.length ? translateText("把 {count} 项非官方源改回官方源；挂代理时官方源最全、最新").replace("{count}", String(offOfficial.length)) : "已安装的工具都在用官方源"}>
            <i className="ti ti-world" /> 全部恢复官方源{offOfficial.length ? `（${offOfficial.length}）` : ""}
          </button>
          <button className="pr sm" disabled={sourceBusy || checkingAll || !tools?.length} onClick={optimizeSources} title="统一测试已安装生态的下载源与仓库镜像，并应用响应更快的可用源">
            <i className={"ti " + (sourceBusy ? "ti-loader spin" : "ti-route-alt-left")} /> {sourceBusy ? "处理中…" : "智能优选源"}
          </button>
        </div>
      </div>
      {!ecosystem && (statusRead && !checkingAll
        ? <div className="space-analysis-state"><i className="ti ti-alert-circle" /> 暂时读不到各生态的状态，点「开始体检」再试一次。</div>
        : <div className="space-analysis-state"><i className="ti ti-loader spin" /> 正在读取各生态的状态和下载源…</div>)}
      {confirmOfficial && <ConfirmModal title="全部恢复官方源" icon="ti-world" confirmLabel="恢复官方源"
        message={<>{translateText("将把下面 {count} 项改回官方源，现有配置会自动备份：").replace("{count}", String(offOfficial.length))}<br />{offOfficial.map((tool) => tool.name).join("、")}<br />没有代理时官方源可能较慢，可以随时再用「智能优选源」换回镜像。</>}
        onClose={() => setConfirmOfficial(false)}
        onConfirm={() => { setConfirmOfficial(false); void restoreOfficial(); }} />}
      {ecosystem && <div className="ecocards">{(ecosystem?.ecosystems ?? []).map((item) => {
        const eco = item.id;
        const meta = ECO_META[eco];
        const sourceTool = (ECOSYSTEM_SOURCE_TOOLS[eco] ?? [])
          .map((id) => tools?.find((tool) => tool.id === id))
          .find((tool): tool is ToolState => !!tool && tool.installed);
        const javaVendor = eco === "java" ? (localStorage.getItem(JAVA_VENDOR_STORAGE) || "temurin") as keyof typeof JAVA_VENDOR_SOURCES : null;
        const sourceLabel = javaVendor && JAVA_VENDOR_SOURCES[javaVendor]
          ? JAVA_VENDOR_SOURCES[javaVendor].label
          : sourceTool ? selectedSourceLabel(sourceTool) : "—";
        const pageIssue = extra.find((e) => e.page === eco && e.sev !== "info");
        const statusText = pageIssue || item.status === "warn" ? "需处理" : item.status === "missing" ? "未配置" : "正常";
        const tone = pageIssue || item.status === "warn" ? "bad" : item.status === "missing" ? "idle" : "ok";
        return (
          <button type="button" className={"ecocard " + tone} key={eco} onClick={() => goto(eco)}>
            <span className="ecocard-head">
              <span className={"av " + meta.av}><i className={"ti " + meta.icon} /></span>
              <b>{meta.label}</b>
              <em>{statusText}</em>
            </span>
            <span className="ecocard-line" title={item.detail || item.summary}>{item.summary}</span>
            <span className="ecocard-line dim" title={sourceLabel}>{sourceLabel}</span>
          </button>
        );
      })}</div>}

    </>
  );
}

const ECO_META: Record<string, { av: string; icon: string; label: string }> = {
  git: { av: "st", icon: "ti-brand-git", label: "Git" },
  python: { av: "py", icon: "ti-brand-python", label: "Python" },
  php: { av: "php", icon: "ti-brand-php", label: "PHP" },
  node: { av: "npm", icon: "ti-brand-nodejs", label: "Node.js" },
  java: { av: "jv", icon: "ti-coffee", label: "Java" },
  go: { av: "go", icon: "ti-brand-golang", label: "Go" },
  maven: { av: "mv2", icon: "ti-feather", label: "Maven" },
  gradle: { av: "gr", icon: "ti-box", label: "Gradle" },
  rust: { av: "rs", icon: "ti-brand-rust", label: "Rust" },
};
