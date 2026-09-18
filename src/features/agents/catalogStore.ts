import { invoke } from "../../invoke";

export type InstallInfo = { path: string; healthy: boolean; version?: string | null; reason?: string | null };

export type VibeSurface = {
  available: boolean;
  label: string;
  kind: string;
  description: string;
  installed: boolean;
  status: "installed" | "update" | "missing" | "broken" | "unknown" | "pending";
  version?: string | null;
  probe_error?: string | null;
  latest?: string | null;
  update_available: boolean;
  path?: string | null;
  command?: string | null;
  install_method?: string | null;
  install_method_label?: string | null;
  install_url: string;
  docs_url: string;
  can_install: boolean;
  install_unavailable_reason?: string | null;
  can_update: boolean;
  can_uninstall: boolean;
  can_open: boolean;
  health?: "healthy" | "broken" | "missing";
  broken_reason?: string | null;
  other_installs?: InstallInfo[];
  can_repair?: boolean;
  latest_error?: string | null;
};
export type VibeTool = {
  id: string;
  family_id: string;
  edition: "unified" | "cn" | "global";
  edition_label: string;
  sort_order: number;
  name: string;
  description: string;
  docs_url: string;
  icon?: string;
  workbench_command?: string | null;
  cli_id?: string | null;
  cli_note?: string | null;
  cli: VibeSurface;
  desktop: VibeSurface;
};

export function surfaceDetected(surface: VibeSurface) {
  return surface.installed || surface.health === "broken" || !!surface.path;
}

export type VibeCache = {
  tools: VibeTool[];
  loading: boolean;
  checked: boolean;
  checkedAt: string | null;
};
const VIBE_CACHE_KEY = "stacker.vibe.status.v3";

function restoreVibeCache(): VibeCache {
  try {
    const stored = JSON.parse(localStorage.getItem(VIBE_CACHE_KEY) || "null") as Partial<VibeCache> | null;
    if (Array.isArray(stored?.tools) && stored.tools.length > 0) {
      return { tools: stored.tools, loading: false, checked: Boolean(stored.checked), checkedAt: stored.checkedAt || null };
    }
  } catch {
    // Invalid or outdated cache is ignored; the lightweight catalog will replace it.
  }
  return { tools: [], loading: false, checked: false, checkedAt: null };
}

const VIBE_INITIAL: VibeCache = restoreVibeCache();
let vibeCache: VibeCache = VIBE_INITIAL;
let vibeRun: Promise<void> | null = null;
const vibeListeners = new Set<(s: VibeCache) => void>();

function publishVibe(next: Partial<VibeCache>) {
  vibeCache = { ...vibeCache, ...next };
  if (vibeCache.tools.length > 0) {
    try {
      localStorage.setItem(VIBE_CACHE_KEY, JSON.stringify({
        tools: vibeCache.tools,
        checked: vibeCache.checked,
        checkedAt: vibeCache.checkedAt,
      }));
    } catch {
      // A storage failure must not block environment management.
    }
  }
  vibeListeners.forEach((fn) => fn(vibeCache));
}

export function vibeSnapshot() {
  return vibeCache;
}

export function subscribeVibe(fn: (s: VibeCache) => void) {
  vibeListeners.add(fn);
  return () => { vibeListeners.delete(fn); };
}

export function runVibeCheck(force = false) {
  if (vibeRun) return vibeRun;
  publishVibe({ loading: true });
  vibeRun = (async () => {
    try {
      const tools = await invoke<VibeTool[]>(force ? "vibe_tools_refresh" : "vibe_tools");
      publishVibe({ tools, checked: true, checkedAt: new Date().toISOString() });
    } finally {
      publishVibe({ loading: false });
      vibeRun = null;
    }
  })();
  return vibeRun;
}

export async function refreshOneTool(id: string) {
  const next = await invoke<VibeTool>("vibe_tool", { id });
  const current = vibeCache.tools;
  const exists = current.some((tool) => tool.id === id);
  publishVibe({
    tools: exists
      ? current.map((tool) => tool.id === id ? next : tool)
      : [...current, next],
    checkedAt: new Date().toISOString(),
  });
  return next;
}

export async function refreshTools(ids: string[]) {
  for (const id of new Set(ids)) await refreshOneTool(id);
}

/** Loads the lightweight catalog and keeps previously detected states. */
export async function loadCatalog() {
  const catalog = await invoke<VibeTool[]>("vibe_catalog");
  const previous = new Map(vibeCache.tools.map((tool) => [tool.id, tool]));
  publishVibe({ tools: catalog.map((tool) => {
    const cached = previous.get(tool.id);
    if (!cached) return tool;
    // Keep a cached detection only while the catalog still offers that surface.
    return {
      ...tool,
      cli: cached.cli.available === tool.cli.available ? cached.cli : tool.cli,
      desktop: cached.desktop.available === tool.desktop.available ? cached.desktop : tool.desktop,
    };
  }) });
}
