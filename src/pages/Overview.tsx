import { useEffect, useState } from "react";
import { invoke } from "../invoke";
import type { Page } from "../App";
import { useBusy, useToast } from "../ui";
import { useNotifications } from "../notifications";

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
      return async () => { await invoke("proxy_disable", { alsoJvm: false }); return "已关闭终端代理"; };
    case "cache_safe_high":
      return async () => { const freed = await invoke<number>("cleanup_delete_safe"); return `已清理安全缓存，释放 ${(freed / 1073741824).toFixed(1)} GB`; };
    default:
      return null;
  }
}
// 纳入「一键优化全部」的 extra 项：仅安全、可还原、免提权的（fnm 集成）；
// 缓存清理（删除）、关代理这些副作用项只给各自的行内按钮，不卷进批量。
const BATCH_EXTRA = new Set(["fnm_no_integration"]);

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

  useEffect(() => subscribeOverview((s) => {
    setTools(s.tools);
    setExtra(s.extra);
    setEcosystem(s.ecosystem);
    setChecking(s.checking);
    setChecked(s.checked);
  }), []);

  async function load() {
    return runOverviewCheck();
  }
  async function reloadAll() {
    const wasChecked = hasChecked;
    try {
      await load();
      toast(wasChecked ? "开发环境体检已完成" : "开发环境体检完成", "ok");
    } catch (e) {
      toast("体检失败：" + e, "err");
    }
  }

  const batchExtra = extra.filter((e) => BATCH_EXTRA.has(e.id));
  const optimizeCount = batchExtra.length;
  const hasChecked = checked || tools !== null || ecosystem !== null;
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
  const overallTitle = !hasChecked ? "未开始"
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
        if (page) {
          const refreshed = await invoke<CheckItem[]>("checkup_page", { page });
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
      for (const e of batchExtra) {
        const f = extraFixer(e.id);
        if (f) { await f(); done++; }
      }
      await load();
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

          const [proxyStatus, mavenProxy, gradleProxy] = await Promise.all([
            invoke<ProxyStatus>("proxy_status").catch(() => ({} as ProxyStatus)),
            invoke<boolean>("source_proxy_state", { toolId: "maven", path: null }).catch(() => false),
            invoke<boolean>("source_proxy_state", { toolId: "gradle", path: null }).catch(() => false),
          ]);
          const proxyHost = proxyStatus.host || "127.0.0.1";
          const proxyPort = proxyStatus.port || proxyStatus.detected_port || 7890;
          let applied = 0;
          if (fastestJava && localStorage.getItem(JAVA_VENDOR_STORAGE) !== fastestJava.id) {
            localStorage.setItem(JAVA_VENDOR_STORAGE, fastestJava.id);
            applied++;
          }
          for (const { tool, mirror } of selected) {
            const storageKey = RUNTIME_SOURCE_KEYS[tool.id];
            const current = storageKey ? (localStorage.getItem(storageKey) || "official") : tool.current;
            if (current === mirror.id) continue;
            if (storageKey) {
              localStorage.setItem(storageKey, mirror.id);
            } else if (tool.id === "go") {
              await invoke("apply_source_scoped", { toolId: tool.id, mirrorId: mirror.id, scope: "user" });
            } else if (tool.id === "maven" || tool.id === "gradle") {
              await invoke("apply_source", {
                toolId: tool.id,
                mirrorId: mirror.id,
                proxyEnabled: tool.id === "maven" ? mavenProxy : gradleProxy,
                proxyHost,
                proxyPort,
              });
            } else {
              await invoke("apply_source", { toolId: tool.id, mirrorId: mirror.id });
            }
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
              <div className="t1">生态环境体检 · {overallTitle}</div>
              <div className="t2">{overallSummary}</div>
            </div>
            <div className="cacts">
              <button className="gh sm" disabled={checkingAll} onClick={reloadAll}>
                <i className={"ti " + (checkingAll ? "ti-loader spin" : hasChecked ? "ti-refresh" : "ti-player-play")} /> {checkingAll ? "体检中…" : hasChecked ? "再次体检" : "开始体检"}
              </button>
              {optimizeCount > 0 && <button className="pr" disabled={busy || checkingAll} onClick={optimizeAll}><i className="ti ti-tool" /> {busy ? "修复中…" : `一键修复（${optimizeCount}）`}</button>}
            </div>
          </div>
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
            <button className={e.sev === "info" ? "gh sm" : "pr sm"} disabled={directBusy}
              onClick={fixer ? () => runExtra(e.id, fixer) : () => goto(e.page)}>
              {fixer && <i className={"ti " + (directBusy ? "ti-loader spin" : "ti-broom")} />} {directBusy ? "处理中…" : e.action}
            </button>
          </div>
        );
      })}

      {hasChecked && <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-stack-2" /> 编程生态</span>
        <div className="ghr">
          <button className="pr sm" disabled={sourceBusy || checkingAll || !tools?.length} onClick={optimizeSources} title="统一测试已安装生态的下载源与仓库镜像，并应用响应更快的可用源">
            <i className={"ti " + (sourceBusy ? "ti-loader spin" : "ti-route-alt-left")} /> {sourceBusy ? "优选中…" : "智能优选源"}
          </button>
        </div>
      </div>}
      {hasChecked && (ecosystem?.ecosystems ?? []).map((item) => {
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
        const statusColor = pageIssue || item.status === "warn" ? "#ef6f6f" : item.status === "missing" ? "#828995" : "#6bcf86";
        return (
          <div className="ecorow" key={eco} onClick={() => goto(eco)}>
            <span className={"av " + meta.av + " big"}><i className={"ti " + meta.icon} /></span>
            <div className="ecocols">
              <div className="ecocell"><div className="k">生态</div><div className="v">{meta.label}</div></div>
              <div className="ecocell"><div className="k">当前环境</div><div className="v" title={item.detail}>{item.summary}</div></div>
              <div className="ecocell"><div className="k">环境源</div><div className="v" title={sourceLabel}>{sourceLabel}</div></div>
              <div className="ecocell"><div className="k">状态</div><div className="v" style={{ color: statusColor }}>{statusText}</div></div>
            </div>
            <i className="ti ti-chevron-right chev" />
          </div>
        );
      })}

    </>
  );
}

const ECO_META: Record<string, { av: string; icon: string; label: string }> = {
  git: { av: "st", icon: "ti-brand-git", label: "Git" },
  python: { av: "py", icon: "ti-brand-python", label: "Python" },
  node: { av: "npm", icon: "ti-brand-nodejs", label: "Node.js" },
  java: { av: "jv", icon: "ti-coffee", label: "Java" },
  go: { av: "go", icon: "ti-brand-golang", label: "Go" },
  maven: { av: "mv2", icon: "ti-feather", label: "Maven" },
  gradle: { av: "gr", icon: "ti-box", label: "Gradle" },
  rust: { av: "rs", icon: "ti-brand-rust", label: "Rust" },
};
