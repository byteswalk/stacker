import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "./invoke";
import { ecosystemUpdateFromInfo, type VersionUpdateInfo } from "./updateHelpers";

type UpdateInfo = {
  current: string;
  latest: string;
  has_update: boolean;
  release_url?: string | null;
  installer_url?: string | null;
  portable_url?: string | null;
  published_at?: string | null;
  notes: string[];
};
type MirrorsUpdateCheck = {
  url: string;
  local_version: string | null;
  remote_version: string;
  has_update: boolean;
  tools: number;
};
type CacheItem = { id: string; name: string; path: string; size: number; category: string; icon: string; av: string };
type CatalogTool = { id: string; mirrors: CatalogMirror[] };
type CatalogMirror = { id: string; name: string; url: string; host: string };
type SdkVersion = { kind: string; version: string; vendor: string; path: string; current: boolean; arch?: string };
type SdkGroup = { kind: string; label: string; current_desc: string; versions: SdkVersion[] };
type JdkAsset = { version: string; filename: string; url: string };
type PyenvStatus = { installed: boolean; versions: { version: string; is_default: boolean }[]; default: string | null };
type FnmStatus = { installed: boolean; versions: { version: string; is_default: boolean }[]; default: string | null };
type RustupStatus = {
  installed: boolean;
  toolchains: { name: string; is_default: boolean }[];
  default: string | null;
  default_version?: string | null;
};
type GitStatus = { installed: boolean };
type CheckItem = { id: string; sev: string; title: string; desc: string; page: string; action: string };
type VibeSurface = { label: string; version?: string | null; latest?: string | null; update_available: boolean };
type VibeTool = { id: string; name: string; cli: VibeSurface; desktop: VibeSurface };

export type NotificationPrefs = {
  enabled: boolean;
  appUpdate: boolean;
  sourceUpdate: boolean;
  cleanup: boolean;
  ecosystemUpdate: boolean;
  environmentIssue: boolean;
  intervalMinutes: number;
  cleanupThresholdGb: number;
};

type EcosystemUpdate = {
  id: string;
  name: string;
  current: string;
  latest: string;
  source: string;
};
type AiToolUpdate = {
  page: "vibe";
  id: string;
  name: string;
  current: string;
  latest: string;
};

type NotificationState = {
  prefs: NotificationPrefs;
  checking: boolean;
  lastChecked: string | null;
  appUpdate: UpdateInfo | null;
  sourceUpdate: MirrorsUpdateCheck | null;
  cleanupBytes: number;
  ecosystemUpdates: EcosystemUpdate[];
  aiToolUpdates: AiToolUpdate[];
  environmentIssues: CheckItem[];
  pageNoticeCounts: Record<string, number>;
  count: number;
  settingsCount: number;
  cleanupCount: number;
};

type NotificationContextValue = NotificationState & {
  setPrefs: (prefs: NotificationPrefs) => void;
  checkNow: (reason?: string) => Promise<void>;
};

const PREF_KEY = "stacker.notificationPrefs";
const GB = 1024 ** 3;

const DEFAULT_PREFS: NotificationPrefs = {
  enabled: true,
  appUpdate: true,
  sourceUpdate: true,
  cleanup: true,
  ecosystemUpdate: true,
  environmentIssue: true,
  intervalMinutes: 30,
  cleanupThresholdGb: 10,
};

const SOURCE_KEYS: Record<string, string> = {
  python: "stacker.python.downloadSource",
  node: "stacker.node.downloadSource",
  maven: "stacker.maven.downloadSource",
  gradle: "stacker.gradle.downloadSource",
  go: "stacker.go.downloadSource",
  rust: "stacker.rust.downloadSource",
};

const FILTER_KEYS = {
  python: {
    onlyStable: "stacker.python.install.onlyStable",
    latestOnly: "stacker.python.install.latestOnly",
  },
  node: {
    ltsOnly: "stacker.node.install.ltsOnly",
    latestOnly: "stacker.node.install.latestOnly",
  },
  rust: {
    onlyStable: "stacker.rust.install.onlyStable",
    latestOnly: "stacker.rust.install.latestOnly",
  },
} as const;

const RUNTIME_TOOL_IDS: Record<string, string> = {
  maven: "maven-runtime",
  gradle: "gradle-runtime",
  go: "go-runtime",
  rust: "rust-runtime",
};
const ECOSYSTEM_PAGES = new Set(["git", "python", "node", "java", "maven", "gradle", "go", "rust"]);

function readPrefs(): NotificationPrefs {
  try {
    const raw = localStorage.getItem(PREF_KEY);
    if (!raw) return DEFAULT_PREFS;
    const parsed = JSON.parse(raw) as Partial<NotificationPrefs>;
    return {
      ...DEFAULT_PREFS,
      ...parsed,
      intervalMinutes: Math.max(15, Number(parsed.intervalMinutes || DEFAULT_PREFS.intervalMinutes)),
      cleanupThresholdGb: Math.max(1, Number(parsed.cleanupThresholdGb || DEFAULT_PREFS.cleanupThresholdGb)),
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

function savePrefs(prefs: NotificationPrefs) {
  localStorage.setItem(PREF_KEY, JSON.stringify(prefs));
}

function cmpVer(a: string, b: string) {
  const nums = (v: string) => (v.match(/\d+/g) ?? []).map((n) => Number(n) || 0);
  const pa = nums(a);
  const pb = nums(b);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d) return d;
  }
  return 0;
}

function stableVersion(v: string) {
  return /^\d+(?:\.\d+){1,2}$/.test(v.replace(/^v/, ""));
}

function exactStableVersion(v: string) {
  return /^\d+\.\d+\.\d+$/.test(v.replace(/^v/, ""));
}

function boolFromStorage(key: string, fallback: boolean) {
  const value = localStorage.getItem(key);
  if (value === null) return fallback;
  return value === "true";
}

function versionLineKey(v: string, parts = 2) {
  const nums = (v.replace(/^go/, "").replace(/^v/, "").match(/\d+/g) ?? []).slice(0, parts);
  return nums.length ? nums.join(".") : v;
}

function latestByFilter(list: string[], opts: { onlyStable: boolean; latestOnly: boolean; groupParts?: number }) {
  let rows = list.map((v) => v.replace(/^go/, "").replace(/^v/, "")).filter(Boolean);
  if (opts.onlyStable) rows = rows.filter(stableVersion);
  if (opts.latestOnly) {
    const best = new Map<string, string>();
    for (const v of rows) {
      const key = versionLineKey(v, opts.groupParts ?? 2);
      const current = best.get(key);
      if (!current || cmpVer(v, current) > 0) best.set(key, v);
    }
    rows = [...best.values()];
  }
  return rows.sort((a, b) => cmpVer(b, a))[0] ?? null;
}

function currentVersion(values: string[]) {
  return values
    .map((v) => v.replace(/^go/, "").replace(/^v/, ""))
    .filter(Boolean)
    .sort((a, b) => cmpVer(b, a))[0] ?? "";
}

function sourceFromStorage(kind: string) {
  return localStorage.getItem(SOURCE_KEYS[kind]) || "official";
}

function sourceUrl(tools: CatalogTool[], toolId: string, sourceId: string) {
  return tools.find((tool) => tool.id === toolId)?.mirrors.find((mirror) => mirror.id === sourceId)?.url ?? "";
}

function javaMajor(version: string) {
  const nums = version.match(/\d+/g) ?? [];
  if (nums[0] === "1" && nums[1]) return nums[1];
  return nums[0] ?? "";
}

function normalizeJavaVersion(version: string) {
  return version.replace(/^jdk-?/i, "").replace(/[_+].*$/, "");
}

function javaComparable(version: string) {
  const nums = version.match(/\d+/g) ?? [];
  if (nums[0] === "1" && nums[1] === "8") return `8.${nums[3] ?? nums[2] ?? "0"}`;
  if (/^8u/i.test(version)) return `8.${nums[1] ?? nums[0] ?? "0"}`;
  return normalizeJavaVersion(version);
}

function javaArchParam(arch?: string) {
  return arch === "x86" ? "x32" : "x64";
}

async function latestJavaFor(current: SdkVersion): Promise<{ latest: string; comparable: string; source: string } | null> {
  const major = javaMajor(current.version);
  if (!major) return null;
  const vendor = current.vendor.toLowerCase();
  const arch = javaArchParam(current.arch);
  if (vendor.includes("zulu") || vendor.includes("azul")) {
    const asset = await invoke<JdkAsset>("zulu_resolve", { major, arch });
    return { latest: normalizeJavaVersion(asset.version), comparable: javaComparable(asset.version), source: "zulu" };
  }
  if (vendor.includes("dragonwell") || vendor.includes("alibaba")) {
    const asset = await invoke<JdkAsset>("dragonwell_resolve", { major });
    return { latest: normalizeJavaVersion(asset.version), comparable: javaComparable(asset.version), source: "dragonwell" };
  }
  if (vendor.includes("temurin") || vendor.includes("adoptium")) {
    const asset = await invoke<JdkAsset>("jdk_resolve", { major, arch });
    return { latest: normalizeJavaVersion(asset.version), comparable: javaComparable(asset.version), source: "temurin" };
  }
  return null;
}

async function checkEcosystemUpdates(onlyId?: string): Promise<EcosystemUpdate[]> {
  const updates: EcosystemUpdate[] = [];
  const tools = await invoke<CatalogTool[]>("list_sources").catch(() => [] as CatalogTool[]);
  async function read<T>(task: Promise<T>, fallback: T, id: string): Promise<T> {
    try {
      return await task;
    } catch (error) {
      if (onlyId === id) throw error;
      return fallback;
    }
  }

  async function pushIfNewer(id: string, name: string, current: string, latest: string | null, source: string) {
    if (!current || !latest || cmpVer(current, latest) >= 0) return;
    updates.push({ id, name, current, latest, source });
  }

  if (!onlyId || onlyId === "git") {
    const status = await read<GitStatus | null>(invoke<GitStatus>("git_status"), null, "git");
    if (status?.installed) {
      const sourceId = localStorage.getItem("stacker.git.downloadSource") || "official";
      const info = await read<VersionUpdateInfo | null>(
        invoke<VersionUpdateInfo>("git_check_update", { sourceId }),
        null,
        "git",
      );
      const update = ecosystemUpdateFromInfo("git", "Git", info);
      if (update) updates.push(update);
    }
  }

  const pySource = sourceFromStorage("python");
  const py = !onlyId || onlyId === "python"
    ? await read<PyenvStatus | null>(invoke<PyenvStatus>("pyenv_status"), null, "python")
    : null;
  if (py?.installed) {
    const onlyStable = boolFromStorage(FILTER_KEYS.python.onlyStable, true);
    const latestOnly = boolFromStorage(FILTER_KEYS.python.latestOnly, false);
    const rows = await read(invoke<string[]>("pyenv_install_list", { source: pySource, includePrerelease: !onlyStable }), [], "python");
    await pushIfNewer("python", "Python", currentVersion(py.versions.map((v) => v.version)), latestByFilter(rows, { onlyStable, latestOnly }), pySource);
  }

  const nodeSource = sourceFromStorage("node");
  const node = !onlyId || onlyId === "node"
    ? await read<FnmStatus | null>(invoke<FnmStatus>("fnm_status"), null, "node")
    : null;
  if (node?.installed) {
    const ltsOnly = boolFromStorage(FILTER_KEYS.node.ltsOnly, true);
    const latestOnly = boolFromStorage(FILTER_KEYS.node.latestOnly, false);
    const rows = await read(invoke<string[]>("fnm_ls_remote", { ltsOnly, source: nodeSource }), [], "node");
    await pushIfNewer("node", "Node.js", currentVersion(node.versions.map((v) => v.version)), latestByFilter(rows, { onlyStable: true, latestOnly, groupParts: 1 }), nodeSource);
  }

  const needsSdkGroups = !onlyId || ["java", "maven", "gradle", "go"].includes(onlyId);
  const groups = needsSdkGroups ? await invoke<SdkGroup[]>("env_state").catch(() => [] as SdkGroup[]) : [];
  const javaCurrent = groups.find((g) => g.kind === "java")?.versions.find((v) => v.current);
  if ((!onlyId || onlyId === "java") && javaCurrent?.version) {
    const latest = onlyId === "java" ? await latestJavaFor(javaCurrent) : await latestJavaFor(javaCurrent).catch(() => null);
    if (latest && cmpVer(javaComparable(javaCurrent.version), latest.comparable) < 0) {
      updates.push({
        id: "java",
        name: "Java",
        current: normalizeJavaVersion(javaCurrent.version),
        latest: latest.latest,
        source: latest.source,
      });
    }
  }

  for (const kind of ["maven", "gradle", "go"] as const) {
    if (onlyId && onlyId !== kind) continue;
    const group = groups.find((g) => g.kind === kind);
    const cur = group?.versions.find((v) => v.current)?.version ?? currentVersion(group?.versions.map((v) => v.version) ?? []);
    if (!cur) continue;
    const source = sourceFromStorage(kind);
    const rows = await read(invoke<string[]>(`${kind}_versions`, {
      source,
      sourceUrl: sourceUrl(tools, RUNTIME_TOOL_IDS[kind], source),
    }), [], kind);
    const onlyStable = boolFromStorage(`stacker.${kind}.install.onlyStable`, true);
    const latestOnly = boolFromStorage(`stacker.${kind}.install.latestOnly`, true);
    await pushIfNewer(kind, kind === "go" ? "Go" : kind === "maven" ? "Maven" : "Gradle", cur, latestByFilter(rows, { onlyStable, latestOnly }), source);
  }

  const rustSource = sourceFromStorage("rust");
  const rust = !onlyId || onlyId === "rust"
    ? await read<RustupStatus | null>(invoke<RustupStatus>("rustup_status"), null, "rust")
    : null;
  if (rust?.installed) {
    const source = sourceUrl(tools, RUNTIME_TOOL_IDS.rust, rustSource);
    const rows = await read(invoke<string[]>("rust_versions", { sourceUrl: source }), [], "rust");
    const onlyStable = boolFromStorage(FILTER_KEYS.rust.onlyStable, true);
    const latestOnly = boolFromStorage(FILTER_KEYS.rust.latestOnly, true);
    const currentDefault =
      rust.default_version ??
      rust.toolchains.find((toolchain) => toolchain.is_default)?.name ??
      rust.default ??
      "";
    const currentDefaultVersion = exactStableVersion(currentDefault) ? currentDefault.replace(/^v/, "") : "";
    await pushIfNewer("rust", "Rust", currentDefaultVersion, latestByFilter(rows, { onlyStable, latestOnly }), rustSource);
  }

  return updates;
}

async function checkAiToolUpdates(): Promise<AiToolUpdate[]> {
  const tools = await invoke<VibeTool[]>("vibe_tools");
  return tools.flatMap((tool) => {
    const rows: AiToolUpdate[] = [];
    for (const surface of [tool.cli, tool.desktop]) {
      if (!surface.update_available) continue;
      rows.push({
        page: "vibe",
        id: `${tool.id}:${surface.label}`,
        name: `${tool.name} ${surface.label}`,
        current: surface.version || "已安装",
        latest: surface.latest || "新版本",
      });
    }
    return rows;
  });
}

function ecosystemIssueItems(items: CheckItem[]) {
  return items.filter((item) => ECOSYSTEM_PAGES.has(item.page) && item.sev !== "info");
}

type Attempt<T> = { ok: true; value: T } | { ok: false };
async function attempt<T>(task: Promise<T>): Promise<Attempt<T>> {
  try {
    return { ok: true, value: await task };
  } catch {
    return { ok: false };
  }
}

function reasonPage(reason?: string) {
  const page = reason?.split("-", 1)[0] ?? "";
  return page === "vibe" || ECOSYSTEM_PAGES.has(page) ? page : null;
}

const NotificationCtx = createContext<NotificationContextValue>({
  prefs: DEFAULT_PREFS,
  checking: false,
  lastChecked: null,
  appUpdate: null,
  sourceUpdate: null,
  cleanupBytes: 0,
  ecosystemUpdates: [],
  aiToolUpdates: [],
  environmentIssues: [],
  pageNoticeCounts: {},
  count: 0,
  settingsCount: 0,
  cleanupCount: 0,
  setPrefs: () => {},
  checkNow: async () => {},
});

export function NotificationProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefsState] = useState<NotificationPrefs>(readPrefs);
  const [checking, setChecking] = useState(false);
  const [lastChecked, setLastChecked] = useState<string | null>(null);
  const [appUpdate, setAppUpdate] = useState<UpdateInfo | null>(null);
  const [sourceUpdate, setSourceUpdate] = useState<MirrorsUpdateCheck | null>(null);
  const [cleanupBytes, setCleanupBytes] = useState(0);
  const [ecosystemUpdates, setEcosystemUpdates] = useState<EcosystemUpdate[]>([]);
  const [aiToolUpdates, setAiToolUpdates] = useState<AiToolUpdate[]>([]);
  const [environmentIssues, setEnvironmentIssues] = useState<CheckItem[]>([]);
  const runRef = useRef<Promise<void> | null>(null);
  const queuedReasonsRef = useRef<string[]>([]);
  const checkNowRef = useRef<(reason?: string) => Promise<void>>(async () => {});

  const setPrefs = useCallback((next: NotificationPrefs) => {
    const cleaned = {
      ...next,
      intervalMinutes: Math.max(15, Number(next.intervalMinutes || DEFAULT_PREFS.intervalMinutes)),
      cleanupThresholdGb: Math.max(1, Number(next.cleanupThresholdGb || DEFAULT_PREFS.cleanupThresholdGb)),
    };
    savePrefs(cleaned);
    setPrefsState(cleaned);
  }, []);

  const checkNow = useCallback(async (reason?: string) => {
    if (!prefs.enabled) return;
    if (runRef.current) {
      const queued = reason ?? "background";
      if (!queuedReasonsRef.current.includes(queued)) queuedReasonsRef.current.push(queued);
      return runRef.current;
    }
    const page = reasonPage(reason);
    const full = !reason || reason === "manual" || reason === "background";
    const settingsOnly = reason === "settings";
    const sourcesChanged = reason?.startsWith("source-") ?? false;
    const cleanupOnly = reason?.startsWith("cleanup") ?? false;
    setChecking(true);
    runRef.current = (async () => {
      let succeeded = false;
      if ((full || settingsOnly) && prefs.appUpdate) {
        const result = await attempt(invoke<UpdateInfo>("app_check_update"));
        if (result.ok) {
          setAppUpdate(result.value.has_update ? result.value : null);
          succeeded = true;
        }
      }
      if ((full || settingsOnly || sourcesChanged) && prefs.sourceUpdate) {
        const result = await attempt(invoke<MirrorsUpdateCheck>("mirrors_check_update", { url: null }));
        if (result.ok) {
          setSourceUpdate(result.value.has_update ? result.value : null);
          succeeded = true;
        }
      }
      if ((full || cleanupOnly) && prefs.cleanup) {
        const result = await attempt(invoke<CacheItem[]>("cleanup_scan"));
        if (result.ok) {
          setCleanupBytes(result.value.filter((item) => item.category === "safe").reduce((sum, item) => sum + item.size, 0));
          succeeded = true;
        }
      }
      if (prefs.ecosystemUpdate && (full || sourcesChanged || (page && page !== "vibe"))) {
        const target = full || sourcesChanged ? undefined : page ?? undefined;
        const result = await attempt(checkEcosystemUpdates(target));
        if (result.ok) {
          setEcosystemUpdates((previous) => target
            ? [...previous.filter((item) => item.id !== target), ...result.value]
            : result.value);
          succeeded = true;
        }
      }
      if (prefs.ecosystemUpdate && (full || page === "vibe")) {
        const result = await attempt(checkAiToolUpdates());
        if (result.ok) {
          setAiToolUpdates(result.value);
          succeeded = true;
        }
      }
      if (prefs.environmentIssue && (full || page)) {
        const task = page
          ? invoke<CheckItem[]>("checkup_page", { page })
          : invoke<CheckItem[]>("checkup_extra").then(ecosystemIssueItems);
        const result = await attempt(task);
        if (result.ok) {
          setEnvironmentIssues((previous) => page
            ? [...previous.filter((item) => item.page !== page), ...result.value.filter((item) => item.page === page)]
            : result.value);
          succeeded = true;
        }
      }
      if (succeeded) setLastChecked(new Date().toISOString());
    })().finally(() => {
      setChecking(false);
      runRef.current = null;
      const next = queuedReasonsRef.current.shift();
      if (next) window.setTimeout(() => { checkNowRef.current(next).catch(() => undefined); }, 0);
    });
    return runRef.current;
  }, [prefs]);

  useEffect(() => {
    checkNowRef.current = checkNow;
  }, [checkNow]);

  useEffect(() => {
    const start = window.setTimeout(() => { checkNow("background").catch(() => undefined); }, 2500);
    return () => window.clearTimeout(start);
  }, [checkNow]);

  useEffect(() => {
    if (!prefs.enabled) return;
    const ms = Math.max(15, prefs.intervalMinutes) * 60 * 1000;
    const timer = window.setInterval(() => { checkNow("background").catch(() => undefined); }, ms);
    return () => window.clearInterval(timer);
  }, [checkNow, prefs.enabled, prefs.intervalMinutes]);

  const cleanupCount = prefs.cleanup && cleanupBytes >= prefs.cleanupThresholdGb * GB ? 1 : 0;
  const settingsCount = (appUpdate ? 1 : 0) + (sourceUpdate ? 1 : 0);
  const pageNoticeCounts = [...ecosystemUpdates, ...aiToolUpdates, ...environmentIssues].reduce<Record<string, number>>((acc, item) => {
    const page = "page" in item ? item.page : item.id;
    acc[page] = (acc[page] ?? 0) + 1;
    return acc;
  }, {});
  const pageNoticeTotal = Object.values(pageNoticeCounts).reduce((sum, n) => sum + n, 0);
  const count = settingsCount + cleanupCount + pageNoticeTotal;

  const value = useMemo<NotificationContextValue>(() => ({
    prefs,
    checking,
    lastChecked,
    appUpdate,
    sourceUpdate,
    cleanupBytes,
    ecosystemUpdates,
    aiToolUpdates,
    environmentIssues,
    pageNoticeCounts,
    count,
    settingsCount,
    cleanupCount,
    setPrefs,
    checkNow,
  }), [prefs, checking, lastChecked, appUpdate, sourceUpdate, cleanupBytes, ecosystemUpdates, aiToolUpdates, environmentIssues, pageNoticeCounts, count, settingsCount, cleanupCount, setPrefs, checkNow]);

  return <NotificationCtx.Provider value={value}>{children}</NotificationCtx.Provider>;
}

export function useNotifications() {
  return useContext(NotificationCtx);
}

export function formatBytes(b: number) {
  if (b >= 1024 ** 3) return (b / 1024 ** 3).toFixed(1) + " GB";
  if (b >= 1024 ** 2) return (b / 1024 ** 2).toFixed(0) + " MB";
  if (b >= 1024) return (b / 1024).toFixed(0) + " KB";
  return b + " B";
}
